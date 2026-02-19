use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

pub struct RecordingSession {
    pub session_id: u64,
    pub stop_flag: Arc<AtomicBool>,
    pub samples: Arc<std::sync::Mutex<Vec<i16>>>,
    pub sample_rate: u32,
    pub audio_thread: Option<std::thread::JoinHandle<()>>,
    pub interim_task: Option<tokio::task::JoinHandle<()>>,
}

pub struct AppState {
    pub funasr_process: Arc<Mutex<Option<FunasrProcess>>>,
    pub funasr_ready: Arc<AtomicBool>,
    pub funasr_starting: Arc<AtomicBool>,
    pub download_task: Arc<Mutex<Option<DownloadTask>>>,
    pub recording: Arc<std::sync::Mutex<Option<RecordingSession>>>,
    pub session_counter: AtomicU64,
    pub input_method: Arc<std::sync::Mutex<String>>,
    /// 待粘贴文本队列：当粘贴时机恰逢新录音已开始，文本会暂存于此，
    /// 等下次录音结束后一并粘贴，避免丢失。
    pub pending_paste: Arc<std::sync::Mutex<Vec<String>>>,
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
        self.funasr_ready.load(Ordering::Relaxed)
    }

    pub fn set_funasr_ready(&self, ready: bool) {
        self.funasr_ready.store(ready, Ordering::Relaxed);
    }
}
