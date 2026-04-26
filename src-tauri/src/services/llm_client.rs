use std::time::Duration;

use serde_json::Value;
use tauri::{Emitter, Manager};

use crate::services::codex_oauth_service;
use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::{ApiFormat, LlmReasoningMode};
use crate::state::AppState;

const STREAM_EVENT_TIMEOUT_SECS: u64 = 90;
const STREAM_TOTAL_TIMEOUT_SECS: u64 = 24 * 60 * 60;
const RETRYABLE_429_DELAYS_MS: &[u64] = &[600, 1200];

#[derive(Debug, Clone, Copy)]
pub struct LlmRequestOptions<'a> {
    pub stream: bool,
    pub json_output: bool,
    pub reasoning_mode: LlmReasoningMode,
    pub stream_event: Option<&'a str>,
    pub session_id: Option<u64>,
    /// 注入模型厂商原生联网搜索工具（OpenAI web_search / Anthropic web_search）
    pub web_search: bool,
    /// OpenAI OAuth 快速模式：OAuth 来源认证时注入 service_tier="priority"
    /// (ChatGPT bearer 与交换得到的 OAuth API key 都适用；wire 值 "priority"
    /// 对应官方 Codex CLI 里 ServiceTier::Fast 的重映射)
    pub openai_fast_mode: bool,
}

impl Default for LlmRequestOptions<'_> {
    fn default() -> Self {
        Self {
            stream: false,
            json_output: false,
            reasoning_mode: LlmReasoningMode::ProviderDefault,
            stream_event: None,
            session_id: None,
            web_search: false,
            openai_fast_mode: false,
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

fn dynamic_timeout(base_secs: u64, text_len: usize, body: &Value, web_search: bool) -> Duration {
    let extra = (text_len / 200) as u64;
    let image_context_len = estimate_image_context_len(body);
    let image_extra = (image_context_len / (512 * 1024)) as u64 * 10;
    let tool_extra = if web_search { 45 } else { 0 };
    let total = base_secs
        .saturating_add(extra)
        .saturating_add(image_extra)
        .saturating_add(tool_extra);
    Duration::from_secs(total.min(base_secs.max(240)))
}

fn estimate_image_context_len(value: &Value) -> usize {
    fn visit(key: Option<&str>, value: &Value) -> usize {
        match value {
            Value::String(s) => match key {
                Some("image_url") if s.starts_with("data:image/") => s.len(),
                Some("url") if s.starts_with("data:image/") => s.len(),
                Some("data") if s.len() > 1024 => s.len(),
                _ => 0,
            },
            Value::Array(items) => items.iter().map(|item| visit(None, item)).sum(),
            Value::Object(map) => map
                .iter()
                .map(|(key, value)| visit(Some(key.as_str()), value))
                .sum(),
            _ => 0,
        }
    }

    visit(None, value)
}

fn uses_codex_chatgpt_backend(endpoint: &LlmEndpoint, api_key: &str) -> bool {
    endpoint.provider == "openai"
        && codex_oauth_service::decode_chatgpt_bearer_token(api_key).is_some()
}

fn uses_openai_oauth_origin_auth(endpoint: &LlmEndpoint, api_key: &str) -> bool {
    endpoint.provider == "openai" && codex_oauth_service::is_oauth_origin_auth(api_key)
}

fn uses_responses_api(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat && endpoint.api_url.contains("/v1/responses")
}

/// Wire value for OpenAI OAuth fast-mode priority processing.
///
/// Why "priority" and not "fast":
///   The user-facing label ("fast mode" / "快速模式") is the product name, but
///   the HTTP body value accepted by the OpenAI Responses API is "priority".
///   Official Codex CLI remaps `ServiceTier::Fast` → `"priority"` before
///   sending — see openai/codex `codex-rs/core/src/client.rs` (main, line
///   938-942 as of 2026-04-20):
///     Some(ServiceTier::Fast) => Some("priority".to_string()),
///
/// Accepted `service_tier` values per the current public OpenAI request docs are
/// `auto | default | flex | priority`. "fast" is NOT a valid wire
/// value; sending it causes the backend to silently ignore the field (this
/// was the pre-fix bug, matching openai/codex issue #14204).
pub(crate) const OPENAI_FAST_MODE_SERVICE_TIER: &str = "priority";

/// OpenAI Responses API `service_tier` wire-level whitelist. Used by tests
/// to independently verify that whatever we inject is actually a legal value
/// the backend will accept — not just whatever we happen to have written.
#[cfg(test)]
pub(crate) const OPENAI_RESPONSES_SERVICE_TIER_WHITELIST: &[&str] =
    &["auto", "default", "flex", "priority"];

fn adapt_body_for_backend(
    endpoint: &LlmEndpoint,
    api_key: &str,
    body: &Value,
    fast_mode: bool,
) -> Value {
    let mut adapted = body.clone();
    let uses_chatgpt_backend = uses_codex_chatgpt_backend(endpoint, api_key);
    if !uses_openai_oauth_origin_auth(endpoint, api_key) {
        return adapted;
    }

    if let Some(map) = adapted.as_object_mut() {
        if uses_chatgpt_backend {
            map.insert("store".to_string(), serde_json::json!(false));
            if uses_responses_api(endpoint) {
                map.insert("stream".to_string(), serde_json::json!(true));
            }
        }
        if fast_mode {
            map.insert(
                "service_tier".to_string(),
                serde_json::json!(OPENAI_FAST_MODE_SERVICE_TIER),
            );
        }
    }

    adapted
}

fn looks_like_max_output_tokens_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_output_limit = normalized.contains("max_output_tokens")
        || normalized.contains("max completion tokens")
        || normalized.contains("maximum output tokens");

    mentions_output_limit
        && (normalized.contains("unsupported")
            || normalized.contains("not supported")
            || normalized.contains("unknown parameter")
            || normalized.contains("invalid"))
}

fn strip_max_output_tokens(body: &mut Value) {
    if let Some(map) = body.as_object_mut() {
        map.remove("max_output_tokens");
    }
}

pub fn build_llm_body(
    endpoint: &LlmEndpoint,
    system_prompt: &str,
    user_input: &LlmUserInput,
    options: LlmRequestOptions<'_>,
) -> Value {
    let mut body = match endpoint.api_format {
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

            if is_responses_api {
                body["max_output_tokens"] = serde_json::json!(4096);
            } else {
                body["max_tokens"] = serde_json::json!(4096);
            }

            // Cerebras json_object 与 stream 不兼容：结构化输出优先，放弃流式
            if options.stream
                && !(options.json_output
                    && !is_responses_api
                    && llm_provider::is_cerebras_like_endpoint(endpoint))
            {
                body["stream"] = serde_json::json!(true);
            }

            if options.web_search {
                inject_openai_web_search(&mut body, is_responses_api);
            }

            body
        }
    };

    if options.web_search && endpoint.api_format == ApiFormat::Anthropic {
        inject_anthropic_web_search(&mut body);
    }

    body
}

