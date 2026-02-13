use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

pub struct AppState {
    pub funasr_process: Arc<Mutex<Option<FunasrProcess>>>,
    pub funasr_ready: Arc<AtomicBool>,
    pub funasr_starting: Arc<AtomicBool>,
    pub download_task: Arc<Mutex<Option<DownloadTask>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            funasr_process: Arc::new(Mutex::new(None)),
            funasr_ready: Arc::new(AtomicBool::new(false)),
            funasr_starting: Arc::new(AtomicBool::new(false)),
            download_task: Arc::new(Mutex::new(None)),
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
