//! 应用状态模块
//!
//! 管理应用的全局状态，包括 FunASR 进程、录音状态等。
//! 通过 Tauri 的状态管理系统，所有命令都可以安全地访问这些状态。

/// 全局应用状态定义
pub mod app_state;

// 重新导出 AppState 和 FunasrProcess，方便外部直接使用
pub use app_state::{AppState, FunasrProcess, DownloadTask};
