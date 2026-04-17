use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::Emitter;

use super::resample::{f32_to_i16, u16_to_i16};
use super::{AUDIO_CAPTURE_INIT_TIMEOUT_SECS, TARGET_SAMPLE_RATE};
use crate::services::audio_service::{InputDeviceInfo, InputDeviceListPayload};
use crate::utils::AppError;

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

// ---------- 多声道混音到单声道 i16 ----------

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
