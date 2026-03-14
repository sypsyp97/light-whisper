use std::time::Duration;

use serde_json::Value;
use tauri::Emitter;

use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::{ApiFormat, LlmReasoningMode};

const STREAM_EVENT_TIMEOUT_SECS: u64 = 90;
const RETRYABLE_429_DELAYS_MS: &[u64] = &[600, 1200];

#[derive(Debug, Clone, Copy)]
pub struct LlmRequestOptions<'a> {
    pub stream: bool,
    pub json_output: bool,
    pub reasoning_mode: LlmReasoningMode,
    pub stream_event: Option<&'a str>,
    pub session_id: Option<u64>,
}

impl Default for LlmRequestOptions<'_> {
    fn default() -> Self {
        Self {
            stream: false,
            json_output: false,
            reasoning_mode: LlmReasoningMode::ProviderDefault,
            stream_event: None,
            session_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmImageInput {
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone)]
pub struct LlmUserInput {
    pub text: String,
    pub images: Vec<LlmImageInput>,
}

impl From<&str> for LlmUserInput {
    fn from(value: &str) -> Self {
        Self {
            text: value.to_string(),
            images: Vec::new(),
        }
    }
}

fn dynamic_timeout(base_secs: u64, text_len: usize) -> Duration {
    let extra = (text_len / 200) as u64;
    Duration::from_secs(base_secs.saturating_add(extra).min(120))
}

fn uses_responses_api(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat && endpoint.api_url.contains("/v1/responses")
}

pub fn build_llm_body(
    endpoint: &LlmEndpoint,
    system_prompt: &str,
    user_input: &LlmUserInput,
    options: LlmRequestOptions<'_>,
) -> Value {
    match endpoint.api_format {
        ApiFormat::Anthropic => serde_json::json!({
            "model": endpoint.model,
            "max_tokens": 4096,
            "system": [{"type": "text", "text": system_prompt, "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": anthropic_user_content(user_input)}],
            "stream": options.stream,
        }),
        ApiFormat::OpenaiCompat => {
            let is_responses_api = uses_responses_api(endpoint);

            let mut body = if is_responses_api {
                serde_json::json!({
                    "model": endpoint.model,
                    "instructions": system_prompt,
                    "input": [
                        {"role": "developer", "content": [{"type": "input_text", "text": if options.json_output { "Output json." } else { "Follow the system instructions exactly." }}]},
                        {"role": "user", "content": openai_responses_user_content(user_input)},
                    ],
                })
            } else {
                serde_json::json!({
                    "model": endpoint.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": openai_chat_user_content(user_input)},
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

            llm_provider::apply_reasoning_controls(
                endpoint,
                is_responses_api,
                &mut body,
                options.reasoning_mode,
            );

            if options.stream {
                body["stream"] = serde_json::json!(true);
            }

            body
        }
    }
}

fn openai_chat_user_content(user_input: &LlmUserInput) -> Value {
    if user_input.images.is_empty() {
        return serde_json::json!(user_input.text);
    }

    let mut content = vec![serde_json::json!({
        "type": "text",
        "text": user_input.text,
    })];
    for image in &user_input.images {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{};base64,{}", image.mime_type, image.data_base64),
            },
        }));
    }
    Value::Array(content)
}

fn openai_responses_user_content(user_input: &LlmUserInput) -> Value {
    let mut content = vec![serde_json::json!({
        "type": "input_text",
        "text": user_input.text,
    })];
    for image in &user_input.images {
        content.push(serde_json::json!({
            "type": "input_image",
            "image_url": format!("data:{};base64,{}", image.mime_type, image.data_base64),
        }));
    }
    Value::Array(content)
}

fn anthropic_user_content(user_input: &LlmUserInput) -> Value {
    if user_input.images.is_empty() {
        return serde_json::json!(user_input.text);
    }

    let mut content = vec![serde_json::json!({
        "type": "text",
        "text": user_input.text,
    })];
    for image in &user_input.images {
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime_type,
                "data": image.data_base64,
            },
        }));
    }
    Value::Array(content)
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

