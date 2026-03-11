use serde::Serialize;
use std::borrow::Cow;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use tauri::{Emitter, Manager};

use crate::services::{ai_polish_service, assistant_service, funasr_service, glm_asr_service};
use crate::state::{
    AppState, MicrophoneLevelMonitor, RecordingMode, RecordingSession, RecordingSlot,
};
use crate::utils::paths;
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
const INTERIM_MAX_AUDIO_WINDOW_SEC: f64 = 12.0;

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
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    {
        let mut writer =
            hound::WavWriter::new(&mut cursor, spec).expect("WAV writer creation failed");
        for &s in samples {
            writer.write_sample(s).expect("WAV sample write failed");
        }
        writer.finalize().expect("WAV finalize failed");
    }
    cursor.into_inner()
}

// ---------- 采样格式转换 ----------

fn f32_to_i16(s: f32) -> i16 {
    let c = s.clamp(-1.0, 1.0);
    if c < 0.0 {
        (c * 32768.0) as i16
    } else {
        (c * 32767.0) as i16
    }
}

fn u16_to_i16(s: u16) -> i16 {
    (s as i32 - 32768) as i16
}

// ---------- 重采样（rubato sinc 插值） ----------

fn resample_to_16k(input: &[i16], input_rate: u32) -> Cow<'_, [i16]> {
    if input.is_empty() || input_rate == 0 || input_rate == TARGET_SAMPLE_RATE {
        return Cow::Borrowed(input);
    }

    use rubato::{FastFixedIn, PolynomialDegree, Resampler};

    let ratio = TARGET_SAMPLE_RATE as f64 / input_rate as f64;
    let chunk_size = input.len();

    let mut resampler =
        match FastFixedIn::<f32>::new(ratio, 1.1, PolynomialDegree::Cubic, chunk_size, 1) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("rubato 初始化失败，跳过重采样: {}", e);
                return Cow::Borrowed(input);
            }
        };

    let input_f32: Vec<f32> = input.iter().map(|&s| s as f32 / 32768.0).collect();

    match resampler.process(&[&input_f32], None) {
        Ok(output) => {
            let resampled: Vec<i16> = output[0].iter().map(|&s| f32_to_i16(s)).collect();
            Cow::Owned(resampled)
        }
        Err(e) => {
            log::warn!("rubato 重采样失败，跳过: {}", e);
            Cow::Borrowed(input)
        }
    }
}

// ---------- cpal 设备管理 ----------

fn resolve_input_device(preferred_name: Option<&str>) -> Result<(cpal::Device, String), AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();

    if let Some(name) = preferred_name.filter(|n| !n.trim().is_empty()) {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                let dn = device.name().unwrap_or_default();
                if dn == name {
                    return Ok((device, dn));
                }
            }
        }
        log::warn!("指定麦克风不可用，回退到默认设备: {}", name);
    }

    let device = host
        .default_input_device()
        .ok_or_else(|| AppError::Audio("未找到可用的音频输入设备".into()))?;
    let name = device.name().unwrap_or_else(|_| "未知设备".into());
    Ok((device, name))
}

fn load_best_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, AppError> {
    use cpal::traits::DeviceTrait;
    use cpal::SampleFormat::{F32, I16, U16};

    let configs: Vec<_> = device
        .supported_input_configs()
        .map_err(|e| AppError::Audio(format!("查询音频设备配置失败: {}", e)))?
        .collect();

    if configs.is_empty() {
        return Err(AppError::Audio("音频设备不支持任何输入配置".into()));
    }

    let supports_16k = |c: &&cpal::SupportedStreamConfigRange| {
        c.min_sample_rate().0 <= TARGET_SAMPLE_RATE && c.max_sample_rate().0 >= TARGET_SAMPLE_RATE
    };
    let fmt = |f| move |c: &&cpal::SupportedStreamConfigRange| c.sample_format() == f;

    let pick = configs
        .iter()
        .find(|c| fmt(I16)(c) && supports_16k(c))
        .or_else(|| configs.iter().find(|c| fmt(F32)(c) && supports_16k(c)))
        .or_else(|| configs.iter().find(|c| fmt(U16)(c) && supports_16k(c)))
        .map(|c| c.with_sample_rate(cpal::SampleRate(TARGET_SAMPLE_RATE)))
        .or_else(|| {
            configs
                .iter()
                .find(|c| fmt(I16)(c))
                .or_else(|| configs.iter().find(|c| fmt(F32)(c)))
                .or_else(|| configs.iter().find(|c| fmt(U16)(c)))
                .or(configs.first())
                .map(|c| c.with_max_sample_rate())
        });

    pick.ok_or_else(|| AppError::Audio("无法找到合适的音频输入配置".into()))
}

