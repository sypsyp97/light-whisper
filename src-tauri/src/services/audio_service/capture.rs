use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

use tauri::Emitter;

use super::resample::{f32_to_i16, u16_to_i16};
use super::{AUDIO_CAPTURE_INIT_TIMEOUT_SECS, TARGET_SAMPLE_RATE};
use crate::services::audio_service::{InputDeviceInfo, InputDeviceListPayload};
use crate::utils::AppError;

/// 录音缓冲硬上限（单位：i16 样本，单声道，post mix-down）。即使 stop 信号
/// 丢失或某条异常路径漏掉了停止逻辑，缓冲也不会无限增长。
///
/// 30min × 48kHz mono = 86_400_000 samples = 172.8MB。覆盖任何合理录音
/// 时长；超过 30 分钟应分段录制。这是兜底安全阀，正常路径不应该触到。
pub(crate) const MAX_RECORD_SAMPLES: usize = 30 * 60 * 48_000;
const AUDIO_CAPTURE_TIMEOUT_JOIN_MS: u64 = 500;

/// 一次性的"已触达录音缓冲硬上限"警告标志。仅在第一次撞上限时打日志。
static RECORD_CAP_WARNED: AtomicBool = AtomicBool::new(false);

fn confirm_audio_thread_exit(handle: std::thread::JoinHandle<()>, wait: std::time::Duration) {
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let joiner = std::thread::Builder::new()
        .name("audio-capture-timeout-join".into())
        .spawn(move || {
            let result = handle.join();
            if result.is_err() {
                log::warn!("录音线程启动超时后退出时发生 panic");
            }
            let _ = done_tx.send(());
        });

    match joiner {
        Ok(_) => {
            if done_rx.recv_timeout(wait).is_err() {
                log::warn!("录音线程启动超时后仍未完成退出，后台 join 将继续等待");
            }
        }
        Err(err) => {
            log::warn!("创建录音线程退出确认 helper 失败: {}", err);
        }
    }
}

// ---------- cpal 设备管理 ----------

pub(super) fn resolve_input_device(
    preferred_name: Option<&str>,
) -> Result<(cpal::Device, String), AppError> {
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

pub(super) fn load_best_input_config(
    device: &cpal::Device,
) -> Result<cpal::SupportedStreamConfig, AppError> {
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

// ---------- 多声道混音到单声道 i16（带硬上限） ----------

pub(crate) fn mix_to_mono_capped_i16(
    data: &[i16],
    channels: usize,
    out: &mut Vec<i16>,
    cap: usize,
) {
    let chans = channels.max(1);
    let allowed_mono_samples = cap.saturating_sub(out.len());
    if allowed_mono_samples == 0 {
        return;
    }
    // mono input: 1 input sample == 1 output sample
    // multi-channel: chans input samples == 1 output sample
    let frame_count = data.len() / chans;
    let take_frames = frame_count.min(allowed_mono_samples);
    if take_frames == 0 {
        return;
    }
    let limited = &data[..take_frames * chans];
    if chans <= 1 {
        out.extend_from_slice(limited);
    } else {
        out.extend(limited.chunks_exact(chans).map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / chans as i32) as i16
        }));
    }
}

pub(crate) fn mix_to_mono_capped_f32(
    data: &[f32],
    channels: usize,
    out: &mut Vec<i16>,
    cap: usize,
) {
    let chans = channels.max(1);
    let allowed_mono_samples = cap.saturating_sub(out.len());
    if allowed_mono_samples == 0 {
        return;
    }
    let frame_count = data.len() / chans;
    let take_frames = frame_count.min(allowed_mono_samples);
    if take_frames == 0 {
        return;
    }
    let limited = &data[..take_frames * chans];
    if chans <= 1 {
        out.extend(limited.iter().map(|&s| f32_to_i16(s)));
    } else {
        out.extend(
            limited
                .chunks_exact(chans)
                .map(|frame| f32_to_i16(frame.iter().sum::<f32>() / chans as f32)),
        );
    }
}

