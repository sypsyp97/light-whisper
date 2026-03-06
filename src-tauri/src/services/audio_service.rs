use std::borrow::Cow;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};
use serde::Serialize;

use tauri::{Emitter, Manager};

use crate::services::{ai_polish_service, funasr_service};
use crate::state::{AppState, MicrophoneLevelMonitor, RecordingSession, RecordingSlot};
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
const MICROPHONE_LEVEL_EMIT_INTERVAL_MS: u64 = 70;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceListPayload {
    pub devices: Vec<InputDeviceInfo>,
    pub selected_device_name: Option<String>,
}

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

fn resolve_input_device(
    preferred_name: Option<&str>,
) -> Result<(cpal::Device, String), AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();

    if let Some(name) = preferred_name.filter(|name| !name.trim().is_empty()) {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                let device_name = device.name().unwrap_or_else(|_| "未知设备".into());
                if device_name == name {
                    return Ok((device, device_name));
                }
            }
        }
        log::warn!("指定麦克风不可用，回退到默认设备: {}", name);
    }

    let device = host
        .default_input_device()
        .ok_or_else(|| AppError::Audio("未找到可用的音频输入设备".into()))?;
    let device_name = device.name().unwrap_or_else(|_| "未知设备".into());
    Ok((device, device_name))
}

fn load_best_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, AppError> {
    use cpal::traits::DeviceTrait;

    let supported: Vec<_> = device
        .supported_input_configs()
        .map_err(|e| AppError::Audio(format!("查询音频设备配置失败: {}", e)))?
        .collect();

    if supported.is_empty() {
        return Err(AppError::Audio("音频设备不支持任何输入配置".into()));
    }

    find_best_config(&supported)
}

pub fn list_input_devices_sync(
    selected_device_name: Option<String>,
) -> Result<InputDeviceListPayload, AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());

    let mut devices = Vec::new();
    for device in host
        .input_devices()
        .map_err(|e| AppError::Audio(format!("枚举音频输入设备失败: {}", e)))?
    {
        let name = device.name().unwrap_or_else(|_| "未知设备".into());
        let is_default = default_name.as_deref() == Some(name.as_str());
        devices.push(InputDeviceInfo { name, is_default });
    }

    devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(InputDeviceListPayload {
        devices,
        selected_device_name,
    })
}

fn mono_peak_i16(data: &[i16], channels: usize) -> u32 {
    if channels <= 1 {
        data.iter()
            .map(|sample| sample.unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(channels)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|&sample| sample as i32).sum();
                (sum / channels as i32).unsigned_abs()
            })
            .max()
            .unwrap_or(0)
    }
}

