use serde::Serialize;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use super::user_profile::{LlmProviderConfig, UserProfile};
use crate::services::codex_oauth_service::OpenaiCodexOauthSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    Dictation,
    Assistant,
}

impl RecordingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationOutputMode {
    Original,
    Translated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingTrigger {
    DictationOriginal,
    DictationTranslated,
    Assistant,
}

impl RecordingTrigger {
    pub fn mode(self) -> RecordingMode {
        match self {
            Self::Assistant => RecordingMode::Assistant,
            Self::DictationOriginal | Self::DictationTranslated => RecordingMode::Dictation,
        }
    }

    pub fn dictation_output(self) -> DictationOutputMode {
        match self {
            Self::DictationTranslated => DictationOutputMode::Translated,
            Self::DictationOriginal | Self::Assistant => DictationOutputMode::Original,
        }
    }
}

#[derive(Clone)]
pub struct InterimCache {
    pub text: String,
    pub sample_count: usize,
    pub language: Option<String>,
}

pub struct RecordingSession {
    pub session_id: u64,
    pub trigger: RecordingTrigger,
    pub stop_flag: Arc<AtomicBool>,
    pub stop_notify: Arc<tokio::sync::Notify>,
    pub samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    pub sample_rate: u32,
    pub audio_thread: Option<JoinHandle<()>>,
    pub interim_task: Option<tokio::task::JoinHandle<()>>,
    pub interim_cache: Arc<parking_lot::Mutex<Option<InterimCache>>>,
    /// 热键按下时并行抓取的选中文本任务。与会话同生同死，避免全局共享导致的
    /// 跨会话污染（finalize_N 读到 hotkey_{N+1} 的 grab）。
    pub edit_grab: Option<tokio::task::JoinHandle<Option<String>>>,
}

#[derive(Clone)]
pub struct PendingRecordingSession {
    pub session_id: u64,
    pub trigger: RecordingTrigger,
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