pub fn list_input_devices_sync(
    selected_device_name: Option<String>,
) -> Result<InputDeviceListPayload, AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut devices: Vec<InputDeviceInfo> = host
        .input_devices()
        .map_err(|e| AppError::Audio(format!("枚举音频输入设备失败: {}", e)))?
        .filter_map(|d| {
            d.name().ok().map(|name| InputDeviceInfo {
                is_default: default_name.as_deref() == Some(&name),
                name,
            })
        })
        .collect();

    devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(InputDeviceListPayload {
        devices,
        selected_device_name,
    })
}

// ---------- 统一的多格式音频流构建宏 ----------

/// 为三种采样格式（I16/F32/U16）构建 cpal 输入流，消除重复代码。
macro_rules! build_input_stream_dispatch {
    ($device:expr, $config:expr, $sample_format:expr, $err_cb:expr, $callback_i16:expr, $callback_f32:expr, $callback_u16:expr) => {
        match $sample_format {
            cpal::SampleFormat::I16 => {
                use cpal::traits::DeviceTrait;
                $device.build_input_stream(&$config.into(), $callback_i16, $err_cb, None)
            }
            cpal::SampleFormat::F32 => {
                use cpal::traits::DeviceTrait;
                $device.build_input_stream(&$config.into(), $callback_f32, $err_cb, None)
            }
            cpal::SampleFormat::U16 => {
                use cpal::traits::DeviceTrait;
                $device.build_input_stream(&$config.into(), $callback_u16, $err_cb, None)
            }
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
    };
}

/// 将多声道数据混音到单声道 i16
fn mix_to_mono_i16(data: &[i16], channels: usize, out: &mut Vec<i16>) {
    if channels <= 1 {
        out.extend_from_slice(data);
    } else {
        out.extend(data.chunks_exact(channels).map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        }));
    }
}

fn mix_to_mono_f32(data: &[f32], channels: usize, out: &mut Vec<i16>) {
    if channels <= 1 {
        out.extend(data.iter().map(|&s| f32_to_i16(s)));
    } else {
        out.extend(
            data.chunks_exact(channels)
                .map(|frame| f32_to_i16(frame.iter().sum::<f32>() / channels as f32)),
        );
    }
}

fn mix_to_mono_u16(data: &[u16], channels: usize, out: &mut Vec<i16>) {
    if channels <= 1 {
        out.extend(data.iter().map(|&s| u16_to_i16(s)));
    } else {
        out.extend(data.chunks_exact(channels).map(|frame| {
            let sum: u64 = frame.iter().map(|&s| s as u64).sum();
            u16_to_i16((sum / channels as u64) as u16)
        }));
    }
}

// ---------- peak 计算 ----------