fn mono_peak_f32(data: &[f32], channels: usize) -> u32 {
    if channels <= 1 {
        data.iter()
            .map(|&sample| f32_to_i16(sample).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(channels)
            .map(|frame| f32_to_i16(frame.iter().sum::<f32>() / channels as f32).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    }
}

fn mono_peak_u16(data: &[u16], channels: usize) -> u32 {
    if channels <= 1 {
        data.iter()
            .map(|&sample| u16_to_i16(sample).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(channels)
            .map(|frame| {
                let sum: u64 = frame.iter().map(|&sample| sample as u64).sum();
                u16_to_i16((sum / channels as u64) as u16).unsigned_abs() as u32
            })
            .max()
            .unwrap_or(0)
    }
}

fn peak_to_meter_value(peak: u32) -> u32 {
    ((peak.min(32767) as f32 / 32767.0) * 1000.0).round() as u32
}

/// 启动音频捕获线程。全部设备逻辑在线程内完成，sample_rate 通过 channel 回传。
pub fn spawn_audio_capture_thread(
    stop_flag: Arc<AtomicBool>,
    samples: Arc<std::sync::Mutex<Vec<i16>>>,
    selected_device_name: Option<String>,
) -> Result<(std::thread::JoinHandle<()>, u32), AppError> {
    let (rate_tx, rate_rx) = std::sync::mpsc::sync_channel::<Result<u32, String>>(1);
    let stop_for_thread = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            use cpal::traits::{DeviceTrait, StreamTrait};

            let (device, device_name) = match resolve_input_device(selected_device_name.as_deref()) {
                Ok(result) => result,
                Err(err) => {
                    let _ = rate_tx.send(Err(err.to_string()));
                    return;
                }
            };

            log::info!("使用音频输入设备: {}", device_name);

            let config = match load_best_input_config(&device) {
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
        .map_err(|e| AppError::Audio(format!("创建录音线程失败: {}", e)))?;

    // 等待线程初始化完成，拿到采样率或错误
    let sample_rate = match rate_rx.recv_timeout(std::time::Duration::from_secs(
        AUDIO_CAPTURE_INIT_TIMEOUT_SECS,
    )) {
        Ok(result) => result.map_err(AppError::Audio)?,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop_flag.store(true, Ordering::Relaxed);
            return Err(AppError::Audio(format!(
                "录音线程启动超时（{} 秒）",
                AUDIO_CAPTURE_INIT_TIMEOUT_SECS
            )));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return Err(AppError::Audio("录音线程启动后未返回结果".into()));
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

    pick.ok_or_else(|| AppError::Audio("无法找到合适的音频输入配置".into()))
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
    stop_notify: Arc<tokio::sync::Notify>,
    samples: Arc<std::sync::Mutex<Vec<i16>>>,
    sample_rate: u32,
    interim_cache: Arc<std::sync::Mutex<Option<crate::state::InterimCache>>>,
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
            // 用 select! 让 stop_notify 能立即打断 sleep，避免白等几百毫秒
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(interval_ms)) => {}
                _ = stop_notify.notified() => { break; }
            }

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
                            "text": &result.text,
                            "interim": true,
                        }),
                    );
                    // 缓存 interim 结果，finalize 时可直接复用
                    if let Ok(mut cache) = interim_cache.lock() {
                        *cache = Some(crate::state::InterimCache {
                            text: result.text,
                            sample_count: current_count,
                        });
                    }
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
    let RecordingSession {
        session_id,
        sample_rate,
        audio_thread,
        interim_task,
        samples,
        interim_cache,
        ..
    } = session;

    // 1. 等待录音线程结束（stop_flag 已在调用方设为 true）
    if let Some(handle) = audio_thread {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = handle.join();
        })
        .await;
    }

    // 2. 等待中间转写任务自然结束（stop_notify 已打断 sleep，通常很快退出）
    //    不能 abort：如果 interim 正持有 funasr_process 锁与 Python 通信，
    //    强杀会导致 stdin/stdout 协议错乱，后续 transcribe 必定失败。
    if let Some(task) = interim_task {
        let _ = task.await;
    }

    // 3. 获取采样数据与 interim 缓存
    let final_sample_count = match samples.lock() {
        Ok(guard) => guard.len(),
        Err(poisoned) => poisoned.into_inner().len(),
    };

    let cached = match interim_cache.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    let duration_sec = final_sample_count as f64 / sample_rate as f64;

    if duration_sec < MIN_AUDIO_DURATION_SEC {
        log::info!("录音时间过短 ({:.2}s)，跳过转写", duration_sec);
        emit_done(&app_handle, session_id, "", duration_sec, false);
        flush_pending_paste(&app_handle);
        return;
    }

    let state = app_handle.state::<AppState>();

    // 4. 获取转写文本：优先复用 interim 缓存，仅在缺失或覆盖不足时重新 ASR
    //    interim 每轮都转写完整音频，只要覆盖了 >=90% 的采样即可直接使用
    let asr_text = if let Some(ref cache) = cached {
        let coverage = if final_sample_count > 0 {
            cache.sample_count as f64 / final_sample_count as f64
        } else {
            0.0
        };
        if coverage >= 0.90 && !cache.text.trim().is_empty() {
            log::info!(
                "复用 interim 缓存 (覆盖率 {:.0}%，省去最终 ASR)",
                coverage * 100.0
            );
            Ok(cache.text.clone())
        } else {
            do_final_asr(&app_handle, state.inner(), &samples, sample_rate).await
        }
    } else {
        do_final_asr(&app_handle, state.inner(), &samples, sample_rate).await
    };

    let text = match asr_text {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            emit_error(&app_handle, session_id, &e);
            flush_pending_paste(&app_handle);
            return;
        }
    };

    if text.is_empty() {
        emit_done(&app_handle, session_id, "", duration_sec, false);
        flush_pending_paste(&app_handle);
        return;
    }

    // 5. AI 润色：失败时 fallback 返回原文
    let original = text.clone();
    let text = ai_polish_service::polish_text(state.inner(), &text, &app_handle)
        .await
        .unwrap_or_else(|e| {
            log::warn!("AI 润色失败，使用原文: {}", e);
            text
        });
    let polished = text != original;
    emit_done(&app_handle, session_id, &text, duration_sec, polished);

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

