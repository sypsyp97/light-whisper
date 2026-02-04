//! 服务层模块
//!
//! 服务层封装了应用的核心业务逻辑，包括：
//! - FunASR 语音识别服务（Python 子进程管理）
//!
//! # 架构说明
//! 服务层位于命令层（commands）和底层工具层（utils）之间：
//! ```text
//! [前端] -> [commands 层] -> [services 层] -> [utils 层]
//! ```
//! 命令层负责接收前端请求，服务层负责处理业务逻辑。

/// FunASR 语音识别服务
pub mod funasr_service;
