use crate::utils::AppError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

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
        #[serde(skip_serializing_if = "Option::is_none")]
        preview: Option<bool>,
    },
    StreamFinish {
        session_id: u64,
    },
    StreamCancel {
        session_id: u64,
    },

    Transcribe {
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<crate::state::user_profile::R2T2Config>,

        #[serde(skip_serializing_if = "Option::is_none")]
        audio_path: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        audio_base64: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        audio_format: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        sample_rate: Option<u32>,

        #[serde(skip_serializing_if = "Option::is_none")]
        hot_words: Option<Vec<String>>,
    },

    Status,

    Stats,

    SetGpuIdle {
        seconds: u64,
    },
    SetAudioProcessing {
        enabled: bool,
    },

    Exit,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ServerResponse {
    pub(super) tentative_text: Option<String>,
    pub(super) session_id: Option<u64>,
    pub(super) sample_count: Option<usize>,
    #[serde(rename = "final")]
    pub(super) is_final: Option<bool>,

    pub(super) request_id: Option<u64>,

    pub(super) success: Option<bool>,

    pub(super) status: Option<String>,

    pub(super) text: Option<String>,

    pub(super) duration: Option<f64>,

    pub(super) error: Option<String>,

    pub(super) language: Option<String>,

    pub(super) message: Option<String>,

    pub(super) model_loaded: Option<bool>,

    pub(super) initialized: Option<bool>,

    pub(super) models: Option<ServerModelStatus>,

    pub(super) device: Option<String>,

    pub(super) gpu_name: Option<String>,

    pub(super) gpu_memory_total: Option<f64>,

    pub(super) engine: Option<String>,

    pub(super) input_mode: Option<String>,

    pub(super) stats: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
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