pub async fn discard_recording(session: RecordingSession) {
    let RecordingSession {
        session_id,
        audio_thread,
        interim_task,
        ..
    } = session;

    if let Some(handle) = audio_thread {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = handle.join();
        })
        .await;
    }

    if let Some(task) = interim_task {
        let _ = task.await;
    }

    log::info!("已丢弃录音会话 (session {})", session_id);
}

/// 执行最终 ASR 转写（仅在 interim 缓存不可用时调用）
async fn do_final_asr(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    samples: &std::sync::Mutex<Vec<i16>>,
    sample_rate: u32,
) -> Result<String, String> {
    let final_samples = match samples.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            log::warn!("采样缓冲区锁已污染，继续使用恢复后的数据");
            poisoned.into_inner().clone()
        }
    };

    let resampled = resample_to_16k(&final_samples, sample_rate);
    let wav_bytes = encode_wav(&resampled, TARGET_SAMPLE_RATE);

    match funasr_service::transcribe(state, wav_bytes, app_handle).await {
        Ok(result) if result.success => Ok(result.text),
        Ok(result) => Err(result.error.unwrap_or_else(|| "语音识别失败".into())),
        Err(e) => Err(format!("语音识别失败: {}", e)),
    }
}

fn emit_done(
    app_handle: &tauri::AppHandle,
    session_id: u64,
    text: &str,
    duration_sec: f64,
    polished: bool,
) {
    let hide_delay_ms = if text.is_empty() {
        EMPTY_RESULT_HIDE_DELAY_MS
    } else {
        RESULT_HIDE_DELAY_MS
    };
    emit_recording_state_if_current(app_handle, session_id, false, false, None);
    let char_count = text.chars().count();
    let _ = app_handle.emit(
        "transcription-result",
        serde_json::json!({
            "sessionId": session_id,
            "text": text,
            "interim": false,
            "durationSec": duration_sec,
            "charCount": char_count,
            "polished": polished,
        }),
    );

    schedule_hide(app_handle, hide_delay_ms);
}

fn emit_error(app_handle: &tauri::AppHandle, session_id: u64, error: &str) {
    emit_recording_state_if_current(app_handle, session_id, false, false, Some(error));

    schedule_hide(app_handle, EMPTY_RESULT_HIDE_DELAY_MS);
}

fn active_recording_session_id(state: &AppState) -> Option<u64> {
    match state.recording.lock() {
        Ok(guard) => guard.as_ref().map(RecordingSlot::session_id),
        Err(poisoned) => {
            log::warn!("录音状态锁已污染，继续使用恢复后的状态");
            poisoned
                .into_inner()
                .as_ref()
                .map(RecordingSlot::session_id)
        }
    }
}

