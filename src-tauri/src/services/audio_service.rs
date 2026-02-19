use std::borrow::Cow;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::{Emitter, Manager};

use crate::services::funasr_service;
use crate::state::{AppState, RecordingSession};
use crate::utils::AppError;

// ---------- 常量 ----------

const TARGET_SAMPLE_RATE: u32 = 16000;
const MIN_AUDIO_DURATION_SEC: f64 = 0.5;
const MIN_SAMPLES_GROWTH: usize = 1024;

const INTERIM_INTERVAL_MIN_MS: u64 = 140;
const INTERIM_INTERVAL_BASE_MS: u64 = 220;
const INTERIM_INTERVAL_MAX_MS: u64 = 460;
const INTERIM_INTERVAL_DOWN_STEP_MS: u64 = 24;
const INTERIM_INTERVAL_UP_STEP_MS: u64 = 42;
const INTERIM_HEAVY_COST_MS: u64 = 420;
const INTERIM_LIGHT_COST_MS: u64 = 180;

const RESULT_HIDE_DELAY_MS: u64 = 2500;
const EMPTY_RESULT_HIDE_DELAY_MS: u64 = 360;
const PASTE_DELAY_MS: u64 = 260;
const AUDIO_CAPTURE_INIT_TIMEOUT_SECS: u64 = 8;

// ---------- WAV 编码 ----------

pub fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_size = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2; // mono 16-bit
    let mut buf = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_size).to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt sub-chunk: 16-bit mono PCM
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data sub-chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    for &s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }

    buf
}

// ---------- 重采样 ----------

fn resample_to_16k<'a>(input: &'a [i16], input_rate: u32) -> Cow<'a, [i16]> {
    if input.is_empty() || input_rate == 0 {
        return Cow::Borrowed(input);
    }
    if input_rate == TARGET_SAMPLE_RATE {
        return Cow::Borrowed(input);
    }
    let ratio = input_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let new_len = (input.len() as f64 / ratio).round() as usize;
    let output: Vec<i16> = (0..new_len)
        .map(|i| {
            let src_idx = i as f64 * ratio;
            let low = src_idx.floor() as usize;
            let high = (low + 1).min(input.len().saturating_sub(1));
            let frac = src_idx - low as f64;
            (input[low] as f64 * (1.0 - frac) + input[high] as f64 * frac).round() as i16
        })
        .collect();
    Cow::Owned(output)
}

// ---------- cpal 音频捕获 ----------