fn mono_peak_i16(data: &[i16], ch: usize) -> u32 {
    if ch <= 1 {
        data.iter()
            .map(|s| s.unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(ch)
            .map(|f| {
                let s: i32 = f.iter().map(|&v| v as i32).sum();
                (s / ch as i32).unsigned_abs()
            })
            .max()
            .unwrap_or(0)
    }
}
fn mono_peak_f32(data: &[f32], ch: usize) -> u32 {
    if ch <= 1 {
        data.iter()
            .map(|&s| f32_to_i16(s).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(ch)
            .map(|f| f32_to_i16(f.iter().sum::<f32>() / ch as f32).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    }
}
fn mono_peak_u16(data: &[u16], ch: usize) -> u32 {
    if ch <= 1 {
        data.iter()
            .map(|&s| u16_to_i16(s).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(ch)
            .map(|f| {
                let s: u64 = f.iter().map(|&v| v as u64).sum();
                u16_to_i16((s / ch as u64) as u16).unsigned_abs() as u32
            })
            .max()
            .unwrap_or(0)
    }
}

fn peak_to_meter(peak: u32) -> u32 {
    ((peak.min(32767) as f32 / 32767.0) * 1000.0).round() as u32
}

// ---------- 音频捕获线程 ----------

pub fn spawn_audio_capture_thread(
    stop_flag: Arc<AtomicBool>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    selected_device_name: Option<String>,
) -> Result<(std::thread::JoinHandle<()>, u32), AppError> {
    let (rate_tx, rate_rx) = std::sync::mpsc::sync_channel::<Result<u32, String>>(1);
    let stop = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            use cpal::traits::StreamTrait;

            let (device, device_name) = match resolve_input_device(selected_device_name.as_deref())
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = rate_tx.send(Err(e.to_string()));
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

            let err_cb = |e: cpal::StreamError| log::error!("音频流错误: {}", e);
            let stop_cb = stop.clone();

            let mk_i16 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    mix_to_mono_i16(data, channels, &mut buf.lock());
                }
            };
            let mk_f32 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    mix_to_mono_f32(data, channels, &mut buf.lock());
                }
            };
            let mk_u16 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    mix_to_mono_u16(data, channels, &mut buf.lock());
                }
            };

            let stream = match build_input_stream_dispatch!(
                device,
                config,
                sample_format,
                err_cb,
                mk_i16,
                mk_f32,
                mk_u16
            ) {
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
            let _ = rate_tx.send(Ok(sample_rate));

            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            drop(stream);
            log::info!("音频捕获已停止");
        })
        .map_err(|e| AppError::Audio(format!("创建录音线程失败: {}", e)))?;

    let sample_rate = match rate_rx.recv_timeout(std::time::Duration::from_secs(
        AUDIO_CAPTURE_INIT_TIMEOUT_SECS,
    )) {
        Ok(r) => r.map_err(AppError::Audio)?,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop_flag.store(true, Ordering::Relaxed);
            return Err(AppError::Audio(format!(
                "录音线程启动超时（{} 秒）",
                AUDIO_CAPTURE_INIT_TIMEOUT_SECS
            )));
        }
        Err(_) => return Err(AppError::Audio("录音线程启动后未返回结果".into())),
    };

    Ok((handle, sample_rate))
}

// ---------- 中间转写循环 ----------

