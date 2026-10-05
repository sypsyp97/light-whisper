use std::time::Duration;

use serde_json::Value;

use crate::services::llm_provider::LlmEndpoint;

use super::events::{emit_stream_citations, emit_stream_error_event, emit_stream_event};
use super::protocol::{
    ensure_non_empty_llm_content, extract_content, extract_openai_compat_error_message,
    finalize_responses_sse_accumulated,
};
use super::request::LlmRequestOptions;

const STREAM_EVENT_TIMEOUT_SECS: u64 = 90;
const STREAM_TOTAL_TIMEOUT_SECS: u64 = 24 * 60 * 60;
pub(crate) const AI_POLISH_STREAM_PROGRESS_TIMEOUT_SECS: u64 = 45;

pub(crate) fn stream_read_budget_at(
    now: tokio::time::Instant,
    started_at: tokio::time::Instant,
    last_progress_at: tokio::time::Instant,
    event_timeout: Duration,
    total_timeout: Duration,
    progress_timeout: Option<Duration>,
) -> Result<Duration, String> {
    let elapsed = now.duration_since(started_at);
    let remaining = total_timeout
        .checked_sub(elapsed)
        .ok_or_else(|| format!("流式读取超过总预算（{} 秒）", total_timeout.as_secs()))?;

    let mut budget = event_timeout.min(remaining);
    if let Some(progress_timeout) = progress_timeout {
        let progress_elapsed = now.duration_since(last_progress_at);
        let progress_remaining = progress_timeout
            .checked_sub(progress_elapsed)
            .ok_or_else(|| stream_progress_timeout_error(progress_timeout))?;
        budget = budget.min(progress_remaining);
    }

    Ok(budget)
}

fn stream_read_budget(
    started_at: tokio::time::Instant,
    last_progress_at: tokio::time::Instant,
    event_timeout: Duration,
    total_timeout: Duration,
    progress_timeout: Option<Duration>,
) -> Result<Duration, String> {
    stream_read_budget_at(
        tokio::time::Instant::now(),
        started_at,
        last_progress_at,
        event_timeout,
        total_timeout,
        progress_timeout,
    )
}

fn stream_progress_timeout_error(progress_timeout: Duration) -> String {
    format!(
        "流式输出停滞（{} 秒无新增内容）",
        progress_timeout.as_secs()
    )
}

fn stream_timeout_error(
    started_at: tokio::time::Instant,
    last_progress_at: tokio::time::Instant,
    total_timeout: Duration,
    progress_timeout: Option<Duration>,
) -> String {
    let now = tokio::time::Instant::now();
    if now.duration_since(started_at) >= total_timeout {
        format!("流式读取超过总预算（{} 秒）", total_timeout.as_secs())
    } else if let Some(progress_timeout) = progress_timeout {
        if now.duration_since(last_progress_at) >= progress_timeout {
            stream_progress_timeout_error(progress_timeout)
        } else {
            format!("流式读取超时（{} 秒无数据）", STREAM_EVENT_TIMEOUT_SECS)
        }
    } else {
        format!("流式读取超时（{} 秒无数据）", STREAM_EVENT_TIMEOUT_SECS)
    }
}

pub(crate) fn stream_progress_timeout(options: LlmRequestOptions<'_>) -> Option<Duration> {
    options
        .stream_progress_timeout_secs
        .map(Duration::from_secs)
}

