use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use serde::Serialize;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::utils::MutexRecover;
use super::user_profile::{LlmProviderConfig, UserProfile};

#[derive(Clone)]
pub struct InterimCache {
    pub text: String,
    pub sample_count: usize,
    pub language: Option<String>,
}

pub struct RecordingSession {
    pub session_id: u64,
    pub stop_flag: Arc<AtomicBool>,
    pub stop_notify: Arc<tokio::sync::Notify>,
    pub samples: Arc<std::sync::Mutex<Vec<i16>>>,
    pub sample_rate: u32,
    pub audio_thread: Option<JoinHandle<()>>,
    pub interim_task: Option<tokio::task::JoinHandle<()>>,
    pub interim_cache: Arc<std::sync::Mutex<Option<InterimCache>>>,
}

#[derive(Clone)]
pub struct PendingRecordingSession {
    pub session_id: u64,
    pub stop_flag: Arc<AtomicBool>,
    pub stop_notify: Arc<tokio::sync::Notify>,
}

pub enum RecordingSlot {
    Starting(PendingRecordingSession),
    Active(RecordingSession),
}

impl RecordingSlot {
    pub fn session_id(&self) -> u64 {
        match self {
            Self::Starting(s) => s.session_id,
            Self::Active(s) => s.session_id,
        }
    }
}

pub struct MicrophoneLevelMonitor {
    pub stop_flag: Arc<AtomicBool>,
    pub handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyDiagnosticState {
    pub shortcut: String,
    pub registered: bool,
    pub backend: String,
    pub is_pressed: bool,
    pub last_error: Option<String>,
    pub warning: Option<String>,
    pub last_event: Option<String>,
    pub last_event_at_ms: Option<u64>,
    pub last_registered_at_ms: Option<u64>,
    pub last_pressed_at_ms: Option<u64>,
    pub last_released_at_ms: Option<u64>,
}

impl Default for HotkeyDiagnosticState {
    fn default() -> Self {
        Self {
            shortcut: String::new(),
            registered: false,
            backend: "none".into(),
            is_pressed: false,
            last_error: None,
            warning: None,
            last_event: None,
            last_event_at_ms: None,
            last_registered_at_ms: None,
            last_pressed_at_ms: None,
            last_released_at_ms: None,
        }
    }
}

pub struct AppState {
    pub funasr_process: Arc<Mutex<Option<FunasrProcess>>>,
    pub funasr_ready: Arc<AtomicBool>,
    pub funasr_starting: Arc<AtomicBool>,
    pub download_task: Arc<Mutex<Option<DownloadTask>>>,
    pub recording: Arc<std::sync::Mutex<Option<RecordingSlot>>>,
    pub session_counter: AtomicU64,
    pub input_method: Arc<std::sync::Mutex<String>>,
    pub pending_paste: Arc<std::sync::Mutex<Vec<String>>>,
    pub subtitle_show_gen: AtomicU64,
    pub selected_input_device_name: Arc<std::sync::Mutex<Option<String>>>,
    pub microphone_level_monitor: Arc<std::sync::Mutex<Option<MicrophoneLevelMonitor>>>,
    pub sound_enabled: Arc<AtomicBool>,
    pub ai_polish_enabled: Arc<AtomicBool>,
    pub ai_polish_api_key: Arc<std::sync::Mutex<String>>,
    pub http_client: reqwest::Client,
    pub user_profile: Arc<std::sync::Mutex<UserProfile>>,
    pub hotkey_diagnostic: Arc<std::sync::Mutex<HotkeyDiagnosticState>>,
    /// 编辑模式：按下热键时抓取的选中文本，finalize 时消费
    pub edit_context: Arc<std::sync::Mutex<Option<String>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            funasr_process: Default::default(),
            funasr_ready: Default::default(),
            funasr_starting: Default::default(),
            download_task: Default::default(),
            recording: Default::default(),
            session_counter: AtomicU64::new(0),
            input_method: Arc::new(std::sync::Mutex::new("sendInput".into())),
            pending_paste: Default::default(),
            subtitle_show_gen: AtomicU64::new(0),
            selected_input_device_name: Default::default(),
            microphone_level_monitor: Default::default(),
            sound_enabled: Arc::new(AtomicBool::new(true)),
            ai_polish_enabled: Default::default(),
            ai_polish_api_key: Default::default(),
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
            user_profile: Default::default(),
            hotkey_diagnostic: Default::default(),
            edit_context: Default::default(),
        }
    }
}

pub struct DownloadTask {
    pub cancel: oneshot::Sender<()>,
}

pub struct FunasrProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
}

impl Drop for FunasrProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_funasr_ready(&self) -> bool {
        self.funasr_ready.load(Ordering::Acquire)
    }

    pub fn set_funasr_ready(&self, ready: bool) {
        self.funasr_ready.store(ready, Ordering::Release);
    }

    pub fn snapshot_profile(&self) -> UserProfile {
        self.user_profile.lock_or_recover().clone()
    }

    pub fn update_profile<R>(&self, f: impl FnOnce(&mut UserProfile) -> R) -> (R, UserProfile) {
        let mut guard = self.user_profile.lock_or_recover();
        let result = f(&mut guard);
        (result, guard.clone())
    }

    pub fn active_llm_provider(&self) -> String {
        self.snapshot_profile().llm_provider.active
    }

    pub fn llm_provider_config(&self) -> LlmProviderConfig {
        self.snapshot_profile().llm_provider
    }

    pub fn read_ai_polish_api_key(&self) -> String {
        self.ai_polish_api_key.lock_or_recover().clone()
    }

    pub fn set_ai_polish_api_key(&self, api_key: impl Into<String>) {
        *self.ai_polish_api_key.lock_or_recover() = api_key.into();
    }

    pub fn selected_input_device_name(&self) -> Option<String> {
        self.selected_input_device_name.lock_or_recover().clone()
    }

    pub fn set_selected_input_device_name(&self, name: Option<String>) {
        *self.selected_input_device_name.lock_or_recover() = name.and_then(|v| {
            let trimmed = v.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
    }

    pub fn hotkey_diagnostic_snapshot(&self) -> HotkeyDiagnosticState {
        self.hotkey_diagnostic.lock_or_recover().clone()
    }

    pub fn update_hotkey_diagnostic<R>(&self, f: impl FnOnce(&mut HotkeyDiagnosticState) -> R) -> (R, HotkeyDiagnosticState) {
        let mut guard = self.hotkey_diagnostic.lock_or_recover();
        let result = f(&mut guard);
        (result, guard.clone())
    }
}