pub async fn read_openai_responses_sse_stream(
    response: reqwest::Response,
    endpoint: &LlmEndpoint,
    app_handle: &tauri::AppHandle,
    event_name: Option<&str>,
    session_id: Option<u64>,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut fallback_content: Option<String> = None;
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let mut stream = response.bytes_stream().eventsource();

    loop {
        match tokio::time::timeout(event_timeout, stream.next()).await {
            Ok(Some(Ok(event))) => {
                let data = event.data.trim();
                if data.is_empty() {
                    continue;
                }
                if data == "[DONE]" {
                    if !accumulated.is_empty() {
                        return Ok(accumulated);
                    }
                    if let Some(content) = fallback_content.filter(|content| !content.is_empty()) {
                        return Ok(content);
                    }
                    return Err("Responses 流式结束，但未收到可解析内容".to_string());
                }

                let Ok(json) = serde_json::from_str::<Value>(data) else {
                    continue;
                };

                if fallback_content.is_none() {
                    fallback_content = extract_content(endpoint, &json)
                        .or_else(|| extract_content(endpoint, &json["response"]));
                }

                match json["type"].as_str() {
                    Some("response.output_text.delta") => {
                        if let Some(delta) = json["delta"].as_str() {
                            accumulated.push_str(delta);
                            token_count += 1;
                            emit_stream_chunk(
                                app_handle,
                                event_name,
                                session_id,
                                delta,
                                token_count,
                            );
                        }
                    }
                    Some("response.output_text.done") if accumulated.is_empty() => {
                        if let Some(text) = json["text"].as_str() {
                            accumulated.push_str(text);
                            token_count += 1;
                            emit_stream_chunk(
                                app_handle,
                                event_name,
                                session_id,
                                text,
                                token_count,
                            );
                        }
                    }
                    Some("response.completed") => {
                        if accumulated.is_empty() {
                            accumulated = extract_content(endpoint, &json["response"])
                                .or_else(|| fallback_content.clone())
                                .unwrap_or_default();
                        }
                        return Ok(accumulated);
                    }
                    Some("response.failed") | Some("error") => {
                        let message = json["response"]["error"]["message"]
                            .as_str()
                            .or_else(|| json["error"]["message"].as_str())
                            .or_else(|| json["message"].as_str())
                            .unwrap_or(data);
                        return Err(format!("Responses 流式错误: {}", message));
                    }
                    _ => {}
                }
            }
            Ok(Some(Err(e))) => return Err(format!("流式读取失败: {}", e)),
            Ok(None) => {
                if !accumulated.is_empty() {
                    return Ok(accumulated);
                }
                if let Some(content) = fallback_content.filter(|content| !content.is_empty()) {
                    return Ok(content);
                }
                return Err("Responses 流式结束，但未收到可解析内容".to_string());
            }
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
        ApiFormat::Anthropic => json["content"].as_array().and_then(|items| {
            items
                .iter()
                .find_map(|item| item["text"].as_str().map(String::from))
        }),
        ApiFormat::OpenaiCompat => {
            if uses_responses_api(endpoint) {
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

fn extract_openai_compat_error_message(body_text: &str) -> Option<String> {
    let json = serde_json::from_str::<Value>(body_text).ok()?;
    let error = json.get("error").unwrap_or(&json);

    let message = error["message"]
        .as_str()
        .or_else(|| json["message"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let mut details = Vec::new();
    if let Some(code) = error["code"]
        .as_str()
        .or_else(|| json["code"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(format!("code: {}", code));
    }
    if let Some(param) = error["param"]
        .as_str()
        .or_else(|| json["param"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(format!("param: {}", param));
    }

    if details.is_empty() {
        Some(message.to_string())
    } else {
        Some(format!("{} ({})", message, details.join(", ")))
    }
}

fn extract_api_error_message(endpoint: &LlmEndpoint, body_text: &str) -> String {
    match endpoint.api_format {
        ApiFormat::Anthropic => serde_json::from_str::<Value>(body_text)
            .ok()
            .and_then(|json| json["error"]["message"].as_str().map(String::from))
            .unwrap_or_else(|| body_text.to_string()),
        ApiFormat::OpenaiCompat => {
            extract_openai_compat_error_message(body_text).unwrap_or_else(|| body_text.to_string())
        }
    }
}

fn is_retryable_overload_error(status: reqwest::StatusCode, message: &str) -> bool {
    if status != reqwest::StatusCode::TOO_MANY_REQUESTS {
        return false;
    }

    let normalized = message.to_ascii_lowercase();
    normalized.contains("queue_exceeded")
        || normalized.contains("high traffic")
        || normalized.contains("too many requests")
        || normalized.contains("rate limit")
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

    async fn dispatch_request(
        http_client: &reqwest::Client,
        endpoint: &LlmEndpoint,
        headers: reqwest::header::HeaderMap,
        body: &Value,
        timeout: Duration,
        stream: bool,
    ) -> Result<reqwest::Response, String> {
        let mut request = http_client.post(&endpoint.api_url).headers(headers);
        if !stream {
            request = request.timeout(timeout);
        }
        tokio::time::timeout(timeout, request.json(body).send())
            .await
            .map_err(|_| format!("请求超时（{} 秒）", timeout.as_secs()))?
            .map_err(|e| format!("请求失败: {}", e))
    }

    let mut response = dispatch_request(
        http_client,
        endpoint,
        headers.clone(),
        body,
        timeout,
        options.stream,
    )
    .await?;

    if !response.status().is_success() {
        let mut status = response.status();
        let mut body_text = response.text().await.unwrap_or_default();
        let mut error_message = extract_api_error_message(endpoint, &body_text);
        let mut successful_retry: Option<reqwest::Response> = None;

        if is_retryable_overload_error(status, &error_message) {
            for delay_ms in RETRYABLE_429_DELAYS_MS {
                log::warn!(
                    "LLM 请求遇到可重试的 429，延迟 {}ms 后重试: provider={}, model={}, err={}",
                    delay_ms,
                    endpoint.provider,
                    endpoint.model,
                    error_message
                );
                tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
                response = dispatch_request(
                    http_client,
                    endpoint,
                    headers.clone(),
                    body,
                    timeout,
                    options.stream,
                )
                .await?;
                if response.status().is_success() {
                    successful_retry = Some(response);
                    break;
                }
                status = response.status();
                let retry_body_text = response.text().await.unwrap_or_default();
                error_message = extract_api_error_message(endpoint, &retry_body_text);
                if !is_retryable_overload_error(status, &error_message) {
                    break;
                }
            }
        }

        if let Some(retry_response) = successful_retry {
            response = retry_response;
        } else if options.reasoning_mode != LlmReasoningMode::ProviderDefault
            && llm_provider::looks_like_reasoning_unsupported_error(&error_message)
        {
            log::warn!(
                "当前模型不支持推理参数，已移除后自动重试: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                error_message
            );
            let mut fallback_body = body.clone();
            llm_provider::strip_reasoning_controls(&mut fallback_body);
            response = dispatch_request(
                http_client,
                endpoint,
                headers,
                &fallback_body,
                timeout,
                options.stream,
            )
            .await?;
            if !response.status().is_success() {
                status = response.status();
                body_text = response.text().await.unwrap_or_default();
                error_message = extract_api_error_message(endpoint, &body_text);
                return Err(format!("API 返回错误 {}: {}", status, error_message));
            }
        } else {
            return Err(format!("API 返回错误 {}: {}", status, error_message));
        }
    }

    if options.stream {
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
                if uses_responses_api(endpoint) {
                    read_openai_responses_sse_stream(
                        response,
                        endpoint,
                        app_handle,
                        options.stream_event,
                        options.session_id,
                    )
                    .await
                } else {
                    read_sse_stream(
                        response,
                        app_handle,
                        options.stream_event,
                        options.session_id,
                    )
                    .await
                }
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

#[cfg(test)]
mod tests {
    use super::{
        build_llm_body, extract_api_error_message, extract_openai_compat_error_message,
        is_retryable_overload_error, LlmRequestOptions, LlmUserInput,
    };
    use crate::services::llm_provider::LlmEndpoint;
    use crate::state::user_profile::{ApiFormat, LlmReasoningMode};

    fn openai_endpoint(api_url: &str) -> LlmEndpoint {
        LlmEndpoint {
            provider: "openai".to_string(),
            api_url: api_url.to_string(),
            model: "gpt-4.1-mini".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::OpenaiCompat,
        }
    }

    #[test]
    fn responses_body_uses_stream_without_forcing_reasoning() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                stream: true,
                json_output: true,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
            },
        );

        assert_eq!(body["stream"], serde_json::json!(true));
        assert_eq!(
            body["text"]["format"]["type"],
            serde_json::json!("json_object")
        );
        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn chat_body_keeps_provider_default_reasoning() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                stream: true,
                json_output: false,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
            },
        );

        assert_eq!(body["stream"], serde_json::json!(true));
        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn volcengine_chat_body_maps_reasoning_mode_to_thinking() {
        let endpoint = LlmEndpoint {
            provider: "custom".to_string(),
            api_url: "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string(),
            model: "doubao-seed-1-6-thinking".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::OpenaiCompat,
        };
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                stream: false,
                json_output: false,
                reasoning_mode: LlmReasoningMode::Off,
                stream_event: None,
                session_id: None,
            },
        );

        assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn openai_chat_body_maps_reasoning_mode_to_effort() {
        let mut endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        endpoint.model = "gpt-5-mini".to_string();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                stream: false,
                json_output: false,
                reasoning_mode: LlmReasoningMode::Deep,
                stream_event: None,
                session_id: None,
            },
        );

        assert_eq!(body["reasoning_effort"], serde_json::json!("high"));
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn parses_openai_compat_top_level_error_message() {
        let message = extract_openai_compat_error_message(
            r#"{"message":"We're experiencing high traffic right now! Please try again soon.","type":"too_many_requests_error","param":"queue","code":"queue_exceeded"}"#,
        );

        assert_eq!(
            message.as_deref(),
            Some(
                "We're experiencing high traffic right now! Please try again soon. (code: queue_exceeded, param: queue)"
            )
        );
    }

    #[test]
    fn api_error_message_falls_back_to_openai_compat_parser() {
        let endpoint = openai_endpoint("https://api.cerebras.ai/v1/chat/completions");

        let message = extract_api_error_message(
            &endpoint,
            r#"{"error":{"message":"model does not support image input","code":"invalid_value"}}"#,
        );

        assert_eq!(
            message,
            "model does not support image input (code: invalid_value)"
        );
    }

    #[test]
    fn recognizes_retryable_queue_exceeded_errors() {
        assert!(is_retryable_overload_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "We're experiencing high traffic right now! Please try again soon. (code: queue_exceeded, param: queue)"
        ));
        assert!(!is_retryable_overload_error(
            reqwest::StatusCode::BAD_REQUEST,
            "queue_exceeded"
        ));
    }
}