pub(crate) fn mix_to_mono_capped_u16(
    data: &[u16],
    channels: usize,
    out: &mut Vec<i16>,
    cap: usize,
) {
    let chans = channels.max(1);
    let allowed_mono_samples = cap.saturating_sub(out.len());
    if allowed_mono_samples == 0 {
        return;
    }
    let frame_count = data.len() / chans;
    let take_frames = frame_count.min(allowed_mono_samples);
    if take_frames == 0 {
        return;
    }
    let limited = &data[..take_frames * chans];
    if chans <= 1 {
        out.extend(limited.iter().map(|&s| u16_to_i16(s)));
    } else {
        out.extend(limited.chunks_exact(chans).map(|frame| {
            let sum: u64 = frame.iter().map(|&s| s as u64).sum();
            u16_to_i16((sum / chans as u64) as u16)
        }));
    }
}

// ---------- 录音波形可视化 ----------

const WAVEFORM_BAR_COUNT: usize = 9;
const WAVEFORM_EMIT_INTERVAL_MS: u64 = 55;
const WAVEFORM_WINDOW_SECS: f64 = 0.12;

fn compute_waveform_bars(samples: &[i16]) -> Vec<f32> {
    let mut bars = vec![0.0f32; WAVEFORM_BAR_COUNT];
    if samples.is_empty() {
        return bars;
    }
    let chunk = samples.len() / WAVEFORM_BAR_COUNT;
    if chunk == 0 {
        return bars;
    }
    for (i, bar) in bars.iter_mut().enumerate() {
        let start = i * chunk;
        let end = (start + chunk).min(samples.len());
        let rms = (samples[start..end]
            .iter()
            .map(|&s| (s as f32) * (s as f32))
            .sum::<f32>()
            / (end - start) as f32)
            .sqrt();
        *bar = (rms / 5000.0).min(1.0).sqrt();
    }
    bars
}

pub fn spawn_waveform_emitter(
    app_handle: tauri::AppHandle,
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    sample_rate: u32,
) {
    let window_size = (sample_rate as f64 * WAVEFORM_WINDOW_SECS) as usize;

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(WAVEFORM_EMIT_INTERVAL_MS)).await;
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            let bars = {
                let guard = samples.lock();
                let start = guard.len().saturating_sub(window_size);
                compute_waveform_bars(&guard[start..])
            };
            let _ = app_handle.emit(
                "waveform",
                serde_json::json!({ "sessionId": session_id, "bars": bars }),
            );
        }
    });
}

// ---------- 音频捕获线程 ----------

pub fn spawn_audio_capture_thread(
    stop_flag: Arc<AtomicBool>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    selected_device_name: Option<String>,
) -> Result<(std::thread::JoinHandle<()>, u32), AppError> {
    // 每个新录音会话重置警告 latch；否则进程级一次警告之后，后续会话即便
    // 再次撞上限也不会写日志，丢失诊断信息。
    RECORD_CAP_WARNED.store(false, Ordering::Relaxed);

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
                    let mut locked = buf.lock();
                    if locked.len() >= MAX_RECORD_SAMPLES
                        && !RECORD_CAP_WARNED.swap(true, Ordering::Relaxed)
                    {
                        log::warn!(
                            "录音缓冲触达硬上限 {} 个 i16 样本，后续输入将被丢弃",
                            MAX_RECORD_SAMPLES
                        );
                    }
                    mix_to_mono_capped_i16(data, channels, &mut locked, MAX_RECORD_SAMPLES);
                }
            };
            let mk_f32 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut locked = buf.lock();
                    if locked.len() >= MAX_RECORD_SAMPLES
                        && !RECORD_CAP_WARNED.swap(true, Ordering::Relaxed)
                    {
                        log::warn!(
                            "录音缓冲触达硬上限 {} 个 i16 样本，后续输入将被丢弃",
                            MAX_RECORD_SAMPLES
                        );
                    }
                    mix_to_mono_capped_f32(data, channels, &mut locked, MAX_RECORD_SAMPLES);
                }
            };
            let mk_u16 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut locked = buf.lock();
                    if locked.len() >= MAX_RECORD_SAMPLES
                        && !RECORD_CAP_WARNED.swap(true, Ordering::Relaxed)
                    {
                        log::warn!(
                            "录音缓冲触达硬上限 {} 个 i16 样本，后续输入将被丢弃",
                            MAX_RECORD_SAMPLES
                        );
                    }
                    mix_to_mono_capped_u16(data, channels, &mut locked, MAX_RECORD_SAMPLES);
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
            confirm_audio_thread_exit(
                handle,
                std::time::Duration::from_millis(AUDIO_CAPTURE_TIMEOUT_JOIN_MS),
            );
            return Err(AppError::Audio(format!(
                "录音线程启动超时（{} 秒）",
                AUDIO_CAPTURE_INIT_TIMEOUT_SECS
            )));
        }
        Err(_) => return Err(AppError::Audio("录音线程启动后未返回结果".into())),
    };

    Ok((handle, sample_rate))
}

