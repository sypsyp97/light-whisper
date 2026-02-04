//! 统一错误类型模块
//!
//! 在 Rust 中，错误处理是通过 Result<T, E> 类型来实现的。
//! 这个模块定义了一个统一的错误枚举 `AppError`，
//! 把应用中可能出现的所有错误类型都收拢到一起。
//!
//! 为什么要统一错误类型？
//! - Tauri 的命令函数需要返回统一的错误类型
//! - 使用 `?` 操作符时，需要错误类型之间能自动转换
//! - 方便前端统一处理错误信息

use serde::Serialize;

/// 应用统一错误类型
///
/// # Rust 知识点
/// - `#[derive(Debug)]`：让这个类型可以用 `{:?}` 格式打印，方便调试
/// - `thiserror::Error`：自动实现 `std::error::Error` trait（特征）
/// - `#[error("...")]`：定义每种错误变体的显示文本
/// - `#[from]`：自动实现 `From` trait，让对应的错误类型可以用 `?` 自动转换
///
/// # 什么是枚举（enum）？
/// 枚举是 Rust 中的一种类型，它可以是多个变体中的任意一个。
/// 比如 `AppError::FunASR("启动失败".into())` 表示一个语音识别错误。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// FunASR 语音识别服务的错误
    #[error("FunASR错误: {0}")]
    FunASR(String),

    /// 文件读写等 IO 操作的错误
    /// `#[from]` 标注意味着 `std::io::Error` 可以自动转换成 `AppError::Io`
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 序列化/反序列化错误
    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),

    /// Tauri 框架本身的错误
    #[error("Tauri错误: {0}")]
    Tauri(String),

    /// 其他未分类的错误
    #[error("{0}")]
    Other(String),
}

/// 为 `AppError` 实现 `Serialize` trait（特征）
///
/// # 为什么需要 Serialize？
/// Tauri 的命令返回错误时，需要把错误序列化成字符串传给前端。
/// 这里我们简单地把错误转成它的文本描述。
///
/// # Rust 知识点：trait（特征）
/// trait 类似于其他语言中的接口（interface），定义了一组方法。
/// `impl Serialize for AppError` 意思是"为 AppError 类型实现 Serialize 接口"。
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // `self.to_string()` 会调用上面 `#[error("...")]` 定义的格式
        serializer.serialize_str(&self.to_string())
    }
}

/// 从 tauri::Error 转换为 AppError
///
/// # Rust 知识点：From trait
/// 实现 `From<A> for B` 后，就可以用 `B::from(a)` 或者在返回 Result 时用 `?` 自动转换。
/// 因为 tauri::Error 没有实现我们需要的某些 trait，所以手动实现转换。
impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        AppError::Tauri(err.to_string())
    }
}
