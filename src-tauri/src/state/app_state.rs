//! 全局应用状态模块
//!
//! 这个模块定义了应用的全局状态，所有 Tauri 命令都可以通过 `tauri::State` 访问它。
//!
//! # Rust 知识点：多线程共享数据
//!
//! 在 Rust 中，多个线程想要共享和修改同一份数据，需要用到几个关键工具：
//!
//! ## Arc（原子引用计数）
//! `Arc` 是 "Atomically Reference Counted" 的缩写。
//! 它让多个线程可以共享同一份数据的所有权。
//! 当最后一个 `Arc` 被丢弃时，数据才会被释放。
//! 可以把它想象成一个"共享指针"。
//!
//! ## Mutex（互斥锁）
//! `Mutex` 保证同一时刻只有一个线程可以访问里面的数据。
//! 想要访问数据时，需要先 `.lock()` 获取锁。
//! 其他线程想访问时必须等待锁被释放。
//! 类似于厕所的门锁——进去了就锁上，出来才能让别人进。
//!
//! ## AtomicBool（原子布尔值）
//! `AtomicBool` 是一个线程安全的布尔值。
//! 对它的读写操作是"原子"的，不需要额外的锁。
//! 适合存储简单的开关状态（是/否）。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tokio::sync::oneshot;

/// 全局应用状态
///
/// 这个结构体存储了应用运行时需要共享的所有状态信息。
/// 它会被 Tauri 框架管理，通过 `tauri::State<AppState>` 在命令中访问。
///
/// # 为什么每个字段都用 Arc 包裹？
/// 因为 Tauri 的命令可能在不同的线程中执行，
/// `Arc` 确保每个线程都持有状态的有效引用。
///
/// # 示例：在 Tauri 命令中使用状态
/// ```rust
/// #[tauri::command]
/// async fn my_command(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
///     let is_ready = state.funasr_ready.load(Ordering::Relaxed);
///     Ok(())
/// }
/// ```
pub struct AppState {
    /// FunASR Python 子进程的句柄
    ///
    /// `Option<Child>` 表示可能有进程（Some）也可能没有（None）。
    /// 用 `Mutex` 保护是因为启动和停止进程需要修改这个值。
    /// `Child` 来自 `tokio::process`，代表一个异步子进程。
    pub funasr_process: Arc<Mutex<Option<FunasrProcess>>>,

    /// FunASR 服务器是否已就绪（可以接受请求）
    ///
    /// 使用 `AtomicBool` 而不是 `Mutex<bool>`，
    /// 因为这只是一个简单的布尔值，不需要互斥锁的开销。
    pub funasr_ready: Arc<AtomicBool>,

    /// FunASR 服务器是否正在启动中（防止并发启动）
    ///
    /// 模型加载需要约 25 秒，在此期间前端轮询可能多次触发 start_server。
    /// 这个标志确保同一时间只有一个启动流程在执行。
    pub funasr_starting: Arc<AtomicBool>,

    /// 模型下载任务（用于取消下载）
    pub download_task: Arc<Mutex<Option<DownloadTask>>>,
}

/// 为 `AppState` 实现 `Default` trait
///
/// # Rust 知识点：Default trait
/// `Default` trait 提供了一个 `default()` 方法来创建默认值。
/// 这在很多场景下很有用，比如初始化结构体时可以只指定部分字段。
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

/// 模型下载任务信息
pub struct DownloadTask {
    pub cancel: oneshot::Sender<()>,
}

/// FunASR 子进程及其标准输入/输出句柄
///
/// 将 stdout 包装成 BufReader 以保证多次读写时缓冲不丢失。
pub struct FunasrProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
}

impl AppState {
    /// 创建一个新的 AppState 实例
    ///
    /// # Rust 知识点：关联函数
    /// `new()` 是一个关联函数（类似于其他语言的静态方法/构造函数）。
    /// 调用方式是 `AppState::new()` 而不是 `some_state.new()`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查 FunASR 服务器是否就绪
    ///
    /// # Rust 知识点：方法
    /// `&self` 参数表示这是一个方法，需要通过实例调用：`state.is_funasr_ready()`。
    /// `&` 表示借用（不获取所有权），只是读取数据。
    pub fn is_funasr_ready(&self) -> bool {
        // `Ordering::Relaxed` 是最宽松的内存顺序，对于简单的布尔读取足够了
        self.funasr_ready.load(Ordering::Relaxed)
    }

    /// 设置 FunASR 服务器的就绪状态
    pub fn set_funasr_ready(&self, ready: bool) {
        self.funasr_ready.store(ready, Ordering::Relaxed);
    }

    
}