/// 启动音频捕获线程。全部设备逻辑在线程内完成，sample_rate 通过 channel 回传。
pub fn spawn_audio_capture_thread(
    stop_flag: Arc<AtomicBool>,
    samples: Arc<std::sync::Mutex<Vec<i16>>>,
) -> Result<(std::thread::JoinHandle<()>, u32), AppError> {
    let (rate_tx, rate_rx) = std::sync::mpsc::sync_channel::<Result<u32, String>>(1);
    let stop_for_thread = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

            let host = cpal::default_host();
            let device = match host.default_input_device() {
                Some(d) => d,
                None => {
                    let _ = rate_tx.send(Err("未找到可用的音频输入设备".into()));
                    return;
                }
            };

            let device_name = device.name().unwrap_or_else(|_| "未知设备".into());
            log::info!("使用音频输入设备: {}", device_name);

            let supported: Vec<_> = match device.supported_input_configs() {
                Ok(iter) => iter.collect(),
                Err(e) => {
                    let _ = rate_tx.send(Err(format!("查询音频设备配置失败: {}", e)));
                    return;
                }
            };

            if supported.is_empty() {
                let _ = rate_tx.send(Err("音频设备不支持任何输入配置".into()));
                return;
            }

            let config = match find_best_config(&supported) {
                Ok(c) => c,
                Err(e) => {
                    let _ = rate_tx.send(Err(e.to_string()));
                    return;
                }
            };

            let sample_rate = config.sample_rate().0;
            let channels = config.channels() as usize;
            let sample_format = config.sample_format();

            log::info!(
                "音频配置: {}Hz, {}ch, {:?}",
                sample_rate,
                channels,
                sample_format
            );

            let err_callback = |err: cpal::StreamError| {
                log::error!("音频流错误: {}", err);
            };

            let stop_for_cb = stop_for_thread.clone();
            let stream = match sample_format {
                cpal::SampleFormat::I16 => {
                    let buf = samples.clone();
                    let stop_for_cb = stop_for_cb.clone();
                    device.build_input_stream(
                        &config.into(),
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            if stop_for_cb.load(Ordering::Relaxed) {
                                return;
                            }
                            let mut guard = match buf.lock() {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            if channels <= 1 {
                                guard.extend_from_slice(data);
                            } else {
                                guard.extend(data.chunks_exact(channels).map(|frame| {
                                    let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                                    (sum / channels as i32) as i16
                                }));
                            }
                        },
                        err_callback,
                        None,
                    )
                }
                cpal::SampleFormat::F32 => {
                    let buf = samples.clone();
                    let stop_for_cb = stop_for_cb.clone();
                    device.build_input_stream(
                        &config.into(),
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            if stop_for_cb.load(Ordering::Relaxed) {
                                return;
                            }
                            let mut guard = match buf.lock() {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            if channels <= 1 {
                                guard.extend(data.iter().map(|&s| f32_to_i16(s)));
                            } else {
                                guard.extend(data.chunks_exact(channels).map(|frame| {
                                    f32_to_i16(frame.iter().sum::<f32>() / channels as f32)
                                }));
                            }
                        },
                        err_callback,
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let buf = samples.clone();
                    let stop_for_cb = stop_for_cb.clone();
                    device.build_input_stream(
                        &config.into(),
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            if stop_for_cb.load(Ordering::Relaxed) {
                                return;
                            }
                            let mut guard = match buf.lock() {
                                Ok(g) => g,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            if channels <= 1 {
                                guard.extend(data.iter().map(|&s| u16_to_i16(s)));
                            } else {
                                guard.extend(data.chunks_exact(channels).map(|frame| {
                                    let sum: u64 = frame.iter().map(|&s| s as u64).sum();
                                    u16_to_i16((sum / channels as u64) as u16)
                                }));
                            }
                        },
                        err_callback,
                        None,
                    )
                }
                other => {
                    let _ = rate_tx.send(Err(format!("不支持的采样格式: {:?}", other)));
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let _ = rate_tx.send(Err(format!("创建音频流失败: {}", e)));
                    return;
                }
            };

            if let Err(e) = stream.play() {
                let _ = rate_tx.send(Err(format!("启动音频流失败: {}", e)));
                return;
            }

            // 通知调用方：成功启动，返回实际采样率
            let _ = rate_tx.send(Ok(sample_rate));

            while !stop_for_thread.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            drop(stream);
            log::info!("音频捕获已停止");
        })
        .map_err(|e| AppError::Other(format!("创建录音线程失败: {}", e)))?;

    // 等待线程初始化完成，拿到采样率或错误
    let sample_rate = match rate_rx.recv_timeout(std::time::Duration::from_secs(
        AUDIO_CAPTURE_INIT_TIMEOUT_SECS,
    )) {
        Ok(result) => result.map_err(AppError::Other)?,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop_flag.store(true, Ordering::Relaxed);
            return Err(AppError::Other(format!(
                "录音线程启动超时（{} 秒）",
                AUDIO_CAPTURE_INIT_TIMEOUT_SECS
            )));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return Err(AppError::Other("录音线程启动后未返回结果".into()));
        }
    };

    Ok((handle, sample_rate))
}

fn find_best_config(
    configs: &[cpal::SupportedStreamConfigRange],
) -> Result<cpal::SupportedStreamConfig, AppError> {
    use cpal::SampleFormat::{F32, I16, U16};

    let supports_16k = |c: &&cpal::SupportedStreamConfigRange| {
        c.min_sample_rate().0 <= TARGET_SAMPLE_RATE && c.max_sample_rate().0 >= TARGET_SAMPLE_RATE
    };
    let is_format = |fmt| move |c: &&cpal::SupportedStreamConfigRange| c.sample_format() == fmt;

    // 按优先级查找：i16@16k > f32@16k > u16@16k > i16@max > f32@max > u16@max > 任意@max
    let pick = configs
        .iter()
        .find(|c| is_format(I16)(c) && supports_16k(c))
        .or_else(|| {
            configs
                .iter()
                .find(|c| is_format(F32)(c) && supports_16k(c))
        })
        .or_else(|| {
            configs
                .iter()
                .find(|c| is_format(U16)(c) && supports_16k(c))
        })
        .map(|c| c.with_sample_rate(cpal::SampleRate(TARGET_SAMPLE_RATE)))
        .or_else(|| {
            configs
                .iter()
                .find(|c| is_format(I16)(c))
                .or_else(|| configs.iter().find(|c| is_format(F32)(c)))
                .or_else(|| configs.iter().find(|c| is_format(U16)(c)))
                .or(configs.first())
                .map(|c| c.with_max_sample_rate())
        });

    pick.ok_or_else(|| AppError::Other("无法找到合适的音频输入配置".into()))
}

