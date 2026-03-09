use std::time::Duration;

use serde_json::Value;
use tauri::Emitter;

use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::ApiFormat;

const STREAM_EVENT_TIMEOUT_SECS: u64 = 90;

#[derive(Debug, Clone, Copy)]
pub struct LlmRequestOptions<'a> {
    pub stream: bool,
    pub json_output: bool,
    pub stream_event: Option<&'a str>,
    pub session_id: Option<u64>,
}

impl Default for LlmRequestOptions<'_> {
    fn default() -> Self {
        Self {
            stream: false,
            json_output: false,
            stream_event: None,
            session_id: None,
        }
    }
}

fn dynamic_timeout(base_secs: u64, text_len: usize) -> Duration {
    let extra = (text_len / 200) as u64;
    Duration::from_secs(base_secs.saturating_add(extra).min(120))
}

pub fn build_llm_body(
    endpoint: &LlmEndpoint,
    system_prompt: &str,
    user_content: &str,
    options: LlmRequestOptions<'_>,
) -> Value {
    match endpoint.api_format {
        ApiFormat::Anthropic => serde_json::json!({
            "model": endpoint.model,
            "max_tokens": 4096,
            "system": system_prompt,
            "messages": [{"role": "user", "content": user_content}],
            "stream": options.stream,
        }),
        ApiFormat::OpenaiCompat => {
            let is_responses_api = endpoint.api_url.contains("/v1/responses");

            let mut body = if is_responses_api {
                serde_json::json!({
                    "model": endpoint.model,
                    "instructions": system_prompt,
                    "input": [
                        {"role": "developer", "content": if options.json_output { "Output json." } else { "Follow the system instructions exactly." }},
                        {"role": "user", "content": user_content},
                    ],
                })
            } else {
                serde_json::json!({
                    "model": endpoint.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_content},
                    ],
                })
            };

            if options.json_output {
                if is_responses_api {
                    body["text"] = serde_json::json!({ "format": { "type": "json_object" } });
                } else {
                    body["response_format"] = serde_json::json!({ "type": "json_object" });
                }
            }

            if is_responses_api {
                body["reasoning"] = serde_json::json!({"effort": "medium"});
            } else if endpoint.api_url.contains("cerebras") {
                body["reasoning_effort"] = serde_json::json!("low");
            }

            if options.stream && !is_responses_api {
                body["stream"] = serde_json::json!(true);
            }

            body
        }
    }
}

fn emit_stream_chunk(
    app_handle: &tauri::AppHandle,
    event_name: Option<&str>,
    session_id: Option<u64>,
    chunk: &str,
    tokens: usize,
) {
    if let Some(event_name) = event_name {
        let mut payload = serde_json::json!({
            "status": "streaming",
            "chunk": chunk,
        });
        if tokens > 0 {
            payload["tokens"] = serde_json::json!(tokens);
        }
        if let Some(session_id) = session_id {
            payload["sessionId"] = serde_json::json!(session_id);
        }
        let _ = app_handle.emit(event_name, payload);
    }
}

fn emit_stream_tokens(
    app_handle: &tauri::AppHandle,
    event_name: Option<&str>,
    session_id: Option<u64>,
    tokens: usize,
) {
    if let Some(event_name) = event_name {
        let mut payload = serde_json::json!({
            "status": "streaming",
            "tokens": tokens,
        });
        if let Some(session_id) = session_id {
            payload["sessionId"] = serde_json::json!(session_id);
        }
        let _ = app_handle.emit(event_name, payload);
    }
}

fn anthropic_output_tokens(json: &Value) -> Option<usize> {
    json["usage"]["output_tokens"]
        .as_u64()
        .or_else(|| json["message"]["usage"]["output_tokens"].as_u64())
        .map(|value| value as usize)
}

pub async fn read_sse_stream(
    response: reqwest::Response,
    app_handle: &tauri::AppHandle,
    event_name: Option<&str>,
    session_id: Option<u64>,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let mut stream = response.bytes_stream().eventsource();

    loop {
        match tokio::time::timeout(event_timeout, stream.next()).await {
            Ok(Some(Ok(event))) => {
                let data = event.data.trim();
                if data == "[DONE]" {
                    return Ok(accumulated);
                }
                if let Ok(json) = serde_json::from_str::<Value>(data) {
                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                        accumulated.push_str(content);
                        token_count += 1;
                        emit_stream_chunk(app_handle, event_name, session_id, content, token_count);
                    }
                }
            }
            Ok(Some(Err(e))) => return Err(format!("流式读取失败: {}", e)),
            Ok(None) => return Ok(accumulated),
            Err(_) => {
                return Err(format!(
                    "流式读取超时（{} 秒无数据）",
                    STREAM_EVENT_TIMEOUT_SECS
                ))
            }
        }
    }
}