    pub fn trigger(&self) -> RecordingTrigger {
        match self {
            Self::Starting(s) => s.trigger,
            Self::Active(s) => s.trigger,
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
    /// Non-empty when another program has registered the same hotkey via RegisterHotKey
    pub system_conflict: Option<String>,
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
            system_conflict: None,
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
    pub recording: Arc<parking_lot::Mutex<Option<RecordingSlot>>>,
    pub session_counter: AtomicU64,
    pub input_method: Arc<parking_lot::Mutex<String>>,
    pub pending_paste: Arc<parking_lot::Mutex<Vec<String>>>,
    pub subtitle_show_gen: AtomicU64,
    pub selected_input_device_name: Arc<parking_lot::Mutex<Option<String>>>,
    pub microphone_level_monitor: Arc<parking_lot::Mutex<Option<MicrophoneLevelMonitor>>>,
    pub sound_enabled: Arc<AtomicBool>,
    pub ai_polish_enabled: Arc<AtomicBool>,
    pub ai_polish_api_key: Arc<parking_lot::Mutex<String>>,
    pub assistant_api_key: Arc<parking_lot::Mutex<String>>,
    pub openai_codex_oauth_session: Arc<parking_lot::Mutex<Option<OpenaiCodexOauthSession>>>,
    pub http_client: reqwest::Client,
    pub user_profile: Arc<parking_lot::Mutex<UserProfile>>,
    pub assistant_image_support_cache: Arc<parking_lot::Mutex<HashMap<String, bool>>>,
    pub hotkey_diagnostic: Arc<parking_lot::Mutex<HotkeyDiagnosticState>>,
    pub online_asr_api_key: Arc<parking_lot::Mutex<String>>,
    pub web_search_api_key: Arc<parking_lot::Mutex<String>>,
    /// 引擎生命周期代数，stop_server 递增，start_server 据此检测是否被取消
    pub funasr_generation: AtomicU64,
    /// 内存音频传输支持状态：0=未知, 1=支持, 2=不支持
    pub inline_audio_transport: AtomicU8,
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
            input_method: Arc::new(parking_lot::Mutex::new("sendInput".into())),
            pending_paste: Default::default(),
            subtitle_show_gen: AtomicU64::new(0),
            selected_input_device_name: Default::default(),
            microphone_level_monitor: Default::default(),
            sound_enabled: Arc::new(AtomicBool::new(true)),
            ai_polish_enabled: Default::default(),
            ai_polish_api_key: Default::default(),
            assistant_api_key: Default::default(),
            openai_codex_oauth_session: Default::default(),
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
            user_profile: Default::default(),
            assistant_image_support_cache: Default::default(),
            hotkey_diagnostic: Default::default(),
            online_asr_api_key: Default::default(),
            web_search_api_key: Default::default(),
            funasr_generation: AtomicU64::new(0),
            inline_audio_transport: AtomicU8::new(0),
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
        self.user_profile.lock().clone()
    }

    /// 借用 profile 执行只读操作，无需克隆
    pub fn with_profile<R>(&self, f: impl FnOnce(&UserProfile) -> R) -> R {
        f(&self.user_profile.lock())
    }

    /// 修改 profile 并返回克隆（用于需要持久化的场景）
    pub fn update_profile<R>(&self, f: impl FnOnce(&mut UserProfile) -> R) -> (R, UserProfile) {
        let mut guard = self.user_profile.lock();
        let result = f(&mut guard);
        (result, guard.clone())
    }

    /// 修改 profile，不返回克隆（无需持久化时使用）
    pub fn update_profile_mut<R>(&self, f: impl FnOnce(&mut UserProfile) -> R) -> R {
        f(&mut self.user_profile.lock())
    }

    pub fn active_llm_provider(&self) -> String {
        self.with_profile(|p| p.llm_provider.resolve_active_provider())
    }

    pub fn llm_provider_config(&self) -> LlmProviderConfig {
        self.with_profile(|p| p.llm_provider.clone())
    }

    pub fn read_ai_polish_api_key(&self) -> String {
        self.ai_polish_api_key.lock().clone()
    }

    pub fn set_ai_polish_api_key(&self, api_key: impl Into<String>) {
        *self.ai_polish_api_key.lock() = api_key.into();
    }

    pub fn read_assistant_api_key(&self) -> String {
        self.assistant_api_key.lock().clone()
    }

    pub fn set_assistant_api_key(&self, api_key: impl Into<String>) {
        *self.assistant_api_key.lock() = api_key.into();
    }

    pub fn read_openai_codex_oauth_session(&self) -> Option<OpenaiCodexOauthSession> {
        self.openai_codex_oauth_session.lock().clone()
    }

    pub fn set_openai_codex_oauth_session(
        &self,
        session: Option<OpenaiCodexOauthSession>,
    ) {
        *self.openai_codex_oauth_session.lock() = session;
    }

    pub fn read_online_asr_api_key(&self) -> String {
        self.online_asr_api_key.lock().clone()
    }

    pub fn set_online_asr_api_key(&self, api_key: impl Into<String>) {
        *self.online_asr_api_key.lock() = api_key.into();
    }

    pub fn read_web_search_api_key(&self) -> String {
        self.web_search_api_key.lock().clone()
    }

    pub fn set_web_search_api_key(&self, api_key: impl Into<String>) {
        *self.web_search_api_key.lock() = api_key.into();
    }

    pub fn inline_audio_transport(&self) -> Option<bool> {
        match self.inline_audio_transport.load(Ordering::Acquire) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    pub fn set_inline_audio_transport(&self, supported: Option<bool>) {
        let encoded = match supported {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        };
        self.inline_audio_transport
            .store(encoded, Ordering::Release);
    }

    pub fn assistant_image_support(&self, cache_key: &str) -> Option<bool> {
        self.assistant_image_support_cache
            .lock()
            .get(cache_key)
            .copied()
    }

    pub fn set_assistant_image_support(&self, cache_key: impl Into<String>, supported: bool) {
        self.assistant_image_support_cache
            .lock()
            .insert(cache_key.into(), supported);
    }

    pub fn selected_input_device_name(&self) -> Option<String> {
        self.selected_input_device_name.lock().clone()
    }

    pub fn set_selected_input_device_name(&self, name: Option<String>) {
        *self.selected_input_device_name.lock() = name.and_then(|v| {
            let trimmed = v.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
    }

    pub fn hotkey_diagnostic_snapshot(&self) -> HotkeyDiagnosticState {
        self.hotkey_diagnostic.lock().clone()
    }

    pub fn update_hotkey_diagnostic<R>(
        &self,
        f: impl FnOnce(&mut HotkeyDiagnosticState) -> R,
    ) -> (R, HotkeyDiagnosticState) {
        let mut guard = self.hotkey_diagnostic.lock();
        let result = f(&mut guard);
        (result, guard.clone())
    }
}