pub fn spawn_interim_loop(
    app_handle: tauri::AppHandle,
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    stop_notify: Arc<tokio::sync::Notify>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    sample_rate: u32,
    interim_cache: Arc<parking_lot::Mutex<Option<crate::state::InterimCache>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        let mut interval_ms = INTERIM_INTERVAL_BASE_MS;
        let mut last_sample_count: usize = 0;
        // 增量快照缓冲区：只从共享 samples 中拷贝新增部分，追加到此处
        let mut snapshot: Vec<i16> = Vec::new();

        if sample_rate == 0 {
            log::error!("中间转写启动失败：采样率为 0 (session {})", session_id);
            return;
        }

        // 在线引擎不做中间转写（每次请求花钱且有网络延迟）
        if paths::is_online_engine(&paths::read_engine_config()) {
            log::info!("在线引擎，跳过中间转写 (session {})", session_id);
            stop_notify.notified().await;
            return;
        }

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(interval_ms)) => {}
                _ = stop_notify.notified() => { break; }
            }
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let current_count = {
                let guard = samples.lock();
                let count = guard.len();
                if count.saturating_sub(last_sample_count) < MIN_SAMPLES_GROWTH {
                    interval_ms = adjust_interval(interval_ms, false, 0);
                    continue;
                }
                if (count as f64 / sample_rate as f64) < MIN_AUDIO_DURATION_SEC {
                    continue;
                }
                // 只拷贝新增的样本，锁持有时间最短
                snapshot.extend_from_slice(&guard[snapshot.len()..]);
                count
            };

            let start = std::time::Instant::now();
            let resampled = resample_to_16k(&snapshot, sample_rate);
            let interim_max_samples =
                (TARGET_SAMPLE_RATE as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize;
            let interim_samples = if resampled.len() > interim_max_samples {
                &resampled[resampled.len() - interim_max_samples..]
            } else {
                resampled.as_ref()
            };
            let covered_sample_count =
                current_count.min((sample_rate as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize);

            match funasr_service::transcribe_pcm16(
                state.inner(),
                interim_samples,
                TARGET_SAMPLE_RATE,
                &app_handle,
            )
            .await
            {
                Ok(result) if result.success && !result.text.is_empty() => {
                    let _ = app_handle.emit(
                        "transcription-result",
                        serde_json::json!({
                            "sessionId": session_id,
                            "text": &result.text,
                            "interim": true,
                            "language": &result.language,
                        }),
                    );
                    *interim_cache.lock() = Some(crate::state::InterimCache {
                        text: result.text,
                        language: result.language,
                        sample_count: covered_sample_count,
                    });
                    last_sample_count = current_count;
                }
                _ => {}
            }

            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            interval_ms = adjust_interval(interval_ms, true, start.elapsed().as_millis() as u64);
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
        mode,
        sample_rate,
        audio_thread,
        interim_task,
        samples,
        interim_cache,
        ..
    } = session;

    if let Some(h) = audio_thread {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = h.join();
        })
        .await;
    }
    // 等待 interim 任务自然结束；超时则 abort 以释放 funasr_process 锁
    if let Some(t) = interim_task {
        let abort_handle = t.abort_handle();
        if tokio::time::timeout(std::time::Duration::from_secs(5), t)
            .await
            .is_err()
        {
            log::warn!("interim 任务超时 (5s)，强制中止");
            abort_handle.abort();
        }
    }

    let final_count = samples.lock().len();
    let cached = interim_cache.lock().clone();
    let duration_sec = final_count as f64 / sample_rate as f64;

    if duration_sec < MIN_AUDIO_DURATION_SEC {
        log::info!("录音时间过短 ({:.2}s)，跳过转写", duration_sec);
        app_handle.state::<AppState>().edit_context.lock().take();
        emit_done(
            &app_handle,
            session_id,
            mode,
            "",
            "",
            duration_sec,
            false,
            None,
        );
        flush_pending_paste(&app_handle);
        return;
    }

    let state = app_handle.state::<AppState>();

    // 优先复用 interim 缓存（覆盖率 >=90%），否则重新 ASR
    let (asr_text, detected_lang): (Result<String, String>, Option<String>) = match cached {
        Some(ref c)
            if final_count > 0
                && (c.sample_count as f64 / final_count as f64) >= 0.90
                && !c.text.trim().is_empty() =>
        {
            log::info!(
                "复用 interim 缓存 (覆盖率 {:.0}%)",
                c.sample_count as f64 / final_count as f64 * 100.0
            );
            (Ok(c.text.clone()), c.language.clone())
        }
        _ => match do_final_asr(&app_handle, state.inner(), &samples, sample_rate).await {
            Ok(r) => (Ok(r.text), r.language),
            Err(e) => (Err(e), None),
        },
    };

    let text = match asr_text {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            state.edit_context.lock().take();
            emit_error(&app_handle, session_id, mode, &e);
            flush_pending_paste(&app_handle);
            return;
        }
    };

    let lang_ref = detected_lang.as_deref();

    if text.is_empty() {
        state.edit_context.lock().take();
        emit_done(
            &app_handle,
            session_id,
            mode,
            "",
            "",
            duration_sec,
            false,
            lang_ref,
        );
        flush_pending_paste(&app_handle);
        return;
    }

    // 检查是否处于编辑模式（热键按下时抓取了选中文本）
    let edit_context = state.edit_context.lock().take();

    if mode == RecordingMode::Dictation && edit_context.is_some() {
        let selected_text = edit_context.unwrap_or_default();
        // 编辑模式：ASR 结果是语音指令，用它改写选中文本
        log::info!(
            "编辑模式：指令=\"{}\"，选中文本长度={}",
            text,
            selected_text.len()
        );
        match ai_polish_service::edit_text(
            state.inner(),
            &selected_text,
            &text,
            &app_handle,
            session_id,
        )
        .await
        {
            Ok(result) => {
                emit_done(
                    &app_handle,
                    session_id,
                    mode,
                    &result,
                    &result,
                    duration_sec,
                    true,
                    lang_ref,
                );
                if !result.is_empty() {
                    let app = app_handle.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
                        do_paste(&app, &result).await;
                    });
                } else {
                    flush_pending_paste(&app_handle);
                }
            }
            Err(e) => {
                log::warn!("编辑选中文本失败，不替换原文: {}", e);
                let _ = app_handle.emit(
                    "recording-error",
                    serde_json::json!({ "message": format!("编辑失败: {}", e) }),
                );
                emit_done(
                    &app_handle,
                    session_id,
                    mode,
                    "",
                    &selected_text,
                    duration_sec,
                    false,
                    lang_ref,
                );
                flush_pending_paste(&app_handle);
            }
        }
    } else if mode == RecordingMode::Assistant {
        match assistant_service::generate_content(
            state.inner(),
            &text,
            edit_context.as_deref(),
            &app_handle,
            session_id,
        )
        .await
        {
            Ok(result) => {
                emit_done(
                    &app_handle,
                    session_id,
                    mode,
                    &result,
                    &text,
                    duration_sec,
                    false,
                    lang_ref,
                );
                if let Err(err) =
                    crate::commands::window::set_subtitle_window_interactive(&app_handle, true)
                {
                    log::warn!("助手结果显示时切换字幕窗口交互态失败: {}", err);
                }
            }
            Err(err) => {
                emit_error(&app_handle, session_id, mode, &err.to_string());
                flush_pending_paste(&app_handle);
            }
        }
    } else {
        // 普通听写模式
        let original = text.clone();
        let text = ai_polish_service::polish_text(state.inner(), &text, &app_handle, session_id)
            .await
            .unwrap_or_else(|e| {
                log::warn!("AI 润色失败，使用原文: {}", e);
                text
            });
        let polished = text != original;
        emit_done(
            &app_handle,
            session_id,
            mode,
            &text,
            &original,
            duration_sec,
            polished,
            lang_ref,
        );

        if !text.is_empty() {
            let app = app_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
                do_paste(&app, &text).await;
            });
        } else {
            flush_pending_paste(&app_handle);
        }
    }
}