fn f32_to_i16(s: f32) -> i16 {
    let clamped = s.clamp(-1.0, 1.0);
    if clamped < 0.0 {
        (clamped * 32768.0) as i16
    } else {
        (clamped * 32767.0) as i16
    }
}

fn u16_to_i16(s: u16) -> i16 {
    // 无符号 PCM16（0..65535）映射到有符号 PCM16（-32768..32767）
    (s as i32 - 32768) as i16
}

// ---------- 中间转写循环 ----------

pub fn spawn_interim_loop(
    app_handle: tauri::AppHandle,
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    samples: Arc<std::sync::Mutex<Vec<i16>>>,
    sample_rate: u32,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        let mut interval_ms = INTERIM_INTERVAL_BASE_MS;
        let mut last_sample_count: usize = 0;
        let mut samples_lock_poisoned_logged = false;

        if sample_rate == 0 {
            log::error!("中间转写启动失败：采样率为 0 (session {})", session_id);
            return;
        }

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;

            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let (current_samples, current_count) = {
                let guard = match samples.lock() {
                    Ok(g) => g,
                    Err(poisoned) => {
                        if !samples_lock_poisoned_logged {
                            log::warn!("采样缓冲区锁已污染，继续使用恢复后的数据");
                            samples_lock_poisoned_logged = true;
                        }
                        poisoned.into_inner()
                    }
                };
                let count = guard.len();
                if count.saturating_sub(last_sample_count) < MIN_SAMPLES_GROWTH {
                    interval_ms = adjust_interval(interval_ms, false, 0);
                    continue;
                }
                if (count as f64 / sample_rate as f64) < MIN_AUDIO_DURATION_SEC {
                    continue;
                }
                (guard.clone(), count)
            };

            let start = std::time::Instant::now();

            let resampled = resample_to_16k(&current_samples, sample_rate);
            let wav_bytes = encode_wav(&resampled, TARGET_SAMPLE_RATE);

            match funasr_service::transcribe(state.inner(), wav_bytes, &app_handle).await {
                Ok(result) if result.success && !result.text.is_empty() => {
                    let _ = app_handle.emit(
                        "transcription-result",
                        serde_json::json!({
                            "sessionId": session_id,
                            "text": result.text,
                            "interim": true,
                        }),
                    );
                    last_sample_count = current_count;
                }
                _ => {}
            }

            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let elapsed_ms = start.elapsed().as_millis() as u64;
            interval_ms = adjust_interval(interval_ms, true, elapsed_ms);
        }

        log::info!("中间转写循环结束 (session {})", session_id);
    })
}

fn adjust_interval(current: u64, executed: bool, elapsed_ms: u64) -> u64 {
    if !executed {
        return current
            .saturating_sub(8)
            .clamp(INTERIM_INTERVAL_MIN_MS, INTERIM_INTERVAL_BASE_MS);
    }

    if elapsed_ms >= INTERIM_HEAVY_COST_MS {
        (current + INTERIM_INTERVAL_UP_STEP_MS).min(INTERIM_INTERVAL_MAX_MS)
    } else if elapsed_ms <= INTERIM_LIGHT_COST_MS {
        current
            .saturating_sub(INTERIM_INTERVAL_DOWN_STEP_MS)
            .max(INTERIM_INTERVAL_MIN_MS)
    } else {
        // 向 BASE 靠拢
        match current.cmp(&INTERIM_INTERVAL_BASE_MS) {
            std::cmp::Ordering::Greater => current.saturating_sub(8).max(INTERIM_INTERVAL_BASE_MS),
            std::cmp::Ordering::Less => (current + 4).min(INTERIM_INTERVAL_BASE_MS),
            std::cmp::Ordering::Equal => current,
        }
    }
}

// ---------- 最终转写 + 粘贴 ----------

