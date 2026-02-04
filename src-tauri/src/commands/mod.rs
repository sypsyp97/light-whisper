//! Tauri 命令模块
//!
//! 所有前端可调用的命令都在这里注册。
//! 每个子模块对应一个功能领域。
//!
//! # 架构说明
//! ```text
//! 前端 (JavaScript/TypeScript)
//!   |
//!   | invoke('command_name', { params })
//!   v
//! 命令层 (commands/)
//!   |
//!   | 调用服务层函数
//!   v
//! 服务层 (services/)
//!   |
//!   | 使用工具和底层 API
//!   v
//! 工具层 (utils/)
//! ```
//!
//! # 命名约定
//! - 命令函数名使用 snake_case（蛇形命名法）
//! - 前端调用时也使用 snake_case
//! - 例如：Rust 函数 `transcribe_audio` -> 前端 `invoke('transcribe_audio')`

/// FunASR 语音识别相关命令
pub mod funasr;

/// 剪贴板操作命令
pub mod clipboard;

/// 窗口管理命令
pub mod window;

/// 全局快捷键命令
pub mod hotkey;