pub async fn discard_recording(session: RecordingSession) {
    if let Some(h) = session.audio_thread {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = h.join();
        })
        .await;
    }
    if let Some(t) = session.interim_task {
        let abort_handle = t.abort_handle();
        if tokio::time::timeout(std::time::Duration::from_secs(5), t)
            .await
            .is_err()
        {
            abort_handle.abort();
        }
    }
    log::info!("已丢弃录音会话 (session {})", session.session_id);
}

async fn do_final_asr(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    samples: &parking_lot::Mutex<Vec<i16>>,
    sample_rate: u32,
) -> Result<funasr_service::TranscriptionResult, String> {
    let data = samples.lock().clone();
    let resampled = resample_to_16k(&data, sample_rate);

    let engine = paths::read_engine_config();
    let result = if paths::is_online_engine(&engine) {
        let wav = encode_wav(&resampled, TARGET_SAMPLE_RATE);
        glm_asr_service::transcribe(state, wav).await
    } else {
        funasr_service::transcribe_pcm16(state, &resampled, TARGET_SAMPLE_RATE, app_handle).await
    };

    match result {
        Ok(r) if r.success => Ok(r),
        Ok(r) => Err(r.error.unwrap_or_else(|| "语音识别失败".into())),
        Err(e) => Err(format!("语音识别失败: {}", e)),
    }
}