pub async fn finalize_recording(app_handle: tauri::AppHandle, session: RecordingSession) {
    let session_id = session.session_id;
    let sample_rate = session.sample_rate;

    // 1. 等待录音线程结束（stop_flag 已在调用方设为 true）
    if let Some(handle) = session.audio_thread {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = handle.join();
        })
        .await;
    }

    // 2. 等待中间转写任务自然结束（stop_flag 已为 true，循环会在当前转写完成后退出）
    //    不能 abort：如果 interim 正持有 funasr_process 锁与 Python 通信，
    //    强杀会导致 stdin/stdout 协议错乱，后续 transcribe 必定失败。
    if let Some(task) = session.interim_task {
        let _ = task.await;
    }

    // 3. 通知前端：处理中
    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": false,
            "isProcessing": true,
        }),
    );

    // 4. 获取采样数据
    let final_samples = match session.samples.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            log::warn!("采样缓冲区锁已污染，继续使用恢复后的数据");
            poisoned.into_inner().clone()
        }
    };

    let resampled = resample_to_16k(&final_samples, sample_rate);
    let duration_sec = resampled.len() as f64 / TARGET_SAMPLE_RATE as f64;

    if duration_sec < MIN_AUDIO_DURATION_SEC {
        log::info!("录音时间过短 ({:.2}s)，跳过转写", duration_sec);
        emit_done(&app_handle, session_id, "", EMPTY_RESULT_HIDE_DELAY_MS);
        // 即使本次录音太短，也要把之前缓冲的待粘贴文本粘出去
        flush_pending_paste(&app_handle);
        return;
    }

    let wav_bytes = encode_wav(&resampled, TARGET_SAMPLE_RATE);
    let state = app_handle.state::<AppState>();

    // 5. 执行最终转写
    match funasr_service::transcribe(state.inner(), wav_bytes, &app_handle).await {
        Ok(result) if result.success => {
            let text = result.text.trim().to_string();
            let hide_delay = if text.is_empty() {
                EMPTY_RESULT_HIDE_DELAY_MS
            } else {
                RESULT_HIDE_DELAY_MS
            };
            emit_done(&app_handle, session_id, &text, hide_delay);

            if !text.is_empty() {
                let app_for_paste = app_handle.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
                    do_paste(&app_for_paste, &text).await;
                });
            } else {
                flush_pending_paste(&app_handle);
            }
        }
        Ok(result) => {
            let msg = result.error.unwrap_or_else(|| "语音识别失败".into());
            emit_error(&app_handle, session_id, &msg);
            flush_pending_paste(&app_handle);
        }
        Err(e) => {
            emit_error(&app_handle, session_id, &format!("语音识别失败: {}", e));
            flush_pending_paste(&app_handle);
        }
    }
}

fn emit_done(app_handle: &tauri::AppHandle, session_id: u64, text: &str, hide_delay_ms: u64) {
    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": false,
            "isProcessing": false,
        }),
    );
    let _ = app_handle.emit(
        "transcription-result",
        serde_json::json!({
            "sessionId": session_id,
            "text": text,
            "interim": false,
        }),
    );

    schedule_hide(app_handle, hide_delay_ms);
}

fn emit_error(app_handle: &tauri::AppHandle, session_id: u64, error: &str) {
    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": false,
            "isProcessing": false,
            "error": error,
        }),
    );

    schedule_hide(app_handle, EMPTY_RESULT_HIDE_DELAY_MS);
}

fn is_recording_active(state: &AppState) -> bool {
    match state.recording.lock() {
        Ok(guard) => guard.is_some(),
        Err(poisoned) => {
            log::warn!("录音状态锁已污染，继续使用恢复后的状态");
            poisoned.into_inner().is_some()
        }
    }
}

fn drain_pending_paste(state: &AppState) -> Vec<String> {
    match state.pending_paste.lock() {
        Ok(mut pending) => pending.drain(..).collect(),
        Err(poisoned) => {
            log::warn!("待粘贴队列锁已污染，继续使用恢复后的数据");
            let mut pending = poisoned.into_inner();
            pending.drain(..).collect()
        }
    }
}

fn push_pending_paste(state: &AppState, text: &str) {
    match state.pending_paste.lock() {
        Ok(mut pending) => pending.push(text.to_string()),
        Err(poisoned) => {
            log::warn!("待粘贴队列锁已污染，继续使用恢复后的数据");
            let mut pending = poisoned.into_inner();
            pending.push(text.to_string());
        }
    }
}