fn emit_recording_state_if_current(
    app_handle: &tauri::AppHandle,
    session_id: u64,
    is_recording: bool,
    is_processing: bool,
    error: Option<&str>,
) {
    let state = app_handle.state::<AppState>();
    if let Some(active_session_id) = active_recording_session_id(state.inner()) {
        if active_session_id != session_id {
            log::info!(
                "跳过过期会话状态广播 (session {}, active {})",
                session_id,
                active_session_id
            );
            return;
        }
    }

    let payload = match error {
        Some(err) => serde_json::json!({
            "sessionId": session_id,
            "isRecording": is_recording,
            "isProcessing": is_processing,
            "error": err,
        }),
        None => serde_json::json!({
            "sessionId": session_id,
            "isRecording": is_recording,
            "isProcessing": is_processing,
        }),
    };
    let _ = app_handle.emit("recording-state", payload);
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

// ---------- 麦克风测试 / 预览 ----------

pub fn stop_microphone_level_monitor(state: &AppState) {
    let monitor = match state.microphone_level_monitor.lock() {
        Ok(mut guard) => guard.take(),
        Err(poisoned) => {
            log::warn!("麦克风预览锁已污染，继续使用恢复后的状态");
            poisoned.into_inner().take()
        }
    };

    if let Some(mut monitor) = monitor {
        monitor.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = monitor.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn start_microphone_level_monitor(
    app_handle: tauri::AppHandle,
    state: &AppState,
) -> Result<String, AppError> {
    use cpal::traits::{DeviceTrait, StreamTrait};

    stop_microphone_level_monitor(state);

    let selected_device_name = state.selected_input_device_name();
    let (device, device_name) = resolve_input_device(selected_device_name.as_deref())?;
    let config = load_best_input_config(&device)?;
    let sample_format = config.sample_format();
    let channels = config.channels() as usize;

    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_for_thread = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("microphone-level-monitor".into())
        .spawn({
            let app_handle = app_handle.clone();
            let device_name = device_name.clone();
            move || {
                let peak_meter = Arc::new(AtomicU32::new(0));

                let err_callback = |err: cpal::StreamError| {
                    log::warn!("麦克风预览流错误: {}", err);
                };

                let stream = match sample_format {
                    cpal::SampleFormat::I16 => {
                        let peak_meter = peak_meter.clone();
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                let peak = peak_to_meter_value(mono_peak_i16(data, channels));
                                peak_meter.fetch_max(peak, Ordering::AcqRel);
                            },
                            err_callback,
                            None,
                        )
                    }
                    cpal::SampleFormat::F32 => {
                        let peak_meter = peak_meter.clone();
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                let peak = peak_to_meter_value(mono_peak_f32(data, channels));
                                peak_meter.fetch_max(peak, Ordering::AcqRel);
                            },
                            err_callback,
                            None,
                        )
                    }
                    cpal::SampleFormat::U16 => {
                        let peak_meter = peak_meter.clone();
                        device.build_input_stream(
                            &config.into(),
                            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                                let peak = peak_to_meter_value(mono_peak_u16(data, channels));
                                peak_meter.fetch_max(peak, Ordering::AcqRel);
                            },
                            err_callback,
                            None,
                        )
                    }
                    other => {
                        let _ = ready_tx.send(Err(format!("麦克风预览不支持的采样格式: {:?}", other)));
                        return;
                    }
                };

                let stream = match stream {
                    Ok(stream) => stream,
                    Err(err) => {
                        let _ = ready_tx.send(Err(format!("创建麦克风预览流失败: {}", err)));
                        return;
                    }
                };

                if let Err(err) = stream.play() {
                    let _ = ready_tx.send(Err(format!("启动麦克风预览流失败: {}", err)));
                    return;
                }

                let _ = ready_tx.send(Ok(()));

                while !stop_for_thread.load(Ordering::Relaxed) {
                    let meter = peak_meter.swap(0, Ordering::AcqRel) as f32 / 1000.0;
                    let _ = app_handle.emit(
                        "microphone-level",
                        serde_json::json!({
                            "deviceName": device_name,
                            "level": meter.clamp(0.0, 1.0),
                        }),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(
                        MICROPHONE_LEVEL_EMIT_INTERVAL_MS,
                    ));
                }

                let _ = app_handle.emit(
                    "microphone-level",
                    serde_json::json!({
                        "deviceName": device_name,
                        "level": 0.0,
                    }),
                );
                drop(stream);
            }
        })
        .map_err(|e| AppError::Audio(format!("创建麦克风预览线程失败: {}", e)))?;

    match ready_rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = handle.join();
            return Err(AppError::Audio(err));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = handle.join();
            return Err(AppError::Audio("麦克风预览启动超时".into()));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return Err(AppError::Audio("麦克风预览线程未返回结果".into()));
        }
    }

    match state.microphone_level_monitor.lock() {
        Ok(mut guard) => {
            *guard = Some(MicrophoneLevelMonitor {
                stop_flag,
                handle: Some(handle),
            });
        }
        Err(poisoned) => {
            log::warn!("麦克风预览锁已污染，继续使用恢复后的状态");
            *poisoned.into_inner() = Some(MicrophoneLevelMonitor {
                stop_flag,
                handle: Some(handle),
            });
        }
    }

    Ok(device_name)
}

pub fn test_microphone_sync(selected_device_name: Option<String>) -> Result<String, AppError> {
    use cpal::traits::{DeviceTrait, StreamTrait};

    let (device, device_name) = resolve_input_device(selected_device_name.as_deref())?;
    let config = load_best_input_config(&device)?;
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
                return Err(AppError::Audio(format!(
                    "麦克风测试不支持的采样格式: {:?}",
                    other
                )));
            }
        }
    }
    .map_err(|e| AppError::Audio(format!("创建音频流失败: {}", e)))?;

    stream
        .play()
        .map_err(|e| AppError::Audio(format!("启动音频流失败: {}", e)))?;

    std::thread::sleep(std::time::Duration::from_millis(220));
    drop(stream);

    if received.load(Ordering::Relaxed) {
        Ok(format!("麦克风正常 ({})", device_name))
    } else {
        Ok(format!("麦克风已连接但未检测到音频数据 ({})", device_name))
    }
}
