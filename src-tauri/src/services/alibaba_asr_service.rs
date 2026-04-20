use serde::Deserialize;

use crate::services::funasr_service::TranscriptionResult;
use crate::state::AppState;
use crate::utils::{paths, AppError};

const ASR_PATH: &str = "/api/v1/services/aigc/multimodal-generation/generation";
const OMNI_CHAT_PATH: &str = "/compatible-mode/v1/chat/completions";
const REQUEST_TIMEOUT_SECS: u64 = 60;

/// DashScope 对入站请求体有 10MB 上限。请求体里的音频是 base64 字符串，
/// base64 会把原始字节放大 4/3 倍。这里按放大后的长度比对，避免"本地 9MB 通过
/// 校验→DashScope 收到 12MB 请求体 400"的错位。
const MAX_BASE64_AUDIO_BYTES: usize = 10 * 1024 * 1024;

fn exceeds_dashscope_limit(raw_len: usize) -> bool {
    // ceil(raw_len * 4 / 3) > MAX_BASE64_AUDIO_BYTES
    raw_len.saturating_mul(4) / 3 > MAX_BASE64_AUDIO_BYTES
}

pub async fn transcribe(
    state: &AppState,
    audio_wav: Vec<u8>,
) -> Result<TranscriptionResult, AppError> {
    let api_key = state.read_online_asr_api_key();
    if api_key.is_empty() {
        return Err(AppError::Asr("Alibaba DashScope API Key 未配置".into()));
    }
    if exceeds_dashscope_limit(audio_wav.len()) {
        return Err(AppError::Asr(format!(
            "音频过大：{} MB 经 base64 编码后超出 DashScope 10 MB 请求体上限",
            audio_wav.len() / 1024 / 1024
        )));
    }

    let base = paths::read_alibaba_endpoint();
    let model = paths::read_alibaba_model();
    log::info!(
        "DashScope ASR 请求: model={}, region={}, 音频 {} KB",
        model,
        paths::read_alibaba_region(),
        audio_wav.len() / 1024,
    );

    if paths::alibaba_model_uses_omni_chat(&model) {
        transcribe_via_omni_chat(state, &base, &model, &api_key, audio_wav).await
    } else {
        transcribe_via_dashscope_asr(state, &base, &model, &api_key, audio_wav).await
    }
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

async fn transcribe_via_dashscope_asr(
    state: &AppState,
    base: &str,
    model: &str,
    api_key: &str,
    audio_wav: Vec<u8>,
) -> Result<TranscriptionResult, AppError> {
    let data_url = format!("data:audio/wav;base64,{}", b64(&audio_wav));

    let body = serde_json::json!({
        "model": model,
        "input": {
            "messages": [
                {"role": "system", "content": [{"text": ""}]},
                {"role": "user", "content": [{"audio": data_url}]}
            ]
        },
        "parameters": {
            "asr_options": {"enable_itn": true}
        }
    });

    let url = format!("{}{}", base, ASR_PATH);
    let resp = state
        .http_client
        .post(&url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| AppError::Asr(format!("DashScope ASR 请求失败: {}", e)))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::Asr(format!("读取 DashScope ASR 响应失败: {}", e)))?;

    if !status.is_success() {
        return Err(AppError::Asr(format!(
            "DashScope ASR HTTP {}: {}",
            status, text
        )));
    }

    let parsed: DashScopeAsrResponse = serde_json::from_str(&text)
        .map_err(|e| AppError::Asr(format!("解析 DashScope ASR 响应失败: {}", e)))?;

    if let Some(code) = parsed.code.as_deref() {
        if !code.is_empty() && code != "Success" {
            return Ok(TranscriptionResult {
                text: String::new(),
                duration: None,
                success: false,
                error: Some(
                    parsed
                        .message
                        .unwrap_or_else(|| format!("DashScope ASR 错误: {}", code)),
                ),
                language: None,
            });
        }
    }

    let text_out = parsed
        .output
        .and_then(|o| o.choices)
        .and_then(|mut c| c.drain(..).next())
        .and_then(|c| c.message)
        .map(|m| m.content_text())
        .unwrap_or_default();

    if text_out.is_empty() {
        log::warn!("DashScope ASR 返回空文本，原始响应: {}", text);
    }

    Ok(TranscriptionResult {
        text: text_out,
        duration: None,
        success: true,
        error: None,
        language: None,
    })
}

