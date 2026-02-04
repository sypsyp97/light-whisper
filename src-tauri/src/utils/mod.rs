//! 工具模块
//!
//! # Rust 知识点：模块系统
//! Rust 的模块系统通过 `mod.rs` 文件来组织代码。
//! `pub mod xxx;` 声明一个公开的子模块，对应同目录下的 `xxx.rs` 文件。
//! `pub use xxx::*;` 把子模块中的所有公开内容重新导出，
//! 这样外部就可以用 `utils::AppError` 而不是 `utils::error::AppError`。

/// 统一错误类型
pub mod error;

/// 跨平台路径工具
pub mod paths;

// 重新导出常用类型，方便外部使用
// `pub use` 的作用是把内部模块的东西"提升"到当前模块级别
pub use error::AppError;
