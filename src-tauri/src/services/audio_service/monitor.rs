use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use tauri::Emitter;

use super::capture::{load_best_input_config, resolve_input_device};
use super::resample::{f32_to_i16, u16_to_i16};
use super::MICROPHONE_LEVEL_EMIT_INTERVAL_MS;
use crate::state::{AppState, MicrophoneLevelMonitor};
use crate::utils::AppError;

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

// ---------- 麦克风测试 / 预览 ----------

pub fn stop_microphone_level_monitor(state: &AppState) {
    if let Some(mut m) = state.recording.microphone_level_monitor.lock().take() {
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

    *state.recording.microphone_level_monitor.lock() = Some(MicrophoneLevelMonitor {
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
