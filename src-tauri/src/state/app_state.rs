use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use super::user_profile::{LlmProviderConfig, UserProfile};

/// interim 循环缓存的最近一次转写结果
#[derive(Clone)]
pub struct InterimCache {
    /// 转写文本
    pub text: String,
    /// 转写时使用的采样数
    pub sample_count: usize,
}

pub struct RecordingSession {
    pub session_id: u64,
    pub stop_flag: Arc<AtomicBool>,
    /// 通知 interim 循环立即退出（配合 tokio::select! 打断 sleep）
    pub stop_notify: Arc<tokio::sync::Notify>,
    pub samples: Arc<std::sync::Mutex<Vec<i16>>>,
    pub sample_rate: u32,
    pub audio_thread: Option<std::thread::JoinHandle<()>>,
    pub interim_task: Option<tokio::task::JoinHandle<()>>,
    /// interim 循环缓存的最新转写结果，finalize 时可直接复用以跳过冗余 ASR
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
            Self::Starting(session) => session.session_id,
            Self::Active(session) => session.session_id,
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
    /// 待粘贴文本队列：当粘贴时机恰逢新录音已开始，文本会暂存于此，
    /// 等下次录音结束后一并粘贴，避免丢失。
    pub pending_paste: Arc<std::sync::Mutex<Vec<String>>>,
    /// 字幕窗口"显示代"计数器：每次 show 时递增。
    /// schedule_hide 会在睡眠前记录当前代，醒来后若代已变则跳过隐藏，
    /// 从而避免旧 hide 任务误杀新一轮字幕。
    pub subtitle_show_gen: AtomicU64,
    pub sound_enabled: Arc<AtomicBool>,
    pub ai_polish_enabled: Arc<AtomicBool>,
    pub ai_polish_api_key: Arc<std::sync::Mutex<String>>,
    pub http_client: reqwest::Client,
    pub user_profile: Arc<std::sync::Mutex<UserProfile>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            funasr_process: Arc::new(Mutex::new(None)),
            funasr_ready: Arc::new(AtomicBool::new(false)),
            funasr_starting: Arc::new(AtomicBool::new(false)),
            download_task: Arc::new(Mutex::new(None)),
            recording: Arc::new(std::sync::Mutex::new(None)),
            session_counter: AtomicU64::new(0),
            input_method: Arc::new(std::sync::Mutex::new("sendInput".to_string())),
            pending_paste: Arc::new(std::sync::Mutex::new(Vec::new())),
            subtitle_show_gen: AtomicU64::new(0),
            sound_enabled: Arc::new(AtomicBool::new(true)),
            ai_polish_enabled: Arc::new(AtomicBool::new(false)),
            ai_polish_api_key: Arc::new(std::sync::Mutex::new(String::new())),
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
            user_profile: Arc::new(std::sync::Mutex::new(UserProfile::default())),
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
        // 确保 Python 子进程在句柄丢弃时被终止，防止孤儿进程
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
        match self.user_profile.lock() {
            Ok(profile) => profile.clone(),
            Err(poisoned) => {
                log::warn!("用户画像锁已污染，继续使用恢复后的状态");
                poisoned.into_inner().clone()
            }
        }
    }

    pub fn update_profile<R, F>(&self, update: F) -> (R, UserProfile)
    where
        F: FnOnce(&mut UserProfile) -> R,
    {
        let mut profile = match self.user_profile.lock() {
            Ok(profile) => profile,
            Err(poisoned) => {
                log::warn!("用户画像锁已污染，继续使用恢复后的状态");
                poisoned.into_inner()
            }
        };

        let result = update(&mut profile);
        (result, profile.clone())
    }

    pub fn active_llm_provider(&self) -> String {
        self.snapshot_profile().llm_provider.active
    }

    pub fn llm_provider_config(&self) -> LlmProviderConfig {
        self.snapshot_profile().llm_provider
    }

    pub fn read_ai_polish_api_key(&self) -> String {
        match self.ai_polish_api_key.lock() {
            Ok(key) => key.clone(),
            Err(poisoned) => {
                log::warn!("AI 润色 API Key 锁已污染，继续使用恢复后的状态");
                poisoned.into_inner().clone()
            }
        }
    }

    pub fn set_ai_polish_api_key(&self, api_key: impl Into<String>) {
        let api_key = api_key.into();
        match self.ai_polish_api_key.lock() {
            Ok(mut key) => *key = api_key,
            Err(poisoned) => {
                log::warn!("AI 润色 API Key 锁已污染，继续使用恢复后的状态");
                *poisoned.into_inner() = api_key;
            }
        }
    }
}
