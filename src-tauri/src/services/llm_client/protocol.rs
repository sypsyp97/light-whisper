use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::Value;

use crate::services::codex_oauth_service;
use crate::services::grok_build_oauth_service;
use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::ApiFormat;

static OUTPUT_TOKEN_LIMIT_UNSUPPORTED_CACHE: OnceLock<parking_lot::Mutex<HashSet<String>>> =
    OnceLock::new();

pub(crate) fn uses_codex_chatgpt_backend(endpoint: &LlmEndpoint, api_key: &str) -> bool {
    endpoint.provider == "openai"
        && !codex_oauth_service::is_chatgpt_plan_auth(api_key)
        && codex_oauth_service::decode_chatgpt_bearer_token(api_key).is_some()
}

pub(crate) fn uses_grok_build_oauth_backend(endpoint: &LlmEndpoint, api_key: &str) -> bool {
    endpoint.provider == "xai" && grok_build_oauth_service::is_grok_build_oauth_origin_auth(api_key)
}

fn uses_openai_oauth_origin_auth(endpoint: &LlmEndpoint, api_key: &str) -> bool {
    endpoint.provider == "openai" && codex_oauth_service::is_oauth_origin_auth(api_key)
}

pub(crate) fn uses_responses_api(endpoint: &LlmEndpoint) -> bool {
    llm_provider::endpoint_uses_responses_api(endpoint)
}

pub(crate) fn uses_deepseek_v4_responses(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat
        && endpoint.provider == "deepseek"
        && matches!(
            endpoint.model.as_str(),
            "deepseek-v4-flash" | "deepseek-v4-pro"
        )
}

pub(crate) fn request_uses_responses_api(endpoint: &LlmEndpoint) -> bool {
    uses_responses_api(endpoint) || uses_deepseek_v4_responses(endpoint)
}

pub(crate) fn responses_api_url(api_url: &str) -> String {
    let trimmed = api_url.trim_end_matches('/');
    if trimmed.to_ascii_lowercase().ends_with("/responses") {
        return trimmed.to_string();
    }
    let suffix = "/chat/completions";
    if trimmed.to_ascii_lowercase().ends_with(suffix) {
        format!("{}/responses", &trimmed[..trimmed.len() - suffix.len()])
    } else {
        format!("{trimmed}/responses")
    }
}

fn uses_openai_chat_completions_api(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat
        && endpoint.api_url.contains("/chat/completions")
        && llm_provider::is_openai_like_endpoint(endpoint)
}

pub(crate) fn chat_output_token_limit_key(endpoint: &LlmEndpoint) -> &'static str {
    if uses_openai_chat_completions_api(endpoint)
        || llm_provider::is_cerebras_like_endpoint(endpoint)
    {
        "max_completion_tokens"
    } else {
        "max_tokens"
    }
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