pub async fn read_anthropic_sse_stream(
    response: reqwest::Response,
    app_handle: &tauri::AppHandle,
    event_name: Option<&str>,
    session_id: Option<u64>,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut output_tokens: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let mut stream = response.bytes_stream().eventsource();

    loop {
        match tokio::time::timeout(event_timeout, stream.next()).await {
            Ok(Some(Ok(event))) => match event.event.as_str() {
                "message_start" | "message_delta" => {
                    if let Ok(json) = serde_json::from_str::<Value>(&event.data) {
                        if let Some(tokens) = anthropic_output_tokens(&json) {
                            output_tokens = tokens;
                            emit_stream_tokens(app_handle, event_name, session_id, output_tokens);
                        }
                    }
                }
                "content_block_delta" => {
                    if let Ok(json) = serde_json::from_str::<Value>(&event.data) {
                        let delta_type = json["delta"]["type"].as_str();
                        if matches!(delta_type, Some("text_delta") | None) {
                            if let Some(text) = json["delta"]["text"].as_str() {
                                accumulated.push_str(text);
                                emit_stream_chunk(
                                    app_handle,
                                    event_name,
                                    session_id,
                                    text,
                                    output_tokens,
                                );
                            }
                        }
                    }
                }
                "ping" => {}
                "message_stop" => return Ok(accumulated),
                "error" => {
                    let message = serde_json::from_str::<Value>(&event.data)
                        .ok()
                        .and_then(|json| json["error"]["message"].as_str().map(String::from))
                        .unwrap_or_else(|| event.data.clone());
                    return Err(format!("Anthropic 流式错误: {}", message));
                }
                _ => {}
            },
            Ok(Some(Err(e))) => return Err(format!("流式读取失败: {}", e)),
            Ok(None) => return Ok(accumulated),
            Err(_) => {
                return Err(format!(
                    "流式读取超时（{} 秒无数据）",
                    STREAM_EVENT_TIMEOUT_SECS
                ))
            }
        }
    }
}

fn extract_content(endpoint: &LlmEndpoint, json: &Value) -> Option<String> {
    match endpoint.api_format {
        ApiFormat::Anthropic => json["content"]
            .as_array()
            .and_then(|items| items.iter().find_map(|item| item["text"].as_str().map(String::from))),
        ApiFormat::OpenaiCompat => {
            if endpoint.api_url.contains("/v1/responses") {
                json["output"].as_array().and_then(|outputs| {
                    outputs.iter().find_map(|item| {
                        if item["type"].as_str() == Some("message") {
                            item["content"][0]["text"].as_str().map(String::from)
                        } else {
                            None
                        }
                    })
                })
            } else {
                json["choices"][0]["message"]["content"]
                    .as_str()
                    .map(String::from)
            }
        }
    }
}

pub async fn send_llm_request(
    http_client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
    body: &Value,
    text_len: usize,
    app_handle: Option<&tauri::AppHandle>,
    options: LlmRequestOptions<'_>,
) -> Result<String, String> {
    let headers = llm_provider::build_auth_headers(&endpoint.api_format, api_key)
        .map_err(|e| format!("构建请求头失败: {e}"))?;
    let timeout = dynamic_timeout(endpoint.timeout_secs, text_len);

    let mut request = http_client.post(&endpoint.api_url).headers(headers);
    if !(options.stream && !endpoint.api_url.contains("/v1/responses")) {
        request = request.timeout(timeout);
    }

    let response = request
        .json(body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let error_message = if endpoint.api_format == ApiFormat::Anthropic {
            serde_json::from_str::<Value>(&body_text)
                .ok()
                .and_then(|json| json["error"]["message"].as_str().map(String::from))
                .unwrap_or(body_text)
        } else {
            body_text
        };
        return Err(format!("API 返回错误 {}: {}", status, error_message));
    }

    if options.stream && !endpoint.api_url.contains("/v1/responses") {
        let app_handle = app_handle.ok_or_else(|| "流式请求缺少 app_handle".to_string())?;
        match endpoint.api_format {
            ApiFormat::Anthropic => {
                read_anthropic_sse_stream(
                    response,
                    app_handle,
                    options.stream_event,
                    options.session_id,
                )
                .await
            }
            ApiFormat::OpenaiCompat => {
                read_sse_stream(response, app_handle, options.stream_event, options.session_id).await
            }
        }
    } else {
        let json: Value = response
            .json()
            .await
            .map_err(|e| format!("响应解析失败: {}", e))?;
        Ok(extract_content(endpoint, &json).unwrap_or_default())
    }
}