fn read_input_method(state: &AppState) -> String {
    match state.input_method.lock() {
        Ok(method) => method.clone(),
        Err(poisoned) => {
            log::warn!("输入方式锁已污染，回退到恢复后的值");
            poisoned.into_inner().clone()
        }
    }
}

/// 延迟隐藏字幕窗口。
/// 记录调度时的"显示代"，醒来后若代已变（说明中间有新的 show）则跳过隐藏；
/// 同时仍保留"正在录音则跳过"的兜底检查。
fn schedule_hide(app_handle: &tauri::AppHandle, delay_ms: u64) {
    let app = app_handle.clone();
    let state = app.state::<AppState>();
    let gen_at_schedule = state
        .subtitle_show_gen
        .load(std::sync::atomic::Ordering::Relaxed);

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        let state = app.state::<AppState>();

        // 若期间字幕窗口被重新 show 过，本次 hide 已过时
        let gen_now = state
            .subtitle_show_gen
            .load(std::sync::atomic::Ordering::Relaxed);
        if gen_now != gen_at_schedule {
            return;
        }

        // 兜底：正在录音时不隐藏
        if is_recording_active(state.inner()) {
            return;
        }

        let _ = crate::commands::window::hide_subtitle_window_inner(&app);
    });
}

/// 将待粘贴队列中的文本粘出去（用于本次录音结果为空或失败的情况）。
fn flush_pending_paste(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let texts = drain_pending_paste(state.inner());

    if texts.is_empty() {
        return;
    }

    let combined: String = texts.into_iter().collect();
    let app = app_handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
        do_paste(&app, &combined).await;
    });
}

async fn do_paste(app_handle: &tauri::AppHandle, text: &str) {
    let state = app_handle.state::<AppState>();

    // 检查是否有新录音正在进行
    let is_recording = is_recording_active(state.inner());

    if is_recording {
        // 新录音已开始，将文本暂存到待粘贴队列，等下次录音结束后一并粘贴
        push_pending_paste(state.inner(), text);
        log::info!("录音进行中，文本已加入待粘贴队列（{} 个字符）", text.len());
        return;
    }

    // 先取出待粘贴队列中的文本（来自之前被中断的粘贴）
    let mut full_text = String::new();
    for t in drain_pending_paste(state.inner()) {
        full_text.push_str(&t);
    }
    full_text.push_str(text);

    let method = read_input_method(state.inner());

    if let Err(e) =
        crate::commands::clipboard::paste_text_impl(app_handle, &full_text, &method).await
    {
        log::error!("自动粘贴失败: {}", e);
    }
}

// ---------- 麦克风测试 ----------

pub fn test_microphone_sync() -> Result<String, AppError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| AppError::Other("未找到可用的音频输入设备".into()))?;

    let device_name = device.name().unwrap_or_else(|_| "未知设备".into());

    let config = device
        .default_input_config()
        .map_err(|e| AppError::Other(format!("获取默认音频配置失败: {}", e)))?;

    let received = Arc::new(AtomicBool::new(false));
    let sample_format = config.sample_format();

    let stream = {
        let flag = received.clone();
        match sample_format {
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |_: &[i16], _: &cpal::InputCallbackInfo| {
                    flag.store(true, Ordering::Relaxed);
                },
                |err| log::warn!("麦克风测试流错误: {}", err),
                None,
            ),
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |_: &[f32], _: &cpal::InputCallbackInfo| {
                    flag.store(true, Ordering::Relaxed);
                },
                |err| log::warn!("麦克风测试流错误: {}", err),
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |_: &[u16], _: &cpal::InputCallbackInfo| {
                    flag.store(true, Ordering::Relaxed);
                },
                |err| log::warn!("麦克风测试流错误: {}", err),
                None,
            ),
            other => {
                return Err(AppError::Other(format!(
                    "麦克风测试不支持的采样格式: {:?}",
                    other
                )));
            }
        }
    }
    .map_err(|e| AppError::Other(format!("创建音频流失败: {}", e)))?;

    stream
        .play()
        .map_err(|e| AppError::Other(format!("启动音频流失败: {}", e)))?;

    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(stream);

    if received.load(Ordering::Relaxed) {
        Ok(format!("麦克风正常 ({})", device_name))
    } else {
        Ok(format!("麦克风已连接但未检测到音频数据 ({})", device_name))
    }
}
