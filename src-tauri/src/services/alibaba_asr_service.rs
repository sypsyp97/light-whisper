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
    let _asr_guard = state.engine.native_asr_owner.lock().await;
    let (api_key, base, model, region) = {
        let _configuration = state.engine.funasr_lifecycle_op.lock().await;
        if paths::read_engine_config() != "alibaba-asr" {
            return Err(AppError::Asr("语音识别引擎已切换，请重试".into()));
        }
        (
            state.read_online_asr_api_key(),
            paths::read_alibaba_endpoint(),
            paths::read_alibaba_model(),
            paths::read_alibaba_region(),
        )
    };
    if api_key.is_empty() {
        return Err(AppError::Asr("Alibaba DashScope API Key 未配置".into()));
    }
    if exceeds_dashscope_limit(audio_wav.len()) {
        return Err(AppError::Asr(format!(
            "音频过大：{} MB 经 base64 编码后超出 DashScope 10 MB 请求体上限",
            audio_wav.len() / 1024 / 1024
        )));
    }

    log::info!(
        "DashScope ASR 请求: model={}, region={}, 音频 {} KB",
        model,
        region,
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

pub(crate) async fn transcribe_via_dashscope_asr(
    state: &AppState,
    base: &str,
    model: &str,
    api_key: &str,
    audio_wav: Vec<u8>,
) -> Result<TranscriptionResult, AppError> {
    let data_url = format!("data:audio/wav;base64,{}", b64(&audio_wav));

    let body = if model.starts_with("qwen-audio-3.")
        && model.contains("-asr-flash")
        && !model.contains("-filetrans")
    {
        serde_json::json!({
            "model": model,
            "input": {"messages": [{
                "role": "user",
                "content": [{"type": "input_audio", "input_audio": {"data": data_url}}]
            }]},
            // WAV carries the actual rate, including original-rate fallback recordings.
            "parameters": {"format": "wav"}
        })
    } else {
        serde_json::json!({
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
        })
    };

    let url = format!("{}{}", base, ASR_PATH);
    let resp = state
        .http_client
        .post(&url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .header("X-DashScope-SSE", "disable")
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

    if parsed
        .output
        .as_ref()
        .and_then(|output| output.choices.as_ref())
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .finish_reason
                    .as_deref()
                    .is_some_and(|reason| reason != "stop")
            })
        })
    {
        return Err(AppError::Asr("DashScope ASR 转写未正常完成，请重试".into()));
    }

    let text_out = parsed
        .output
        .and_then(|o| {
            o.text
                .or_else(|| {
                    o.output?
                        .pointer("/sentence/text")?
                        .as_str()
                        .map(str::to_string)
                })
                .or_else(|| {
                    o.choices?
                        .into_iter()
                        .next()?
                        .message
                        .and_then(|m| m.content_text())
                })
        })
        .ok_or_else(|| {
            AppError::Asr("DashScope ASR 响应缺少转写文本，请重试或检查模型配置".into())
        })?;

    if text_out.is_empty() {
        log::warn!(
            "DashScope ASR 返回空文本 (响应{}字符)",
            text.chars().count()
        );
    }

    Ok(TranscriptionResult {
        text: text_out,
        duration: None,
        success: true,
        error: None,
        language: None,
    })
}