#[cfg(test)]
mod cap_tests {
    //! Tests for the capped mix-to-mono helpers and the buffer hard cap.
    //!
    //! Contract:
    //!   - `mix_to_mono_capped_<T>(data, channels, out, cap)` mixes multi-channel
    //!     audio down to mono i16 (averaging across channels per frame),
    //!     same as the existing private `mix_to_mono_<T>` helpers, **but**
    //!     `out.len()` MUST NOT exceed `cap` after the call.
    //!   - If `out.len() >= cap` already, append zero.
    //!   - If a partial frame fits, append only the frames that fit.
    //!
    //!   - `MAX_RECORD_SAMPLES` is 30 minutes of 48 kHz mono and must be
    //!     large enough to cover at least an hour of 16 kHz mono.
    use super::{
        mix_to_mono_capped_f32, mix_to_mono_capped_i16, mix_to_mono_capped_u16, MAX_RECORD_SAMPLES,
    };

    const MIN_ONE_HOUR_16K_SAMPLES: usize = 60 * 60 * 16_000;
    const _: () = assert!(MAX_RECORD_SAMPLES >= MIN_ONE_HOUR_16K_SAMPLES);

    // ----- MAX_RECORD_SAMPLES constant -----------------------------------

    #[test]
    fn max_record_samples_constant_value() {
        assert_eq!(MAX_RECORD_SAMPLES, 30 * 60 * 48_000);
    }

    // ----- mix_to_mono_capped_i16 ----------------------------------------