/// OpenAI: chat completions 用 web_search_preview, responses API 用 web_search
fn inject_openai_web_search(body: &mut Value, is_responses_api: bool) {
    let tool = if is_responses_api {
        serde_json::json!({"type": "web_search"})
    } else {
        serde_json::json!({"type": "web_search_preview", "web_search_preview": {}})
    };
    match body.get_mut("tools") {
        Some(Value::Array(arr)) => arr.push(tool),
        _ => body["tools"] = serde_json::json!([tool]),
    }
}

/// Anthropic: web_search_20250305 工具
fn inject_anthropic_web_search(body: &mut Value) {
    let tool = serde_json::json!({
        "type": "web_search_20250305",
        "name": "web_search",
        "max_uses": 3,
    });
    match body.get_mut("tools") {
        Some(Value::Array(arr)) => arr.push(tool),
        _ => body["tools"] = serde_json::json!([tool]),
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

fn build_stream_event_payload(
    session_id: Option<u64>,
    chunk: Option<&str>,
    tokens: usize,
) -> Value {
    let mut payload = serde_json::json!({ "status": "streaming" });
    if let Some(chunk) = chunk {
        payload["chunk"] = serde_json::json!(chunk);
    }
    if tokens > 0 {
        payload["tokens"] = serde_json::json!(tokens);
    }
    if let Some(session_id) = session_id {
        payload["sessionId"] = serde_json::json!(session_id);
    }
    payload
}

fn emit_stream_event(
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
    chunk: Option<&str>,
    tokens: usize,
) {
    if let (Some(app_handle), Some(event_name)) = (app_handle, event_name) {
        if event_name == "ai-polish-status" && (chunk.is_some() || tokens > 0) {
            if let Some(session_id) = session_id {
                app_handle
                    .state::<AppState>()
                    .mark_ai_polish_stream_started(session_id);
            }
        }
        let payload = build_stream_event_payload(session_id, chunk, tokens);
        let _ = app_handle.emit(event_name, payload);
    }
}

fn emit_stream_error_event(
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
    message: &str,
) {
    if let (Some(app_handle), Some(event_name)) = (app_handle, event_name) {
        let mut payload = serde_json::json!({
            "status": "error",
            "message": message,
        });
        if let Some(session_id) = session_id {
            payload["sessionId"] = serde_json::json!(session_id);
        }
        let _ = app_handle.emit(event_name, payload);
    }
}

fn stream_read_budget(
    started_at: tokio::time::Instant,
    event_timeout: Duration,
    total_timeout: Duration,
) -> Result<Duration, String> {
    let elapsed = started_at.elapsed();
    let remaining = total_timeout
        .checked_sub(elapsed)
        .ok_or_else(|| format!("流式读取超过总预算（{} 秒）", total_timeout.as_secs()))?;
    Ok(event_timeout.min(remaining))
}

fn stream_timeout_error(started_at: tokio::time::Instant, total_timeout: Duration) -> String {
    if started_at.elapsed() >= total_timeout {
        format!("流式读取超过总预算（{} 秒）", total_timeout.as_secs())
    } else {
        format!("流式读取超时（{} 秒无数据）", STREAM_EVENT_TIMEOUT_SECS)
    }
}

fn anthropic_output_tokens(json: &Value) -> Option<usize> {
    json["usage"]["output_tokens"]
        .as_u64()
        .or_else(|| json["message"]["usage"]["output_tokens"].as_u64())
        .map(|value| value as usize)
}

pub async fn read_sse_stream(
    endpoint: &LlmEndpoint,
    response: reqwest::Response,
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let total_timeout = Duration::from_secs(STREAM_TOTAL_TIMEOUT_SECS);
    let started_at = tokio::time::Instant::now();
    let mut stream = response.bytes_stream().eventsource();

    loop {
        let read_budget = match stream_read_budget(started_at, event_timeout, total_timeout) {
            Ok(budget) => budget,
            Err(message) => {
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        };
        match tokio::time::timeout(read_budget, stream.next()).await {
            Ok(Some(Ok(event))) => {
                let data = event.data.trim();
                if data == "[DONE]" {
                    return ensure_non_empty_llm_content(accumulated, endpoint, "openai_chat_sse_done");
                }
                if let Ok(json) = serde_json::from_str::<Value>(data) {
                    if let Some(message) = json["error"]["message"].as_str() {
                        let message = format!("OpenAI 流式错误: {}", message);
                        emit_stream_error_event(app_handle, event_name, session_id, &message);
                        return Err(message);
                    }
                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                        accumulated.push_str(content);
                        token_count += 1;
                        emit_stream_event(
                            app_handle,
                            event_name,
                            session_id,
                            Some(content),
                            token_count,
                        );
                    }
                }
            }
            Ok(Some(Err(e))) => {
                let message = format!("流式读取失败: {}", e);
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
            Ok(None) => return ensure_non_empty_llm_content(accumulated, endpoint, "openai_chat_sse_eos"),
            Err(_) => {
                let message = stream_timeout_error(started_at, total_timeout);
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        }
    }
}

pub async fn read_openai_responses_sse_stream(
    response: reqwest::Response,
    endpoint: &LlmEndpoint,
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut fallback_content: Option<String> = None;
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let total_timeout = Duration::from_secs(STREAM_TOTAL_TIMEOUT_SECS);
    let started_at = tokio::time::Instant::now();
    let mut stream = response.bytes_stream().eventsource();

    loop {
        let read_budget = match stream_read_budget(started_at, event_timeout, total_timeout) {
            Ok(budget) => budget,
            Err(message) => {
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        };
        match tokio::time::timeout(read_budget, stream.next()).await {
            Ok(Some(Ok(event))) => {
                let data = event.data.trim();
                if data.is_empty() {
                    continue;
                }
                if data == "[DONE]" {
                    return finalize_responses_sse_accumulated(
                        accumulated,
                        fallback_content,
                        endpoint,
                        "openai_responses_sse_done",
                    );
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
                            emit_stream_event(
                                app_handle,
                                event_name,
                                session_id,
                                Some(delta),
                                token_count,
                            );
                        }
                    }
                    Some("response.output_text.done") if accumulated.is_empty() => {
                        if let Some(text) = json["text"].as_str() {
                            accumulated.push_str(text);
                            token_count += 1;
                            emit_stream_event(
                                app_handle,
                                event_name,
                                session_id,
                                Some(text),
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
                        return ensure_non_empty_llm_content(
                            accumulated,
                            endpoint,
                            "openai_responses_sse_completed",
                        );
                    }
                    Some("response.failed") | Some("error") => {
                        let message = json["response"]["error"]["message"]
                            .as_str()
                            .or_else(|| json["error"]["message"].as_str())
                            .or_else(|| json["message"].as_str())
                            .unwrap_or(data);
                        let message = format!("Responses 流式错误: {}", message);
                        emit_stream_error_event(app_handle, event_name, session_id, &message);
                        return Err(message);
                    }
                    _ => {}
                }
            }
            Ok(Some(Err(e))) => {
                let message = format!("流式读取失败: {}", e);
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
            Ok(None) => {
                return finalize_responses_sse_accumulated(
                    accumulated,
                    fallback_content,
                    endpoint,
                    "openai_responses_sse_eos",
                );
            }
            Err(_) => {
                let message = stream_timeout_error(started_at, total_timeout);
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        }
    }
}

pub async fn read_anthropic_sse_stream(
    endpoint: &LlmEndpoint,
    response: reqwest::Response,
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut output_tokens: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let total_timeout = Duration::from_secs(STREAM_TOTAL_TIMEOUT_SECS);
    let started_at = tokio::time::Instant::now();
    let mut stream = response.bytes_stream().eventsource();

    loop {
        let read_budget = match stream_read_budget(started_at, event_timeout, total_timeout) {
            Ok(budget) => budget,
            Err(message) => {
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        };
        match tokio::time::timeout(read_budget, stream.next()).await {
            Ok(Some(Ok(event))) => match event.event.as_str() {
                "message_start" | "message_delta" => {
                    if let Ok(json) = serde_json::from_str::<Value>(&event.data) {
                        if let Some(tokens) = anthropic_output_tokens(&json) {
                            output_tokens = tokens;
                            emit_stream_event(
                                app_handle,
                                event_name,
                                session_id,
                                None,
                                output_tokens,
                            );
                        }
                    }
                }
                "content_block_delta" => {
                    if let Ok(json) = serde_json::from_str::<Value>(&event.data) {
                        let delta_type = json["delta"]["type"].as_str();
                        if matches!(delta_type, Some("text_delta") | None) {
                            if let Some(text) = json["delta"]["text"].as_str() {
                                accumulated.push_str(text);
                                emit_stream_event(
                                    app_handle,
                                    event_name,
                                    session_id,
                                    Some(text),
                                    output_tokens,
                                );
                            }
                        }
                    }
                }
                "ping" => {}
                "message_stop" => return ensure_non_empty_llm_content(accumulated, endpoint, "anthropic_sse_message_stop"),
                "error" => {
                    let message = serde_json::from_str::<Value>(&event.data)
                        .ok()
                        .and_then(|json| json["error"]["message"].as_str().map(String::from))
                        .unwrap_or_else(|| event.data.clone());
                    let message = format!("Anthropic 流式错误: {}", message);
                    emit_stream_error_event(app_handle, event_name, session_id, &message);
                    return Err(message);
                }
                _ => {}
            },
            Ok(Some(Err(e))) => {
                let message = format!("流式读取失败: {}", e);
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
            Ok(None) => return ensure_non_empty_llm_content(accumulated, endpoint, "anthropic_sse_eos"),
            Err(_) => {
                let message = stream_timeout_error(started_at, total_timeout);
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        }
    }
}

/// `ensure_non_empty_llm_content` 产生的错误消息的稳定前缀。
/// 调用方（例如 polish 4 段 transport fallback）需要识别这条错误来决定
/// 是否短路重试——同一个 prompt 在另一个 transport stage 通常也会回空，
/// 没必要再花 3 次 LLM 请求。
pub(crate) const EMPTY_LLM_RESPONSE_ERROR_PREFIX: &str = "LLM 响应为空（";

/// 把"HTTP 成功但没产生任何可用文本"统一映射为错误，并把 provider/model
/// 信息塞到错误里便于诊断。`source` 用于区分调用栈
/// （例如 "non_stream"、"openai_chat_sse_done"、"openai_chat_sse_eos"、
/// "anthropic_sse_message_stop"、"anthropic_sse_eos"）。
///
/// 注：reasoning-only / tool-call-only 等"合法的空文本"响应不走这个路径。
/// OpenAI Responses SSE 在 `read_openai_responses_sse_stream` 中独立处理。
pub(crate) fn ensure_non_empty_llm_content(
    content: String,
    endpoint: &LlmEndpoint,
    source: &str,
) -> Result<String, String> {
    if content.trim().is_empty() {
        Err(format!(
            "{}{}）：provider={}，model={}",
            EMPTY_LLM_RESPONSE_ERROR_PREFIX, source, endpoint.provider, endpoint.model
        ))
    } else {
        Ok(content)
    }
}

/// 识别 `ensure_non_empty_llm_content` 产生的"空响应"错误。其它来源的错误
/// 字符串里只要不是手工拼接 EMPTY_LLM_RESPONSE_ERROR_PREFIX，就不会撞这个
/// 前缀（中文 + 全角括号的组合在错误信息里很罕见）。
pub(crate) fn is_empty_llm_response_error(err: &str) -> bool {
    err.starts_with(EMPTY_LLM_RESPONSE_ERROR_PREFIX)
}

/// 把 Responses SSE 流读到尾时的 "是否为空" 判定统一到一处。
/// 优先级：accumulated 非空 → fallback_content 非空 → 委托
/// `ensure_non_empty_llm_content` 让上层短路 transport fallback。
///
/// 注：这里沿用 [DONE] / Ok(None) / response.completed 三个分支原本的
/// `!is_empty()` 语义（不做 trim），避免改变现有行为；真正的 trim 检查由
/// 下游 `ensure_non_empty_llm_content` 在最终 Err 路径上负责。
pub(crate) fn finalize_responses_sse_accumulated(
    accumulated: String,
    fallback_content: Option<String>,
    endpoint: &LlmEndpoint,
    source: &str,
) -> Result<String, String> {
    if !accumulated.is_empty() {
        return Ok(accumulated);
    }
    if let Some(content) = fallback_content {
        if !content.is_empty() {
            return Ok(content);
        }
    }
    ensure_non_empty_llm_content(String::new(), endpoint, source)
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
    let mut headers = llm_provider::build_auth_headers(&endpoint.api_format, api_key)
        .map_err(|e| format!("构建请求头失败: {e}"))?;
    if uses_codex_chatgpt_backend(endpoint, api_key) {
        if let Some(session_id) = options.session_id {
            let header = session_id.to_string();
            if let Ok(value) = header.parse::<reqwest::header::HeaderValue>() {
                headers.insert("session_id", value);
            }
        }
    }
    let request_body = adapt_body_for_backend(endpoint, api_key, body, options.openai_fast_mode);
    let timeout = dynamic_timeout(
        endpoint.timeout_secs,
        text_len,
        &request_body,
        options.web_search,
    );
    let transport_stream = request_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let requested_stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    async fn dispatch_request(
        http_client: &reqwest::Client,
        endpoint: &LlmEndpoint,
        api_key: &str,
        headers: reqwest::header::HeaderMap,
        body: &Value,
        timeout: Duration,
    ) -> Result<reqwest::Response, String> {
        let request_url = if uses_codex_chatgpt_backend(endpoint, api_key) {
            codex_oauth_service::CHATGPT_CODEX_RESPONSES_URL
        } else {
            endpoint.api_url.as_str()
        };
        let request = http_client.post(request_url).headers(headers);
        tokio::time::timeout(timeout, request.json(body).send())
            .await
            .map_err(|_| format!("请求超时（{} 秒）", timeout.as_secs()))?
            .map_err(|e| format!("请求失败: {}", e))
    }

    let mut response = dispatch_request(
        http_client,
        endpoint,
        api_key,
        headers.clone(),
        &request_body,
        timeout,
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
                    api_key,
                    headers.clone(),
                    &request_body,
                    timeout,
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
        } else if looks_like_max_output_tokens_unsupported_error(&error_message) {
            log::warn!(
                "当前后端不支持 max_output_tokens，已移除后自动重试: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                error_message
            );
            let mut fallback_body = request_body.clone();
            strip_max_output_tokens(&mut fallback_body);
            response = dispatch_request(
                http_client,
                endpoint,
                api_key,
                headers,
                &fallback_body,
                timeout,
            )
            .await?;
            if !response.status().is_success() {
                status = response.status();
                body_text = response.text().await.unwrap_or_default();
                error_message = extract_api_error_message(endpoint, &body_text);
                return Err(format!("API 返回错误 {}: {}", status, error_message));
            }
        } else if options.reasoning_mode != LlmReasoningMode::ProviderDefault
            && llm_provider::looks_like_reasoning_unsupported_error(&error_message)
        {
            log::warn!(
                "当前模型不支持推理参数，已移除后自动重试: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                error_message
            );
            let mut fallback_body = request_body.clone();
            llm_provider::strip_reasoning_controls(&mut fallback_body);
            response = dispatch_request(
                http_client,
                endpoint,
                api_key,
                headers,
                &fallback_body,
                timeout,
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

    // 根据 body 中实际是否启用了 stream 来决定响应解析方式
    // （build_llm_body 可能因供应商限制而跳过 stream，如 Cerebras json_object 不兼容流式）
    if transport_stream {
        if requested_stream && app_handle.is_none() {
            return Err("流式请求缺少 app_handle".to_string());
        }
        let stream_app_handle = if requested_stream { app_handle } else { None };
        match endpoint.api_format {
            ApiFormat::Anthropic => {
                read_anthropic_sse_stream(
                    endpoint,
                    response,
                    stream_app_handle,
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
                        stream_app_handle,
                        options.stream_event,
                        options.session_id,
                    )
                    .await
                } else {
                    read_sse_stream(
                        endpoint,
                        response,
                        stream_app_handle,
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
        ensure_non_empty_llm_content(
            extract_content(endpoint, &json).unwrap_or_default(),
            endpoint,
            "non_stream",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        adapt_body_for_backend, build_llm_body, build_stream_event_payload, dynamic_timeout,
        ensure_non_empty_llm_content, extract_api_error_message,
        extract_openai_compat_error_message, finalize_responses_sse_accumulated,
        is_retryable_overload_error, looks_like_max_output_tokens_unsupported_error,
        LlmRequestOptions, LlmUserInput, OPENAI_RESPONSES_SERVICE_TIER_WHITELIST,
    };
    use crate::services::codex_oauth_service;
    use crate::services::llm_provider::LlmEndpoint;
    use crate::state::user_profile::{ApiFormat, LlmReasoningMode};
    use base64::Engine;

    fn make_test_endpoint() -> crate::services::llm_provider::LlmEndpoint {
        crate::services::llm_provider::LlmEndpoint {
            provider: "test-provider".to_string(),
            api_url: "https://test.example.com/v1/chat/completions".to_string(),
            model: "test-model-x".to_string(),
            timeout_secs: 10,
            api_format: crate::state::user_profile::ApiFormat::OpenaiCompat,
        }
    }

    fn openai_endpoint(api_url: &str) -> LlmEndpoint {
        LlmEndpoint {
            provider: "openai".to_string(),
            api_url: api_url.to_string(),
            model: "gpt-4.1-mini".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::OpenaiCompat,
        }
    }

    fn chatgpt_codex_api_key() -> String {
        format!(
            "openai-codex-chatgpt:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"access_token":"test","account_id":"acc"}"#)
        )
    }

    fn oauth_wrapped_api_key() -> String {
        codex_oauth_service::encode_oauth_api_key("sk-oauth-session").expect("wrapped key")
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
                web_search: false,
                openai_fast_mode: false,
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
                web_search: false,
                openai_fast_mode: false,
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
                web_search: false,
                openai_fast_mode: false,
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
                web_search: false,
                openai_fast_mode: false,
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

    #[test]
    fn stream_event_payload_omits_partial_text() {
        let payload = build_stream_event_payload(Some(7), Some("abc"), 3);

        assert_eq!(payload["status"], serde_json::json!("streaming"));
        assert_eq!(payload["chunk"], serde_json::json!("abc"));
        assert_eq!(payload["tokens"], serde_json::json!(3));
        assert_eq!(payload["sessionId"], serde_json::json!(7));
        assert!(payload.get("partialText").is_none());
    }

    #[test]
    fn timeout_budget_accounts_for_images_and_tool_context() {
        let plain = dynamic_timeout(10, 0, &serde_json::json!({}), false);
        let image_body = serde_json::json!({
            "messages": [{
                "content": [{
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:image/jpeg;base64,{}", "a".repeat(600_000))
                    }
                }]
            }]
        });
        let with_context = dynamic_timeout(10, 0, &image_body, true);

        assert!(with_context > plain);
    }

    #[test]
    fn stream_readers_have_total_budget_and_error_chunks() {
        let source = include_str!("llm_client.rs");

        assert!(
            source.contains("STREAM_TOTAL_TIMEOUT") || source.contains("total_stream_budget"),
            "SSE readers need a total stream budget in addition to per-event idle timeouts"
        );
        assert!(
            source.contains("\"status\": \"error\"") || source.contains("\"status\":\"error\""),
            "streaming failures and total-budget timeouts should emit an error chunk/status to the UI"
        );
    }

    #[test]
    fn chat_body_sets_max_tokens_for_openai_compat() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        assert_eq!(body["max_tokens"], serde_json::json!(4096));
    }

    #[test]
    fn responses_body_sets_max_output_tokens() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        assert_eq!(body["max_output_tokens"], serde_json::json!(4096));
    }

    #[test]
    fn chatgpt_backend_keeps_max_output_tokens_by_default() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );
        let api_key = format!(
            "openai-codex-chatgpt:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"access_token":"test","account_id":"acc"}"#)
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert_eq!(adapted["max_output_tokens"], serde_json::json!(4096));
        assert_eq!(adapted["store"], serde_json::json!(false));
    }

    #[test]
    fn chatgpt_backend_responses_json_output_forces_stream_transport() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                stream: false,
                json_output: true,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
            },
        );
        let api_key = chatgpt_codex_api_key();

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert_eq!(adapted["store"], serde_json::json!(false));
        assert_eq!(adapted["stream"], serde_json::json!(true));
    }

    #[test]
    fn chatgpt_backend_responses_gpt5_reasoning_off_forces_stream_transport() {
        let mut endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        endpoint.model = "gpt-5.1-mini".to_string();
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
                web_search: false,
                openai_fast_mode: false,
            },
        );
        let api_key = chatgpt_codex_api_key();

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert_eq!(adapted["store"], serde_json::json!(false));
        assert_eq!(adapted["reasoning"]["effort"], serde_json::json!("none"));
        assert_eq!(adapted["stream"], serde_json::json!(true));
    }

    #[test]
    fn recognizes_max_output_tokens_unsupported_errors() {
        assert!(looks_like_max_output_tokens_unsupported_error(
            "Unknown parameter: max_output_tokens"
        ));
        assert!(looks_like_max_output_tokens_unsupported_error(
            "max_output_tokens is not supported by this backend"
        ));
        assert!(!looks_like_max_output_tokens_unsupported_error(
            "request timed out after 30s"
        ));
    }

    #[test]
    fn cerebras_json_output_disables_stream_to_preserve_response_format() {
        let endpoint = LlmEndpoint {
            provider: "cerebras".to_string(),
            api_url: "https://api.cerebras.ai/v1/chat/completions".to_string(),
            model: "gpt-oss-120b".to_string(),
            timeout_secs: 5,
            api_format: ApiFormat::OpenaiCompat,
        };
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
                web_search: false,
                openai_fast_mode: false,
            },
        );

        // json_object 优先于 stream：保留 response_format，放弃流式
        assert_eq!(
            body["response_format"],
            serde_json::json!({"type": "json_object"})
        );
        assert!(!body
            .get("stream")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false));
    }

    #[test]
    fn cerebras_without_json_output_keeps_stream() {
        let endpoint = LlmEndpoint {
            provider: "cerebras".to_string(),
            api_url: "https://api.cerebras.ai/v1/chat/completions".to_string(),
            model: "gpt-oss-120b".to_string(),
            timeout_secs: 5,
            api_format: ApiFormat::OpenaiCompat,
        };
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
                web_search: false,
                openai_fast_mode: false,
            },
        );

        assert!(body.get("response_format").is_none());
        assert_eq!(body["stream"], serde_json::json!(true));
    }

    // --- Fast mode tests -------------------------------------------------
    //
    // Tests here deliberately do NOT import `OPENAI_FAST_MODE_SERVICE_TIER`.
    // The wire value is hard-coded as a string literal so that test and
    // implementation must agree *via* an independent, externally-anchored
    // ground truth — not via a shared constant that lets a typo slip through
    // both sides in lockstep (which was the pre-2026-04-20 tautological bug:
    // test and impl both said "fast", both went green, backend silently
    // ignored the field).
    //
    // Ground truth sources, anchored outside this repo:
    //   1. openai/codex `codex-rs/core/src/client.rs` main branch, function
    //      `build_responses_request` — maps `ServiceTier::Fast => "priority"`.
    //   2. OpenAI public request docs — `service_tier` accepts exactly
    //      `auto | default | flex | priority` (note: "fast" is NOT
    //      in this set; sending it is a silent no-op).
    //
    // Two independent assertions are used for this reason:
    //   - Literal-value test pins the current official wire value.
    //   - Whitelist test catches any future typo that still passes #1 (e.g.
    //      someone accidentally changes the impl to "priorty" or "urgent").

    #[test]
    fn chatgpt_backend_injects_service_tier_priority_when_fast_mode_enabled() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        // Hard-coded literal — NOT a re-export of OPENAI_FAST_MODE_SERVICE_TIER.
        // Changing the impl constant without updating this literal must fail.
        assert_eq!(
            adapted["service_tier"],
            serde_json::json!("priority"),
            "Fast mode must inject service_tier=\"priority\" on the wire. \
             Source: openai/codex codex-rs/core/src/client.rs (Fast → \"priority\"). \
             If this fails, either the official CLI changed its mapping or a \
             regression landed locally — do NOT weaken this test without \
             re-verifying against the upstream source."
        );
    }

    #[test]
    fn service_tier_whitelist_matches_openai_responses_api_spec() {
        // Meta-guard: lock the whitelist itself against drift. If someone
        // "fixes" both impl + literal test by also adding "fast" to the
        // whitelist to silence the tautology detector, this pin fails first.
        // Canonical set per OpenAI Responses API public spec.
        assert_eq!(
            OPENAI_RESPONSES_SERVICE_TIER_WHITELIST,
            &["auto", "default", "flex", "priority"],
            "Whitelist drifted. Reconfirm against the current OpenAI Responses \
             API spec before changing — this pin exists to prevent silent \
             widening (e.g. adding the product label \"fast\") that would \
             neutralize the other fast-mode assertions."
        );
        assert!(
            !OPENAI_RESPONSES_SERVICE_TIER_WHITELIST.contains(&"fast"),
            "\"fast\" is the product label, not a valid API value — it MUST \
             NOT appear in the whitelist under any circumstance."
        );
    }

    #[test]
    fn injected_service_tier_is_in_the_openai_responses_api_whitelist() {
        // Independent guard: whatever the impl injects, it must be a value
        // the OpenAI Responses API will actually accept. "fast" famously
        // passes a naive string compare but is not in the whitelist — so
        // this test catches typos / stale labels that the literal-equality
        // test alone would miss if someone "fixed" both sides the same way.
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        let injected = adapted["service_tier"]
            .as_str()
            .expect("service_tier must be injected as a JSON string when fast mode is on");

        assert!(
            OPENAI_RESPONSES_SERVICE_TIER_WHITELIST.contains(&injected),
            "Injected service_tier={injected:?} is NOT a valid OpenAI Responses \
             API value. Accepted values (per the public API spec): {:?}. \
             Note in particular: \"fast\" is the user-facing product label, \
             not a valid wire value — the backend silently drops it.",
            OPENAI_RESPONSES_SERVICE_TIER_WHITELIST
        );
    }

    #[test]
    fn chatgpt_backend_omits_service_tier_when_fast_mode_disabled() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert!(
            adapted.get("service_tier").is_none(),
            "service_tier must NOT be present when fast mode is disabled; got {:?}",
            adapted.get("service_tier")
        );
    }

    #[test]
    fn plain_openai_api_key_never_gets_service_tier_even_if_fast_mode_true() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = "sk-test";
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, api_key, &body, true);

        assert!(
            adapted.get("service_tier").is_none(),
            "Plain OpenAI API keys must never receive service_tier=fast; that header/body flag is ChatGPT-OAuth only"
        );
    }

    #[test]
    fn wrapped_oauth_api_key_gets_service_tier_without_chatgpt_backend_fields() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = oauth_wrapped_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        assert_eq!(adapted["service_tier"], serde_json::json!("priority"));
        assert!(
            adapted.get("store").is_none(),
            "Wrapped OAuth API keys must stay on the normal OpenAI endpoint path"
        );
    }

    #[test]
    fn chat_completions_chatgpt_backend_also_gets_service_tier_when_enabled() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        assert_eq!(
            adapted["service_tier"],
            serde_json::json!("priority"),
            "Fast mode is request-body scoped and must apply to chat/completions \
             too when ChatGPT-auth is used. Wire value hard-coded here to catch \
             divergence from openai/codex upstream — see the module-level \
             comment above these tests for why this literal is not a shared const."
        );
    }

    // --- ensure_non_empty_llm_content tests ------------------------------
    //
    // Contract:
    //   - Returns Err(...) when content.trim().is_empty()
    //   - Error message contains endpoint.provider, endpoint.model, and source
    //   - Otherwise returns Ok(content) — content is NOT trimmed

    #[test]
    fn ensure_non_empty_llm_content_rejects_empty_string() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content(String::new(), &endpoint, "polish");

        assert!(
            result.is_err(),
            "empty content must produce Err; got {:?}",
            result
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_rejects_whitespace_only() {
        let endpoint = make_test_endpoint();
        let result =
            ensure_non_empty_llm_content("   \n\t".to_string(), &endpoint, "polish");

        assert!(
            result.is_err(),
            "whitespace-only content must produce Err; got {:?}",
            result
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_passes_real_text() {
        let endpoint = make_test_endpoint();
        let result =
            ensure_non_empty_llm_content("hello world".to_string(), &endpoint, "polish");

        assert_eq!(
            result.as_deref(),
            Ok("hello world"),
            "real text must pass through verbatim, NOT trimmed"
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_preserves_internal_whitespace() {
        let endpoint = make_test_endpoint();
        let result =
            ensure_non_empty_llm_content("  hi  ".to_string(), &endpoint, "polish");

        assert_eq!(
            result.as_deref(),
            Ok("  hi  "),
            "leading/trailing whitespace must be preserved when content has \
             non-whitespace characters — the function checks emptiness via \
             trim() but must NOT mutate the returned string"
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_error_message_includes_provider_and_model() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content(String::new(), &endpoint, "polish");

        let err = result.expect_err("empty content must produce Err");
        assert!(
            err.contains("test-provider"),
            "error message must mention provider {:?}; got {:?}",
            endpoint.provider,
            err
        );
        assert!(
            err.contains("test-model-x"),
            "error message must mention model {:?}; got {:?}",
            endpoint.model,
            err
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_error_message_includes_source_label() {
        let endpoint = make_test_endpoint();
        let result =
            ensure_non_empty_llm_content("   ".to_string(), &endpoint, "ai-polish");

        let err = result.expect_err("whitespace-only content must produce Err");
        assert!(
            err.contains("ai-polish"),
            "error message must mention source label {:?} verbatim; got {:?}",
            "ai-polish",
            err
        );
    }

    // --- is_empty_llm_response_error tests --------------------------------
    //
    // 这条识别函数是 polish/assistant fallback 决定是否短路重试的依据。
    // 如果识别失稳（漏判 → 多花 3 次请求；误判 → 把无关错误当空响应丢弃），
    // 就退回到原始浪费/错失的状态。

    #[test]
    fn is_empty_llm_response_error_matches_fresh_helper_output() {
        let endpoint = make_test_endpoint();
        let err = ensure_non_empty_llm_content(String::new(), &endpoint, "non_stream")
            .expect_err("empty content must produce Err");
        assert!(
            super::is_empty_llm_response_error(&err),
            "recognizer must accept the very error string the helper produces; got {:?}",
            err
        );
    }

    #[test]
    fn is_empty_llm_response_error_matches_each_documented_source() {
        // 每个 callsite 都要被识别——任何一个漏掉就意味着那条路径上的空
        // 响应仍会触发 polish 4 段 fallback 全跑。
        let endpoint = make_test_endpoint();
        for source in [
            "non_stream",
            "openai_chat_sse_done",
            "openai_chat_sse_eos",
            "anthropic_sse_message_stop",
            "anthropic_sse_eos",
            "openai_responses_sse_completed",
            // 新增的 Responses SSE finalize 路径：done 事件结束流，
            // eos 是流意外终止后由 finalize 兜底。两条分支都必须被
            // recognizer 识别，否则 polish/assistant fallback 链路会
            // 误以为是真正的 transport 错误而连跑 4 段。
            "openai_responses_sse_done",
            "openai_responses_sse_eos",
        ] {
            let err = ensure_non_empty_llm_content(String::new(), &endpoint, source)
                .expect_err("empty content must produce Err");
            assert!(
                super::is_empty_llm_response_error(&err),
                "recognizer missed source={:?}; err={:?}",
                source,
                err
            );
        }
    }

    #[test]
    fn is_empty_llm_response_error_rejects_unrelated_errors() {
        // 未来加新错误时不应该误命中。这里也防止前缀被改成"空响应"等更通用
        // 的措辞导致与 anthropic / openai 自带的错误文案撞车。
        for unrelated in [
            "HTTP 500: internal server error",
            "流式读取失败: connection reset",
            "Anthropic 流式错误: rate_limit_exceeded",
            "Responses 流式错误: invalid_request",
            "API 返回错误 401: invalid api key",
            "响应解析失败: expected ident at column 5",
            "",
        ] {
            assert!(
                !super::is_empty_llm_response_error(unrelated),
                "recognizer must NOT match unrelated error; got match for {:?}",
                unrelated
            );
        }
    }

    #[test]
    fn is_empty_llm_response_error_prefix_constant_is_load_bearing() {
        // EMPTY_LLM_RESPONSE_ERROR_PREFIX 是 helper 与 recognizer 共用的契约
        // 字符串。把它改了就要同步改两边——这条断言是个 wake-up，提醒读者
        // 这个常量不是装饰品。
        assert_eq!(super::EMPTY_LLM_RESPONSE_ERROR_PREFIX, "LLM 响应为空（");
    }

    // --- finalize_responses_sse_accumulated tests --------------------------
    //
    // OpenAI Responses SSE 在 stream 结束（done）或意外断流（eos）时，
    // 我们手里同时握有：
    //   - accumulated：从 delta 事件里逐段拼起来的正文
    //   - fallback_content：completed/done 事件里附带的最终 content（可选）
    // finalizer 的合同是：
    //   1) accumulated 优先（含义最权威，且包含逐 token 流出的内容），
    //      非空就直接返回，且**不** trim——和 ensure_non_empty_llm_content
    //      对齐：只校验 trim 后是否为空，但返回原始字符串以保留前后空白。
    //   2) accumulated 为空才退回到事件里挂的 fallback_content；同样要求非空。
    //   3) 两者皆空再走 ensure_non_empty_llm_content，借它统一拼出
    //      EMPTY_LLM_RESPONSE_ERROR_PREFIX 起头的错误，让 recognizer 能识别。

    #[test]
    fn finalize_responses_sse_accumulated_returns_accumulated_when_non_empty() {
        // accumulated 存在就赢——即使 fallback 也非空也要被忽略。否则会出现
        // 流式内容被 done 事件的截断 fallback 覆盖，丢字。
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            "hello".to_string(),
            Some("ignored".to_string()),
            &endpoint,
            "openai_responses_sse_done",
        );

        assert_eq!(
            result.as_deref(),
            Ok("hello"),
            "accumulated 非空时必须直接返回 accumulated 原值，忽略 fallback"
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_preserves_accumulated_whitespace() {
        // 与 ensure_non_empty_llm_content 行为一致：判空用 trim，但返回原字符串。
        // 流式拼接出来的前后空白可能是有意义的（例如续写场景）。
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            "  hi  ".to_string(),
            None,
            &endpoint,
            "openai_responses_sse_done",
        );

        assert_eq!(
            result.as_deref(),
            Ok("  hi  "),
            "accumulated 必须 verbatim 返回，不允许被 trim 改写"
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_falls_back_when_accumulated_empty() {
        // 部分 provider 在 delta 里只丢 reasoning，正文只在 completed/done
        // 事件里给一次。accumulated 为空时必须用 fallback 兜底。
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            String::new(),
            Some("from_event".to_string()),
            &endpoint,
            "openai_responses_sse_done",
        );

        assert_eq!(
            result.as_deref(),
            Ok("from_event"),
            "accumulated 为空、fallback 非空时必须返回 fallback 原值"
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_ignores_empty_fallback() {
        // fallback 是 Some 但内容为空，等价于没有 fallback。要走空响应错误，
        // 而不是返回 Ok("")——后者会让上游误以为模型给了空字符串答案。
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            String::new(),
            Some(String::new()),
            &endpoint,
            "openai_responses_sse_done",
        );

        let err = result.expect_err("accumulated 与 fallback 都为空时必须 Err");
        assert!(
            super::is_empty_llm_response_error(&err),
            "错误必须能被 is_empty_llm_response_error 识别，否则 polish fallback 短路失效；got {:?}",
            err
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_ignores_none_fallback() {
        // 完全没有 fallback 也是常见场景（流被中断、只能用 eos 兜底）。
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            String::new(),
            None,
            &endpoint,
            "openai_responses_sse_eos",
        );

        let err = result.expect_err("accumulated 空、fallback None 时必须 Err");
        assert!(
            super::is_empty_llm_response_error(&err),
            "错误必须能被 is_empty_llm_response_error 识别；got {:?}",
            err
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_error_carries_provider_model_and_source() {
        // 错误信息要能在日志里直接定位是哪个 provider/model/调用点出问题的。
        // 这条断言把三段都钉死，避免有人把 source 标签替换成更"通用"的措辞
        // 而让 grep 失效。
        let endpoint = make_test_endpoint();
        let err = finalize_responses_sse_accumulated(
            String::new(),
            None,
            &endpoint,
            "openai_responses_sse_done",
        )
        .expect_err("空响应必须 Err");

        assert!(
            err.contains(&endpoint.provider),
            "错误必须包含 provider {:?}; got {:?}",
            endpoint.provider,
            err
        );
        assert!(
            err.contains(&endpoint.model),
            "错误必须包含 model {:?}; got {:?}",
            endpoint.model,
            err
        );
        assert!(
            err.contains("openai_responses_sse_done"),
            "错误必须包含 source 标签 verbatim，方便日志/grep 定位; got {:?}",
            err
        );
    }
}
