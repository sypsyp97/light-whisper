use serde::Serialize;

pub(crate) const TARGET_SAMPLE_RATE: u32 = 16000;

pub(crate) const MIN_AUDIO_DURATION_SEC: f64 = 0.5;

pub(crate) const MIN_INTERIM_DURATION_SEC: f64 = 0.2;
pub(crate) const MIN_SAMPLES_GROWTH: usize = 1024;

pub(crate) const INTERIM_INTERVAL_MIN_MS: u64 = 140;
pub(crate) const INTERIM_INTERVAL_BASE_MS: u64 = 220;
pub(crate) const INTERIM_INTERVAL_MAX_MS: u64 = 460;
pub(crate) const INTERIM_INTERVAL_DOWN_STEP_MS: u64 = 24;
pub(crate) const INTERIM_INTERVAL_UP_STEP_MS: u64 = 42;
pub(crate) const INTERIM_HEAVY_COST_MS: u64 = 420;
pub(crate) const INTERIM_LIGHT_COST_MS: u64 = 180;
pub(crate) const INTERIM_MAX_AUDIO_WINDOW_SEC: f64 = 12.0;

pub(crate) const RESULT_HIDE_DELAY_MS: u64 = 2500;
pub(crate) const EMPTY_RESULT_HIDE_DELAY_MS: u64 = 360;

pub(crate) const PASTE_DELAY_MS: u64 = 60;
pub(crate) const AUDIO_CAPTURE_INIT_TIMEOUT_SECS: u64 = 8;
pub(crate) const MICROPHONE_LEVEL_EMIT_INTERVAL_MS: u64 = 70;

pub(crate) const EDIT_GRAB_WAIT_MS: u64 = 650;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceListPayload {
    pub devices: Vec<InputDeviceInfo>,
    pub selected_device_name: Option<String>,
}

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

mod capture;
mod capture_bounds;
#[cfg(test)]
mod capture_bounds_tests;
#[cfg(test)]
mod capture_mixer_bounds_tests;
mod finalize;
#[cfg(test)]
mod history_output_tests;
mod interim;
mod monitor;
mod native_capture;
#[cfg(test)]
mod native_capture_tests;
#[cfg(test)]
mod native_integration_tests;
mod native_recording;
pub(crate) mod output;
#[cfg(test)]
mod output_tests;
mod resample;
#[cfg(test)]
mod shared_native_route_tests;
mod wav;
#[cfg(target_os = "windows")]
mod windows_capture;

pub use capture::{list_input_devices_sync, spawn_audio_capture_thread, spawn_waveform_emitter};
pub(crate) use finalize::flush_pending_paste;
pub use finalize::{discard_recording, finalize_recording};
pub use interim::spawn_interim_loop;
pub use monitor::{
    start_microphone_level_monitor, stop_microphone_level_monitor, test_microphone_sync,
};
pub(crate) use native_recording::spawn_native_recording;
pub use native_recording::NativeRecording;
pub use wav::encode_wav;

pub(crate) fn uses_native_stream(engine: &str) -> bool {
    matches!(engine, "qwen3-asr-0.6b" | "confucius4-r2t2")
}