    #[test]
    fn mix_to_mono_capped_i16_under_cap_appends_all() {
        let data = vec![1i16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_i16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn mix_to_mono_capped_i16_at_cap_appends_zero() {
        let data = vec![1i16; 50];
        let mut out: Vec<i16> = vec![0; 100];
        mix_to_mono_capped_i16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn mix_to_mono_capped_i16_partial_fit_truncates() {
        let data = vec![2i16; 50];
        let mut out: Vec<i16> = vec![0; 90];
        mix_to_mono_capped_i16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100, "should fill exactly up to cap, no overflow");
    }

    #[test]
    fn mix_to_mono_capped_i16_stereo_downmix() {
        // 10 stereo frames of (1, 2) -> 10 mono samples after mixing
        let data: Vec<i16> = (0..10).flat_map(|_| [1i16, 2i16]).collect();
        assert_eq!(data.len(), 20);
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_i16(&data, 2, &mut out, 100);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn mix_to_mono_capped_i16_stereo_partial_fit() {
        // 10 stereo frames -> at most 10 mono frames; only 5 should fit.
        let data: Vec<i16> = (0..10).flat_map(|_| [1i16, 2i16]).collect();
        let mut out: Vec<i16> = vec![0; 95];
        mix_to_mono_capped_i16(&data, 2, &mut out, 100);
        assert_eq!(
            out.len(),
            100,
            "5 frames fit into the remaining 5 slots, no overflow"
        );
    }

    #[test]
    fn mix_to_mono_capped_i16_zero_cap_appends_zero() {
        let data = vec![1i16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_i16(&data, 1, &mut out, 0);
        assert_eq!(out.len(), 0);

        let mut out2: Vec<i16> = vec![0; 30];
        mix_to_mono_capped_i16(&data, 1, &mut out2, 0);
        assert_eq!(out2.len(), 30, "cap=0 must never shrink existing buffer");
    }

    // ----- mix_to_mono_capped_f32 ----------------------------------------

    #[test]
    fn mix_to_mono_capped_f32_under_cap_appends_all() {
        let data = vec![0.5f32; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_f32(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn mix_to_mono_capped_f32_at_cap_appends_zero() {
        let data = vec![0.5f32; 50];
        let mut out: Vec<i16> = vec![0; 100];
        mix_to_mono_capped_f32(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn mix_to_mono_capped_f32_partial_fit_truncates() {
        let data = vec![-0.5f32; 50];
        let mut out: Vec<i16> = vec![0; 90];
        mix_to_mono_capped_f32(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100, "should fill exactly up to cap, no overflow");
    }

    #[test]
    fn mix_to_mono_capped_f32_stereo_downmix() {
        let data: Vec<f32> = (0..10).flat_map(|_| [0.5f32, -0.5f32]).collect();
        assert_eq!(data.len(), 20);
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_f32(&data, 2, &mut out, 100);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn mix_to_mono_capped_f32_stereo_partial_fit() {
        let data: Vec<f32> = (0..10).flat_map(|_| [0.5f32, -0.5f32]).collect();
        let mut out: Vec<i16> = vec![0; 95];
        mix_to_mono_capped_f32(&data, 2, &mut out, 100);
        assert_eq!(
            out.len(),
            100,
            "5 frames fit into the remaining 5 slots, no overflow"
        );
    }

    #[test]
    fn mix_to_mono_capped_f32_zero_cap_appends_zero() {
        let data = vec![0.5f32; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_f32(&data, 1, &mut out, 0);
        assert_eq!(out.len(), 0);

        let mut out2: Vec<i16> = vec![0; 30];
        mix_to_mono_capped_f32(&data, 1, &mut out2, 0);
        assert_eq!(out2.len(), 30, "cap=0 must never shrink existing buffer");
    }

    // ----- mix_to_mono_capped_u16 ----------------------------------------

    #[test]
    fn mix_to_mono_capped_u16_under_cap_appends_all() {
        let data = vec![32768u16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_u16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn mix_to_mono_capped_u16_at_cap_appends_zero() {
        let data = vec![40000u16; 50];
        let mut out: Vec<i16> = vec![0; 100];
        mix_to_mono_capped_u16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn mix_to_mono_capped_u16_partial_fit_truncates() {
        let data = vec![32768u16; 50];
        let mut out: Vec<i16> = vec![0; 90];
        mix_to_mono_capped_u16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100, "should fill exactly up to cap, no overflow");
    }

    #[test]
    fn mix_to_mono_capped_u16_stereo_downmix() {
        let data: Vec<u16> = (0..10).flat_map(|_| [32768u16, 40000u16]).collect();
        assert_eq!(data.len(), 20);
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_u16(&data, 2, &mut out, 100);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn mix_to_mono_capped_u16_stereo_partial_fit() {
        let data: Vec<u16> = (0..10).flat_map(|_| [32768u16, 40000u16]).collect();
        let mut out: Vec<i16> = vec![0; 95];
        mix_to_mono_capped_u16(&data, 2, &mut out, 100);
        assert_eq!(
            out.len(),
            100,
            "5 frames fit into the remaining 5 slots, no overflow"
        );
    }

    #[test]
    fn mix_to_mono_capped_u16_zero_cap_appends_zero() {
        let data = vec![32768u16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_u16(&data, 1, &mut out, 0);
        assert_eq!(out.len(), 0);

        let mut out2: Vec<i16> = vec![0; 30];
        mix_to_mono_capped_u16(&data, 1, &mut out2, 0);
        assert_eq!(out2.len(), 30, "cap=0 must never shrink existing buffer");
    }
}