// ---------- 事件发送 ----------

fn emit_done(
    app: &tauri::AppHandle,
    sid: u64,
    mode: RecordingMode,
    text: &str,
    original_text: &str,
    dur: f64,
    polished: bool,
    language: Option<&str>,
) {
    let delay = if text.is_empty() {
        EMPTY_RESULT_HIDE_DELAY_MS
    } else {
        RESULT_HIDE_DELAY_MS
    };
    emit_recording_state_if_current(app, sid, mode, false, false, None);
    let _ = app.emit(
        "transcription-result",
        serde_json::json!({
            "sessionId": sid, "text": text, "interim": false,
            "durationSec": dur, "charCount": text.chars().count(), "polished": polished,
            "language": language, "mode": mode.as_str(), "originalText": original_text,
        }),
    );
    if mode != RecordingMode::Assistant || text.is_empty() {
        schedule_hide(app, delay);
    }
}

fn emit_error(app: &tauri::AppHandle, sid: u64, mode: RecordingMode, error: &str) {
    emit_recording_state_if_current(app, sid, mode, false, false, Some(error));
    schedule_hide(app, EMPTY_RESULT_HIDE_DELAY_MS);
}

fn emit_recording_state_if_current(
    app: &tauri::AppHandle,
    sid: u64,
    mode: RecordingMode,
    recording: bool,
    processing: bool,
    error: Option<&str>,
) {
    let state = app.state::<AppState>();
    if let Some(active) = state
        .recording
        .lock()
        .as_ref()
        .map(RecordingSlot::session_id)
    {
        if active != sid {
            log::info!("跳过过期会话状态广播 (session {}, active {})", sid, active);
            return;
        }
    }
    let mut payload = serde_json::json!({
        "sessionId": sid, "isRecording": recording, "isProcessing": processing,
        "mode": mode.as_str(),
    });
    if let Some(err) = error {
        payload["error"] = serde_json::json!(err);
    }
    let _ = app.emit("recording-state", payload);
}

// ---------- 粘贴逻辑 ----------

fn schedule_hide(app: &tauri::AppHandle, delay_ms: u64) {
    let app = app.clone();
    let gen = app
        .state::<AppState>()
        .subtitle_show_gen
        .load(Ordering::Relaxed);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        let state = app.state::<AppState>();
        if state.subtitle_show_gen.load(Ordering::Relaxed) != gen {
            return;
        }
        if state.recording.lock().is_some() {
            return;
        }
        let _ = crate::commands::window::hide_subtitle_window_inner(&app);
    });
}

fn flush_pending_paste(app: &tauri::AppHandle) {
    let texts: Vec<String> = app
        .state::<AppState>()
        .pending_paste
        .lock()
        .drain(..)
        .collect();
    if texts.is_empty() {
        return;
    }
    let combined: String = texts.into_iter().collect();
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
        do_paste(&app, &combined).await;
    });
}

async fn do_paste(app: &tauri::AppHandle, text: &str) {
    let state = app.state::<AppState>();
    if state.recording.lock().is_some() {
        state.pending_paste.lock().push(text.to_string());
        log::info!("录音进行中，文本已加入待粘贴队列（{} 个字符）", text.len());
        return;
    }

    let mut full = String::new();
    for t in state.pending_paste.lock().drain(..) {
        full.push_str(&t);
    }
    full.push_str(text);

    let method = state.input_method.lock().clone();
    if let Err(e) = crate::commands::clipboard::paste_text_impl(app, &full, &method).await {
        log::error!("自动粘贴失败: {}", e);
    }
}

// ---------- 麦克风测试 / 预览 ----------

pub fn stop_microphone_level_monitor(state: &AppState) {
    if let Some(mut m) = state.microphone_level_monitor.lock().take() {
        m.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = m.handle.take() {
            let _ = h.join();
        }
    }
}