pub(crate) fn stream_total_timeout(options: LlmRequestOptions<'_>) -> Duration {
    let secs = options
        .stream_total_timeout_secs
        .map(|value| value.min(600))
        .unwrap_or(STREAM_TOTAL_TIMEOUT_SECS);
    Duration::from_secs(secs)
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
    progress_timeout: Option<Duration>,
    total_timeout: Duration,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let started_at = tokio::time::Instant::now();
    let mut last_progress_at = started_at;
    let mut stream = response.bytes_stream().eventsource();

    loop {
        let read_budget = match stream_read_budget(
            started_at,
            last_progress_at,
            event_timeout,
            total_timeout,
            progress_timeout,
        ) {
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
                    return ensure_non_empty_llm_content(
                        accumulated,
                        endpoint,
                        "openai_chat_sse_done",
                    );
                }
                if let Ok(json) = serde_json::from_str::<Value>(data) {
                    emit_stream_citations(app_handle, event_name, session_id, &json);
                    if let Some(message) = json["error"]["message"].as_str() {
                        let message = format!("OpenAI 流式错误: {}", message);
                        emit_stream_error_event(app_handle, event_name, session_id, &message);
                        return Err(message);
                    }
                    if let Some(content) = json["choices"][0]["delta"]["content"]
                        .as_str()
                        .filter(|content| !content.is_empty())
                    {
                        accumulated.push_str(content);
                        token_count += 1;
                        last_progress_at = tokio::time::Instant::now();
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
            Ok(None) => {
                return ensure_non_empty_llm_content(accumulated, endpoint, "openai_chat_sse_eos")
            }
            Err(_) => {
                let message = stream_timeout_error(
                    started_at,
                    last_progress_at,
                    total_timeout,
                    progress_timeout,
                );
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn read_openai_responses_sse_stream(
    response: reqwest::Response,
    endpoint: &LlmEndpoint,
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
    progress_timeout: Option<Duration>,
    total_timeout: Duration,
    require_completed: bool,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut fallback_content: Option<String> = None;
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let started_at = tokio::time::Instant::now();
    let mut last_progress_at = started_at;
    let mut stream = response.bytes_stream().eventsource();
    let incomplete_error = || {
        let message =
            "CHATGPT_PLAN_ERROR: Responses 响应未完成，未收到 response.completed。".to_string();
        emit_stream_error_event(app_handle, event_name, session_id, &message);
        message
    };

    loop {
        let read_budget = match stream_read_budget(
            started_at,
            last_progress_at,
            event_timeout,
            total_timeout,
            progress_timeout,
        ) {
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
                    if require_completed {
                        return Err(incomplete_error());
                    }
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

                emit_stream_citations(app_handle, event_name, session_id, &json);

                if fallback_content.is_none() {
                    fallback_content = extract_content(endpoint, &json)
                        .or_else(|| extract_content(endpoint, &json["response"]));
                }

                match json["type"].as_str() {
                    Some("response.output_text.delta") => {
                        if let Some(delta) =
                            json["delta"].as_str().filter(|delta| !delta.is_empty())
                        {
                            accumulated.push_str(delta);
                            token_count += 1;
                            last_progress_at = tokio::time::Instant::now();
                            emit_stream_event(
                                app_handle,
                                event_name,
                                session_id,
                                Some(delta),
                                token_count,
                            );
                        }
                    }
                    Some("response.output_text.done") => {
                        if let Some(text) = json["text"].as_str() {
                            let (should_emit, progressed) =
                                apply_responses_done_text(&mut accumulated, text);
                            if progressed {
                                last_progress_at = tokio::time::Instant::now();
                            }
                            if should_emit {
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
                    }
                    Some("response.completed") => {
                        if require_completed
                            && json["response"]["status"].as_str() != Some("completed")
                        {
                            return Err(incomplete_error());
                        }
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
                    Some("response.failed") | Some("error") | Some("response.incomplete") => {
                        let response_error =
                            extract_openai_compat_error_message(&json["response"].to_string())
                                .or_else(|| extract_openai_compat_error_message(data));
                        let message = response_error.unwrap_or_else(|| {
                            let reason = json["response"]["incomplete_details"]["reason"].as_str();
                            reason.map_or_else(|| data.to_string(), |r| format!("响应未完成: {r}"))
                        });
                        let prefix = if require_completed {
                            "CHATGPT_PLAN_ERROR: "
                        } else {
                            ""
                        };
                        let message = format!("{prefix}Responses 流式错误: {message}");
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
                if require_completed {
                    return Err(incomplete_error());
                }
                return finalize_responses_sse_accumulated(
                    accumulated,
                    fallback_content,
                    endpoint,
                    "openai_responses_sse_eos",
                );
            }
            Err(_) => {
                let message = stream_timeout_error(
                    started_at,
                    last_progress_at,
                    total_timeout,
                    progress_timeout,
                );
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
    progress_timeout: Option<Duration>,
    total_timeout: Duration,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut output_tokens: usize = 0;
    let event_timeout = Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS);
    let started_at = tokio::time::Instant::now();
    let mut last_progress_at = started_at;
    let mut stream = response.bytes_stream().eventsource();

    loop {
        let read_budget = match stream_read_budget(
            started_at,
            last_progress_at,
            event_timeout,
            total_timeout,
            progress_timeout,
        ) {
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
                            if tokens > output_tokens {
                                output_tokens = tokens;
                                last_progress_at = tokio::time::Instant::now();
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
                }
                "content_block_delta" => {
                    if let Ok(json) = serde_json::from_str::<Value>(&event.data) {
                        let delta_type = json["delta"]["type"].as_str();
                        if matches!(delta_type, Some("text_delta") | None) {
                            if let Some(text) = json["delta"]["text"]
                                .as_str()
                                .filter(|text| !text.is_empty())
                            {
                                accumulated.push_str(text);
                                last_progress_at = tokio::time::Instant::now();
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
                "message_stop" => {
                    return ensure_non_empty_llm_content(
                        accumulated,
                        endpoint,
                        "anthropic_sse_message_stop",
                    )
                }
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
            Ok(None) => {
                return ensure_non_empty_llm_content(accumulated, endpoint, "anthropic_sse_eos")
            }
            Err(_) => {
                let message = stream_timeout_error(
                    started_at,
                    last_progress_at,
                    total_timeout,
                    progress_timeout,
                );
                emit_stream_error_event(app_handle, event_name, session_id, &message);
                return Err(message);
            }
        }
    }
}
pub(crate) fn apply_responses_done_text(accumulated: &mut String, text: &str) -> (bool, bool) {
    if text.is_empty() {
        return (false, false);
    }

    if accumulated.is_empty() {
        accumulated.push_str(text);
        return (true, true);
    }

    if text.len() > accumulated.len() && text.starts_with(accumulated.as_str()) {
        accumulated.clear();
        accumulated.push_str(text);
        return (false, true);
    }
    (false, false)
}
