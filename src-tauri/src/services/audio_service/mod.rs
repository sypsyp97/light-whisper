use serde::Serialize;

// ---------- 常量 ----------

pub(crate) const TARGET_SAMPLE_RATE: u32 = 16000;
/// finalize_recording 的下限：低于这个时长整段录音直接跳过（视为误按）
pub(crate) const MIN_AUDIO_DURATION_SEC: f64 = 0.5;
/// interim 的下限：首个 tick 积到这个时长就开始送 Python 推理，不再等到 0.5s。
/// 短于 0.5s 的部分会在 funasr_service::transcribe_pcm16 里尾部补零对齐 Python VAD。
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
/// ASR 结果出来后到实际粘贴之间的固定延迟。**本质是 UX 节奏，不是焦点防护**。
///
/// stop→paste 之间发生的事：
///   1. `emit_done` 发 "transcription-result" 事件
///   2. 字幕窗口 React 侧更新 DOM（纯 DOM 变化，不触发 OS 级窗口激活）
///   3. `do_paste` 运行，`GetForegroundWindow()` 读用户原目标 app（一直没变过）
///
/// 字幕窗口在 start_recording_inner 阶段就由后台 task 调 `show_subtitle_window`
/// 创建好，且建窗参数是 `.focused(false) + skip_taskbar + ignore_cursor_events`。
/// 其中 `force_window_topmost` 用 `SWP_NOACTIVATE` 明确告诉 Windows 不激活窗口。
/// 也就是说从 start 到 stop 这几秒里，目标 app 的前台状态从未被动过。
///
/// 所以这个 sleep 真正做的是：让用户在按键事件进入目标 app 之前，有短暂一瞬能在
/// 字幕上看到识别结果——视觉确认 → 动作确认的节奏。曾经是 260ms（v1.2.3 之前），
/// 降到 120ms（v1.2.3），再降到 60ms。60ms 低于大多数人的"注意到字幕更新"阈值，
/// 基本等于立即粘贴，适合追求速度的用户。
///
/// 如果用户反馈 "结果出现和粘贴同时发生感觉太突然"，可以往回调到 120-150ms。
/// 如果以后真的出现 "粘到字幕窗口而不是目标 app" 或按键顺序错乱，说明焦点理论被
/// 翻案了，需要调回 200+ ms 并重新审查 show_subtitle_window 里的窗口操作序列。
pub(crate) const PASTE_DELAY_MS: u64 = 60;
pub(crate) const AUDIO_CAPTURE_INIT_TIMEOUT_SECS: u64 = 8;
pub(crate) const MICROPHONE_LEVEL_EMIT_INTERVAL_MS: u64 = 70;
/// finalize 阶段等待并行抓取选中文本的最大时长。超时就按普通听写处理。
pub(crate) const EDIT_GRAB_WAIT_MS: u64 = 650;

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

// ---------- 子模块 ----------

mod capture;
mod finalize;
mod interim;
mod monitor;
mod resample;
mod wav;

// ---------- 外部 API 再导出 ----------
//
// 保持外部引用点零改动：
// `use crate::services::audio_service::X` 在拆分前后语义相同。

pub use capture::{list_input_devices_sync, spawn_audio_capture_thread, spawn_waveform_emitter};
pub use finalize::{discard_recording, finalize_recording};
pub use interim::spawn_interim_loop;
pub use monitor::{
    start_microphone_level_monitor, stop_microphone_level_monitor, test_microphone_sync,
};
pub use wav::encode_wav;