async fn transcribe_via_omni_chat(
    state: &AppState,
    base: &str,
    model: &str,
    api_key: &str,
    audio_wav: Vec<u8>,
) -> Result<TranscriptionResult, AppError> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let encoded = b64(&audio_wav);
    let data_url = format!("data:;base64,{}", encoded);

    let body = serde_json::json!({
        "model": model,
        "stream": true,
        "stream_options": {"include_usage": false},
        "modalities": ["text"],
        "messages": [
            {
                "role": "system",
                "content": "You are a professional speech recognizer. Transcribe the audio verbatim. Output only the transcription with no extra commentary."
            },
            {
                "role": "user",
                "content": [
                    {"type": "input_audio", "input_audio": {"data": data_url, "format": "wav"}},
                    {"type": "text", "text": "Please transcribe this audio into text. Return the transcription only."}
                ]
            }
        ]
    });

    let url = format!("{}{}", base, OMNI_CHAT_PATH);
    let resp = state
        .http_client
        .post(&url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body)
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| AppError::Asr(format!("DashScope Omni 请求失败: {}", e)))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Asr(format!(
            "DashScope Omni HTTP {}: {}",
            status, body
        )));
    }

    let mut stream = resp.bytes_stream().eventsource();
    let mut collected = String::new();
    let mut stream_error: Option<String> = None;
    let mut logged_parse_failure = false;

    while let Some(event) = stream.next().await {
        let event =
            event.map_err(|e| AppError::Asr(format!("DashScope Omni 流式读取失败: {}", e)))?;
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let parsed = match serde_json::from_str::<OmniStreamChunk>(data) {
            Ok(p) => p,
            Err(e) => {
                // 对第一个 parse-fail 留一条日志，后续的保持安静——否则一条损坏
                // 会淹没日志。HTTP 层错误已经在主路径上显式处理过，这里只兜底。
                if !logged_parse_failure {
                    log::warn!(
                        "DashScope Omni 流中跳过无法解析的 chunk: {} (err={})",
                        data,
                        e
                    );
                    logged_parse_failure = true;
                }
                continue;
            }
        };
        if let Some(err_obj) = parsed.error {
            stream_error = Some(
                err_obj
                    .message
                    .unwrap_or_else(|| "DashScope Omni 流中返回错误".into()),
            );
        }
        if let Some(choices) = parsed.choices {
            for choice in choices {
                if let Some(delta) = choice.delta {
                    if let Some(content) = delta.content_as_string() {
                        collected.push_str(&content);
                    }
                }
            }
        }
    }

    if let Some(err) = stream_error {
        return Err(AppError::Asr(format!("DashScope Omni 返回错误: {}", err)));
    }

    let collected = collected.trim().to_string();
    if collected.is_empty() {
        // 空流没有 [DONE] 以外的任何内容，通常意味着上游模型拒绝或配额耗尽，
        // 不要当成"用户啥也没说"默默返回空串——给一个可操作的错误。
        return Err(AppError::Asr(
            "DashScope Omni 流式响应为空，请检查模型可用性、额度或控制台日志".into(),
        ));
    }

    Ok(TranscriptionResult {
        text: collected,
        duration: None,
        success: true,
        error: None,
        language: None,
    })
}

#[derive(Deserialize)]
struct DashScopeAsrResponse {
    output: Option<DashScopeAsrOutput>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct DashScopeAsrOutput {
    choices: Option<Vec<DashScopeAsrChoice>>,
}

#[derive(Deserialize)]
struct DashScopeAsrChoice {
    message: Option<DashScopeAsrMessage>,
}

#[derive(Deserialize)]
struct DashScopeAsrMessage {
    content: Option<DashScopeAsrContentField>,
}

impl DashScopeAsrMessage {
    fn content_text(self) -> String {
        match self.content {
            Some(DashScopeAsrContentField::Text(s)) => s,
            Some(DashScopeAsrContentField::List(items)) => items
                .into_iter()
                .filter_map(|i| i.text)
                .collect::<Vec<_>>()
                .join(""),
            None => String::new(),
        }
    }
}

/// DashScope 历史上既见过 `"content": "..."` 也见过
/// `"content": [{"text": "..."}]`。两种都兜底。
#[derive(Deserialize)]
#[serde(untagged)]
enum DashScopeAsrContentField {
    Text(String),
    List(Vec<DashScopeAsrContent>),
}

#[derive(Deserialize)]
struct DashScopeAsrContent {
    text: Option<String>,
}

#[derive(Deserialize)]
struct OmniStreamChunk {
    choices: Option<Vec<OmniStreamChoice>>,
    error: Option<OmniStreamError>,
}

#[derive(Deserialize)]
struct OmniStreamChoice {
    delta: Option<OmniStreamDelta>,
}

#[derive(Deserialize)]
struct OmniStreamDelta {
    content: Option<serde_json::Value>,
}

impl OmniStreamDelta {
    /// delta.content 可能是字符串或 `[{"type":"text","text":"..."}]` 形式。两种都处理。
    fn content_as_string(&self) -> Option<String> {
        let content = self.content.as_ref()?;
        if let Some(s) = content.as_str() {
            return Some(s.to_string());
        }
        if let Some(arr) = content.as_array() {
            let mut out = String::new();
            for item in arr {
                if let Some(obj) = item.as_object() {
                    if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                        out.push_str(text);
                    }
                }
            }
            if !out.is_empty() {
                return Some(out);
            }
        }
        None
    }
}

#[derive(Deserialize)]
struct OmniStreamError {
    message: Option<String>,
}