pub(crate) async fn transcribe_via_omni_chat(
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
    let mut completed = false;

    while let Some(event) = stream.next().await {
        let event =
            event.map_err(|e| AppError::Asr(format!("DashScope Omni 流式读取失败: {}", e)))?;
        let data = event.data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            completed = true;
            break;
        }
        let parsed: OmniStreamChunk = serde_json::from_str(data)
            .map_err(|_| AppError::Asr("DashScope Omni 流式响应格式无效，请重试".into()))?;
        if parsed
            .code
            .as_deref()
            .is_some_and(|code| !code.is_empty() && code != "Success")
        {
            return Err(AppError::Asr(
                "DashScope Omni 返回服务错误，请检查 API Key、模型权限或额度后重试".into(),
            ));
        }
        if let Some(err_obj) = parsed.error {
            stream_error = Some(
                err_obj
                    .message
                    .unwrap_or_else(|| "DashScope Omni 流中返回错误".into()),
            );
        }
        if let Some(choices) = parsed.choices {
            for choice in choices {
                if let Some(reason) = choice.finish_reason.as_deref() {
                    if reason != "stop" {
                        return Err(AppError::Asr(
                            "DashScope Omni 转写未正常完成，请重试".into(),
                        ));
                    }
                    completed = true;
                }
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

    if !completed {
        return Err(AppError::Asr("DashScope Omni 响应提前结束，请重试".into()));
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
    text: Option<String>,
    output: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct DashScopeAsrChoice {
    finish_reason: Option<String>,
    message: Option<DashScopeAsrMessage>,
}

#[derive(Deserialize)]
struct DashScopeAsrMessage {
    content: Option<DashScopeAsrContentField>,
}

impl DashScopeAsrMessage {
    fn content_text(self) -> Option<String> {
        match self.content? {
            DashScopeAsrContentField::Text(s) => Some(s),
            DashScopeAsrContentField::List(items) => {
                let texts = items
                    .into_iter()
                    .filter_map(|item| item.text)
                    .collect::<Vec<_>>();
                (!texts.is_empty()).then(|| texts.join(""))
            }
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
    code: Option<String>,
    choices: Option<Vec<OmniStreamChoice>>,
    error: Option<OmniStreamError>,
}

#[derive(Deserialize)]
struct OmniStreamChoice {
    finish_reason: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const NATIVE_MODEL: &str = "qwen-audio-3.1-asr-flash";
    const AUDIO: &[u8] = b"RIFFtest";
    const AUDIO_DATA_URL: &str = "data:audio/wav;base64,UklGRnRlc3Q=";

    struct CapturedRequest {
        request_line: String,
        headers: BTreeMap<String, String>,
        body: Value,
    }

    async fn transcribe_at_capture_server(
        model: &str,
        status: &str,
        response: Value,
    ) -> (Result<TranscriptionResult, AppError>, CapturedRequest) {
        transcribe_at_raw_capture_server(model, status, response.to_string(), false).await
    }

    async fn transcribe_at_raw_capture_server(
        model: &str,
        status: &str,
        response: String,
        omni: bool,
    ) -> (Result<TranscriptionResult, AppError>, CapturedRequest) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_string();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let (header_end, content_length) = loop {
                let mut chunk = [0_u8; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0, "client closed before completing the request");
                request.extend_from_slice(&chunk[..count]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .expect("JSON request must have content-length");
                if request.len() >= header_end + 4 + content_length {
                    break (header_end, content_length);
                }
            };
            let header_text = String::from_utf8_lossy(&request[..header_end]);
            let mut header_lines = header_text.lines();
            let request_line = header_lines.next().unwrap().to_string();
            let headers = header_lines
                .map(|line| {
                    let (name, value) = line.split_once(':').unwrap();
                    (name.to_ascii_lowercase(), value.trim().to_string())
                })
                .collect();
            let body_start = header_end + 4;
            let body = serde_json::from_slice(&request[body_start..body_start + content_length])
                .expect("request body must be JSON");
            let content_type = if omni {
                "text/event-stream"
            } else {
                "application/json"
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len(),
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            CapturedRequest {
                request_line,
                headers,
                body,
            }
        });
        let mut state = AppState::new();
        state.http_client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            if omni {
                transcribe_via_omni_chat(&state, &base, model, "test-api-key", AUDIO.to_vec()).await
            } else {
                transcribe_via_dashscope_asr(&state, &base, model, "test-api-key", AUDIO.to_vec())
                    .await
            }
        })
        .await
        .expect("loopback transcription must finish");
        let request = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("capture server must finish")
            .expect("capture server must not panic");
        (result, request)
    }

    #[tokio::test]
    async fn missing_transcript_fields_are_asr_errors() {
        let mut accepted = Vec::new();
        for response in [
            json!({}),
            json!({"output": null}),
            json!({"output": {}}),
            json!({"output": {"text": null}}),
            json!({"output": {"output": {"sentence": {}}}}),
            json!({"output": {"output": {"sentence": {"text": null}}}}),
            json!({"output": {"choices": []}}),
            json!({"output": {"choices": [{"message": {}}]}}),
            json!({"output": {"choices": [{"message": {"content": null}}]}}),
            json!({"output": {"choices": [{"message": {"content": []}}]}}),
            json!({"output": {"choices": [{"message": {"content": [{}]}}]}}),
        ] {
            let (result, _) =
                transcribe_at_capture_server(NATIVE_MODEL, "200 OK", response.clone()).await;
            if !matches!(result, Err(AppError::Asr(_))) {
                accepted.push(response);
            }
        }
        assert!(
            accepted.is_empty(),
            "missing transcript accepted: {accepted:?}"
        );
    }

    #[tokio::test]
    async fn explicit_empty_transcripts_remain_successful_silence() {
        for response in [
            json!({"output": {"text": ""}}),
            json!({"output": {"output": {"sentence": {"text": ""}}}}),
            json!({"output": {"choices": [{"message": {"content": ""}}]}}),
            json!({"output": {"choices": [{"message": {"content": [{"text": ""}]}}]}}),
        ] {
            let (result, _) =
                transcribe_at_capture_server(NATIVE_MODEL, "200 OK", response.clone()).await;
            let result =
                result.unwrap_or_else(|error| panic!("explicit silence {response}: {error:?}"));
            assert!(result.success);
            assert!(result.text.is_empty());
            assert!(result.error.is_none());
        }
    }

    const OMNI_CONTENT: &str =
        "data: {\"choices\":[{\"delta\":{\"content\":\"spoken words\"}}]}\n\n";

    #[tokio::test]
    async fn omni_provider_error_code_after_content_is_an_asr_error() {
        let (result, _) = transcribe_at_raw_capture_server(
            "qwen3-omni-flash",
            "200 OK",
            format!("{OMNI_CONTENT}data: {{\"code\":\"InvalidApiKey\",\"message\":\"synthetic-provider-error\"}}\n\ndata: [DONE]\n\n"),
            true,
        )
        .await;
        assert!(
            matches!(result, Err(AppError::Asr(_))),
            "provider error must fail instead of returning partial text: {result:?}"
        );
    }

    #[tokio::test]
    async fn omni_success_code_metadata_preserves_completed_transcript() {
        let (result, _) = transcribe_at_raw_capture_server(
            "qwen3-omni-flash",
            "200 OK",
            format!("data: {{\"code\":\"Success\"}}\n\n{OMNI_CONTENT}data: [DONE]\n\n"),
            true,
        )
        .await;
        let result = result.expect("Success metadata must not reject a valid transcription");
        assert!(result.success);
        assert_eq!(result.text, "spoken words");
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn omni_malformed_event_after_content_is_an_asr_error() {
        let (result, _) = transcribe_at_raw_capture_server(
            "qwen3-omni-flash",
            "200 OK",
            format!("{OMNI_CONTENT}data: {{broken-json\n\ndata: [DONE]\n\n"),
            true,
        )
        .await;
        assert!(
            matches!(result, Err(AppError::Asr(_))),
            "malformed event must fail: {result:?}"
        );
    }

    #[tokio::test]
    async fn omni_content_without_completion_is_an_asr_error() {
        let (result, _) = transcribe_at_raw_capture_server(
            "qwen3-omni-flash",
            "200 OK",
            OMNI_CONTENT.to_string(),
            true,
        )
        .await;
        assert!(
            matches!(result, Err(AppError::Asr(_))),
            "truncated stream must fail: {result:?}"
        );
    }

    #[tokio::test]
    async fn omni_accepts_done_or_stop_completion() {
        for completion in [
            "data: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        ] {
            let (result, request) = transcribe_at_raw_capture_server(
                "qwen3-omni-flash",
                "200 OK",
                format!("{OMNI_CONTENT}{completion}"),
                true,
            )
            .await;
            assert_eq!(
                request.request_line,
                "POST /compatible-mode/v1/chat/completions HTTP/1.1"
            );
            let result = result.unwrap();
            assert!(result.success);
            assert_eq!(result.text, "spoken words");
            assert!(result.error.is_none());
        }
    }

    #[tokio::test]
    async fn native_model_sends_documented_audio_request_without_polishing() {
        let (_, request) = transcribe_at_capture_server(
            NATIVE_MODEL,
            "200 OK",
            json!({"output": {"text": "原始转写"}}),
        )
        .await;

        assert_eq!(
            request.request_line,
            "POST /api/v1/services/aigc/multimodal-generation/generation HTTP/1.1"
        );
        assert_eq!(request.headers["authorization"], "Bearer test-api-key");
        assert_eq!(request.headers["content-type"], "application/json");
        assert_eq!(
            request.body,
            json!({
                "model": NATIVE_MODEL,
                "input": {"messages": [{
                    "role": "user",
                    "content": [{
                        "type": "input_audio",
                        "input_audio": {"data": AUDIO_DATA_URL}
                    }]
                }]},
                "parameters": {"format": "wav"}
            })
        );
        assert_eq!(
            request.headers.get("x-dashscope-sse").map(String::as_str),
            Some("disable")
        );
    }

    #[tokio::test]
    async fn native_model_returns_output_text() {
        let (result, _) = transcribe_at_capture_server(
            NATIVE_MODEL,
            "200 OK",
            json!({"output": {"text": "保留原话和数字 123。"}}),
        )
        .await;
        let result = result.unwrap();
        assert!(result.success);
        assert_eq!(result.text, "保留原话和数字 123。");
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn native_model_returns_nested_sentence_text() {
        let (result, _) = transcribe_at_capture_server(
            NATIVE_MODEL,
            "200 OK",
            json!({"output": {"output": {"sentence": {"text": "嵌套的原始转写。"}}}}),
        )
        .await;
        let result = result.unwrap();
        assert!(result.success);
        assert_eq!(result.text, "嵌套的原始转写。");
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn legacy_model_preserves_request_and_string_or_list_transcripts() {
        for content in [
            json!("旧版转写。"),
            json!([{"text": "旧版"}, {"text": "转写。"}]),
        ] {
            let (result, request) = transcribe_at_capture_server(
                "qwen3-asr-flash",
                "200 OK",
                json!({"output": {"choices": [{"message": {"content": content}}]}}),
            )
            .await;
            assert_eq!(
                request.request_line,
                "POST /api/v1/services/aigc/multimodal-generation/generation HTTP/1.1"
            );
            assert_eq!(
                request.body,
                json!({
                    "model": "qwen3-asr-flash",
                    "input": {"messages": [
                        {"role": "system", "content": [{"text": ""}]},
                        {"role": "user", "content": [{"audio": AUDIO_DATA_URL}]}
                    ]},
                    "parameters": {"asr_options": {"enable_itn": true}}
                })
            );
            let result = result.unwrap();
            assert!(result.success);
            assert_eq!(result.text, "旧版转写。");
            assert!(result.error.is_none());
        }
    }

    #[tokio::test]
    async fn native_model_preserves_http_error_status_and_body() {
        let (result, _) = transcribe_at_capture_server(
            NATIVE_MODEL,
            "429 Too Many Requests",
            json!({"code": "Throttling", "message": "test quota exceeded"}),
        )
        .await;
        let error = result.unwrap_err();
        let AppError::Asr(message) = error else {
            panic!("HTTP failure must remain an ASR error");
        };
        assert!(message.contains("429"));
        assert!(message.contains("Throttling"));
        assert!(message.contains("test quota exceeded"));
    }

    #[tokio::test]
    async fn native_model_preserves_unsuccessful_api_result() {
        let (result, _) = transcribe_at_capture_server(
            NATIVE_MODEL,
            "200 OK",
            json!({"code": "InvalidApiKey", "message": "test key rejected"}),
        )
        .await;
        let result = result.unwrap();
        assert!(!result.success);
        assert!(result.text.is_empty());
        assert_eq!(result.error.as_deref(), Some("test key rejected"));
    }
}