pub(crate) fn adapt_body_for_backend(
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

    let uses_plan =
        endpoint.provider == "openai" && codex_oauth_service::is_chatgpt_plan_auth(api_key);
    if uses_plan {
        if let Some(map) = adapted.as_object_mut() {
            for key in [
                "background",
                "conversation",
                "max_output_tokens",
                "max_tool_calls",
                "metadata",
                "moderation",
                "multi_agent",
                "prompt",
                "prompt_cache_retention",
                "safety_identifier",
                "temperature",
                "top_logprobs",
                "top_p",
                "truncation",
                "user",
                "previous_response_id",
                "max_tokens",
                "max_completion_tokens",
            ] {
                map.remove(key);
            }
        }
    }
    if uses_chatgpt_backend {
        // The ChatGPT Codex backend rejects this Responses API field, so avoid
        // a guaranteed failed first request before the compatibility retry.
        strip_output_token_limits(&mut adapted);
    }

    if let Some(map) = adapted.as_object_mut() {
        if uses_chatgpt_backend || uses_plan {
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

pub(crate) fn looks_like_output_token_limit_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_output_limit = normalized.contains("max_output_tokens")
        || normalized.contains("max_tokens")
        || normalized.contains("max completion tokens")
        || normalized.contains("max_completion_tokens")
        || normalized.contains("maximum output tokens");

    mentions_output_limit
        && (normalized.contains("unsupported")
            || normalized.contains("not supported")
            || normalized.contains("unknown parameter")
            || normalized.contains("unrecognized parameter")
            || normalized.contains("not recognized"))
}

pub(crate) fn strip_output_token_limits(body: &mut Value) {
    if let Some(map) = body.as_object_mut() {
        map.remove("max_output_tokens");
        map.remove("max_completion_tokens");
        map.remove("max_tokens");
    }
}

pub(crate) fn has_output_token_limit(body: &Value) -> bool {
    body.get("max_output_tokens").is_some()
        || body.get("max_completion_tokens").is_some()
        || body.get("max_tokens").is_some()
}

fn output_token_limit_unsupported_cache_key(endpoint: &LlmEndpoint) -> String {
    format!(
        "{:?}|{}|{}",
        endpoint.api_format,
        endpoint.api_url,
        endpoint.model.trim().to_ascii_lowercase()
    )
}

fn output_token_limit_unsupported_cache() -> &'static parking_lot::Mutex<HashSet<String>> {
    OUTPUT_TOKEN_LIMIT_UNSUPPORTED_CACHE.get_or_init(|| parking_lot::Mutex::new(HashSet::new()))
}

pub(crate) fn cached_output_token_limit_unsupported(endpoint: &LlmEndpoint) -> bool {
    output_token_limit_unsupported_cache()
        .lock()
        .contains(&output_token_limit_unsupported_cache_key(endpoint))
}

pub(crate) fn remember_output_token_limit_unsupported(endpoint: &LlmEndpoint) {
    output_token_limit_unsupported_cache()
        .lock()
        .insert(output_token_limit_unsupported_cache_key(endpoint));
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
pub(crate) fn extract_content(endpoint: &LlmEndpoint, json: &Value) -> Option<String> {
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
                        (item["type"].as_str() == Some("message")).then_some(())?;
                        item["content"].as_array()?.iter().find_map(|part| {
                            if part["type"].as_str() == Some("output_text") {
                                part["text"].as_str().map(String::from)
                            } else {
                                None
                            }
                        })
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

pub(crate) fn extract_openai_compat_error_message(body_text: &str) -> Option<String> {
    let json = serde_json::from_str::<Value>(body_text).ok()?;
    let error = json.get("error").unwrap_or(&json);

    let message = error["message"]
        .as_str()
        .or_else(|| json["detail"].as_str())
        .or_else(|| json["message"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let guidance = match error["code"].as_str() {
        Some("subscription_sharing_usage_limit_exceeded") => {
            Some("轻语的 ChatGPT 套餐额度已达上限，请在 ChatGPT 设置的 Usage 页面查看额度。")
        }
        Some("subscription_sharing_user_not_eligible") => {
            Some("当前 ChatGPT 账户或工作区未获准使用套餐接入，请检查账户与工作区权限。")
        }
        Some("subscription_sharing_usage_unavailable") => {
            Some("暂时无法检查 ChatGPT 套餐额度，请稍后重试。")
        }
        _ => None,
    };
    let message = guidance.unwrap_or(message);

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

/// Terminal auth/policy/quota failures cannot be recovered by changing transport.
pub(crate) fn is_terminal_provider_error(error: &str) -> bool {
    error.contains("CHATGPT_PLAN_ERROR:")
        || error.contains("API 返回错误 401")
        || error.contains("API 返回错误 402")
        || error.contains("API 返回错误 403")
        || error.contains("subscription_sharing_usage_limit_exceeded")
        || error.contains("subscription_sharing_user_not_eligible")
        || error.contains("CHATGPT_REAUTH_REQUIRED")
        || error.contains("CODEX_REAUTH_REQUIRED")
        || error.contains("认证路线已更新")
        || error.contains("账户已切换")
        || error.contains("已退出登录")
        || error.contains("登录已失效")
        || error.contains("套餐使用未获授权")
}

pub(crate) fn extract_api_error_message(endpoint: &LlmEndpoint, body_text: &str) -> String {
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