pub fn start_microphone_level_monitor(
    app_handle: tauri::AppHandle,
    state: &AppState,
) -> Result<String, AppError> {
    use cpal::traits::StreamTrait;

    stop_microphone_level_monitor(state);

    let (device, device_name) =
        resolve_input_device(state.selected_input_device_name().as_deref())?;
    let config = load_best_input_config(&device)?;
    let fmt = config.sample_format();
    let ch = config.channels() as usize;

    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("mic-level-monitor".into())
        .spawn({
            let app = app_handle.clone();
            let dn = device_name.clone();
            move || {
                let peak = Arc::new(AtomicU32::new(0));
                let err_cb = |e: cpal::StreamError| log::warn!("麦克风预览流错误: {}", e);

                let mk_i16 = {
                    let p = peak.clone();
                    move |d: &[i16], _: &cpal::InputCallbackInfo| {
                        p.fetch_max(peak_to_meter(mono_peak_i16(d, ch)), Ordering::AcqRel);
                    }
                };
                let mk_f32 = {
                    let p = peak.clone();
                    move |d: &[f32], _: &cpal::InputCallbackInfo| {
                        p.fetch_max(peak_to_meter(mono_peak_f32(d, ch)), Ordering::AcqRel);
                    }
                };
                let mk_u16 = {
                    let p = peak.clone();
                    move |d: &[u16], _: &cpal::InputCallbackInfo| {
                        p.fetch_max(peak_to_meter(mono_peak_u16(d, ch)), Ordering::AcqRel);
                    }
                };

                let stream = match build_input_stream_dispatch!(
                    device, config, fmt, err_cb, mk_i16, mk_f32, mk_u16
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("创建麦克风预览流失败: {}", e)));
                        return;
                    }
                };
                if let Err(e) = stream.play() {
                    let _ = ready_tx.send(Err(format!("启动麦克风预览流失败: {}", e)));
                    return;
                }
                let _ = ready_tx.send(Ok(()));

                while !stop.load(Ordering::Relaxed) {
                    let meter = peak.swap(0, Ordering::AcqRel) as f32 / 1000.0;
                    let _ = app.emit(
                        "microphone-level",
                        serde_json::json!({ "deviceName": dn, "level": meter.clamp(0.0, 1.0) }),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(
                        MICROPHONE_LEVEL_EMIT_INTERVAL_MS,
                    ));
                }
                let _ = app.emit(
                    "microphone-level",
                    serde_json::json!({ "deviceName": dn, "level": 0.0 }),
                );
                drop(stream);
            }
        })
        .map_err(|e| AppError::Audio(format!("创建麦克风预览线程失败: {}", e)))?;

    match ready_rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = handle.join();
            return Err(AppError::Audio(e));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = handle.join();
            return Err(AppError::Audio("麦克风预览启动超时".into()));
        }
        Err(_) => return Err(AppError::Audio("麦克风预览线程未返回结果".into())),
    }

    *state.microphone_level_monitor.lock() = Some(MicrophoneLevelMonitor {
        stop_flag,
        handle: Some(handle),
    });
    Ok(device_name)
}

pub fn test_microphone_sync(selected_device_name: Option<String>) -> Result<String, AppError> {
    use cpal::traits::StreamTrait;

    let (device, device_name) = resolve_input_device(selected_device_name.as_deref())?;
    let config = load_best_input_config(&device)?;
    let received = Arc::new(AtomicBool::new(false));
    let fmt = config.sample_format();

    let err_cb = |e: cpal::StreamError| log::warn!("麦克风测试流错误: {}", e);

    let r1 = received.clone();
    let r2 = received.clone();
    let r3 = received.clone();
    let stream = build_input_stream_dispatch!(
        device,
        config,
        fmt,
        err_cb,
        move |_: &[i16], _: &cpal::InputCallbackInfo| {
            r1.store(true, Ordering::Relaxed);
        },
        move |_: &[f32], _: &cpal::InputCallbackInfo| {
            r2.store(true, Ordering::Relaxed);
        },
        move |_: &[u16], _: &cpal::InputCallbackInfo| {
            r3.store(true, Ordering::Relaxed);
        }
    )
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
