//! FunASR stdin/stdout protocol types and response parsing.

use crate::utils::AppError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// 发送给 Python 服务器的命令
///
/// Python 端期望的 JSON 格式是扁平的：
/// - `{"action": "status"}`
/// - `{"action": "transcribe", "audio_path": "/path/to/file.wav"}`
/// - `{"action": "exit"}`
///
/// 使用 `#[serde(tag = "action")]` 生成带 `action` 字段的扁平 JSON，
/// `rename_all = "snake_case"` 将变体名转为小写下划线格式。
#[derive(Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ServerCommand {
    StreamStart {
        session_id: u64,
        context: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        hot_words: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    StreamFeed {
        session_id: u64,
        offset: usize,
        audio_base64: String,
        audio_format: String,
        sample_rate: u32,
    },
    StreamFinish {
        session_id: u64,
    },
    StreamCancel {
        session_id: u64,
    },
    /// 转写音频文件
    Transcribe {
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<crate::state::user_profile::R2T2Config>,
        /// 音频文件的路径
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_path: Option<String>,
        /// 内存音频负载（Base64）
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_base64: Option<String>,
        /// 内存音频编码格式
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_format: Option<String>,
        /// 内存音频采样率
        #[serde(skip_serializing_if = "Option::is_none")]
        sample_rate: Option<u32>,
        /// 热词列表（可选）
        #[serde(skip_serializing_if = "Option::is_none")]
        hot_words: Option<Vec<String>>,
    },
    /// 查询服务器状态
    Status,
    /// 退出服务器
    Exit,
}

/// Python 服务器的 JSON 响应
///
/// 这个结构体对应 Python 服务器返回的 JSON 格式。
/// `Option<T>` 表示字段可能存在也可能不存在。
#[derive(Debug, Deserialize)]
pub(super) struct ServerResponse {
    pub(super) tentative_text: Option<String>,
    pub(super) session_id: Option<u64>,
    pub(super) sample_count: Option<usize>,
    #[serde(rename = "final")]
    pub(super) is_final: Option<bool>,
    /// 请求 ID；新协议用于丢弃取消/超时后迟到的旧响应
    pub(super) request_id: Option<u64>,
    /// 操作是否成功
    pub(super) success: Option<bool>,
    /// 状态标识
    pub(super) status: Option<String>,
    /// 转写得到的文本
    pub(super) text: Option<String>,
    /// 音频时长
    pub(super) duration: Option<f64>,
    /// 错误信息
    pub(super) error: Option<String>,
    /// 检测到的语言
    pub(super) language: Option<String>,
    /// 附加消息
    pub(super) message: Option<String>,
    /// 模型是否已加载
    pub(super) model_loaded: Option<bool>,
    /// 模型是否已初始化（Python status 返回）
    pub(super) initialized: Option<bool>,
    /// 模型加载状态
    pub(super) models: Option<ServerModelStatus>,
    /// 设备信息
    pub(super) device: Option<String>,
    /// GPU 名称
    pub(super) gpu_name: Option<String>,
    /// GPU 总显存（GB）
    pub(super) gpu_memory_total: Option<f64>,
    /// 当前引擎
    pub(super) engine: Option<String>,
    /// 服务端实际采用的输入模式（memory/path）
    pub(super) input_mode: Option<String>,
}

/// Python status 返回的模型状态
#[derive(Debug, Deserialize, Clone)]
pub(super) struct ServerModelStatus {
    pub(super) asr: Option<bool>,
    pub(super) vad: Option<bool>,
    pub(super) punc: Option<bool>,
}

impl ServerResponse {
    pub(super) fn is_model_loaded(&self) -> bool {
        self.model_loaded.unwrap_or_else(|| {
            self.models
                .as_ref()
                .map(|m| {
                    m.asr.unwrap_or(false) && m.vad.unwrap_or(false) && m.punc.unwrap_or(false)
                })
                .unwrap_or(false)
        })
    }
}

pub(super) async fn read_json_response<T, R>(
    reader: &mut R,
    timeout: Duration,
    context: &str,
) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de>,
    R: AsyncBufRead + Unpin,
{
    read_json_response_matching(reader, timeout, context, |_| true).await
}

pub(super) async fn read_json_response_matching<T, R>(
    reader: &mut R,
    timeout: Duration,
    context: &str,
    mut accept: impl FnMut(&T) -> bool,
) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de>,
    R: AsyncBufRead + Unpin,
{
    let start_at = Instant::now();
    let mut line_bytes = Vec::new();

    loop {
        let remaining = timeout
            .checked_sub(start_at.elapsed())
            .ok_or_else(|| AppError::Asr(format!("{}超时", context)))?;

        line_bytes.clear();
        let read_result =
            tokio::time::timeout(remaining, reader.read_until(b'\n', &mut line_bytes)).await;

        match read_result {
            Ok(Ok(0)) => {
                return Err(AppError::Asr(format!("{}失败：stdout 已关闭", context)));
            }
            Ok(Ok(_)) => {
                let line = match std::str::from_utf8(&line_bytes) {
                    Ok(line) => std::borrow::Cow::Borrowed(line),
                    Err(err) => {
                        log::warn!(
                            "{}阶段收到非 UTF-8 输出，已按损坏文本容错处理: {}",
                            context,
                            err
                        );
                        String::from_utf8_lossy(&line_bytes)
                    }
                };

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(value) = serde_json::from_str::<T>(trimmed) {
                    if accept(&value) {
                        return Ok(value);
                    }
                    log::warn!("{}阶段丢弃了不匹配的旧 JSON 响应", context);
                    continue;
                }

                // 某些 Windows 机器上，第三方库会把噪音输出和 JSON 响应挤在同一行。
                // 尝试从首个 '{' 到末尾 '}' 提取有效 JSON，避免一次脏输出导致整次初始化失败。
                if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
                    if start < end {
                        if let Ok(value) = serde_json::from_str::<T>(&trimmed[start..=end]) {
                            if !accept(&value) {
                                log::warn!("{}阶段丢弃了不匹配的旧 JSON 响应", context);
                                continue;
                            }
                            log::warn!("{}阶段从混合输出中恢复了 JSON 响应", context);
                            return Ok(value);
                        }
                    }
                }

                log::warn!(
                    "{}阶段收到非JSON输出 ({}字符)",
                    context,
                    trimmed.chars().count()
                );
                continue;
            }
            Ok(Err(e)) => {
                return Err(AppError::Asr(format!("{}失败：{}", context, e)));
            }
            Err(_) => {
                return Err(AppError::Asr(format!("{}超时", context)));
            }
        }
    }
}
