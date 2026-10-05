use std::time::Duration;

use serde_json::Value;

use crate::services::codex_oauth_service;
use crate::services::grok_build_oauth_service;
use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::{ApiFormat, LlmReasoningMode};

use super::protocol::{
    adapt_body_for_backend, cached_output_token_limit_unsupported, ensure_non_empty_llm_content,
    extract_api_error_message, extract_content, has_output_token_limit,
    looks_like_output_token_limit_unsupported_error, remember_output_token_limit_unsupported,
    responses_api_url, strip_output_token_limits, uses_codex_chatgpt_backend,
    uses_deepseek_v4_responses, uses_grok_build_oauth_backend, uses_responses_api,
};
use super::request::LlmRequestOptions;
use super::stream::{
    read_anthropic_sse_stream, read_openai_responses_sse_stream, read_sse_stream,
    stream_progress_timeout, stream_total_timeout,
};

const RETRYABLE_429_DELAYS_MS: &[u64] = &[600, 1200];

pub(crate) fn dynamic_timeout(
    base_secs: u64,
    text_len: usize,
    body: &Value,
    web_search: bool,
) -> Duration {
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
pub(crate) fn is_retryable_overload_error(status: reqwest::StatusCode, message: &str) -> bool {
    if status != reqwest::StatusCode::TOO_MANY_REQUESTS {
        return false;
    }

    let normalized = message.to_ascii_lowercase();
    if normalized.contains("subscription_sharing_usage_limit_exceeded")
        || normalized.contains("insufficient_quota")
    {
        return false;
    }
    normalized.contains("queue_exceeded")
        || normalized.contains("high traffic")
        || normalized.contains("too many requests")
        || normalized.contains("rate limit")
}

pub(crate) fn request_url_for_backend<'a>(endpoint: &'a LlmEndpoint, api_key: &str) -> &'a str {
    if uses_codex_chatgpt_backend(endpoint, api_key) {
        codex_oauth_service::CHATGPT_CODEX_RESPONSES_URL
    } else if uses_grok_build_oauth_backend(endpoint, api_key) {
        grok_build_oauth_service::GROK_BUILD_RESPONSES_URL
    } else {
        endpoint.api_url.as_str()
    }
}

async fn read_error_body(
    response: reqwest::Response,
    deadline: tokio::time::Instant,
) -> Result<String, String> {
    tokio::time::timeout_at(deadline, response.text())
        .await
        .map_err(|_| "响应读取超时".to_string())?
        .map_err(|error| format!("响应读取失败: {error}"))
}

async fn dispatch_request(
    http_client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
    headers: reqwest::header::HeaderMap,
    body: &Value,
    deadline: tokio::time::Instant,
) -> Result<reqwest::Response, String> {
    let request_url = request_url_for_backend(endpoint, api_key);
    let request = http_client.post(request_url).headers(headers);
    tokio::time::timeout_at(deadline, request.json(body).send())
        .await
        .map_err(|_| "请求超时".to_string())?
        .map_err(|e| format!("请求失败: {}", e))
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_with_auth_recovery<F, Fut>(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    auth: &str,
    mut headers: reqwest::header::HeaderMap,
    body: &Value,
    deadline: tokio::time::Instant,
    session_id: Option<u64>,
    recover: F,
) -> Result<
    (
        reqwest::Response,
        Option<String>,
        reqwest::header::HeaderMap,
    ),
    String,
>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>, String>>,
{
    let mut response =
        dispatch_request(client, endpoint, auth, headers.clone(), body, deadline).await?;
    let replacement = if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        tokio::time::timeout_at(deadline, recover())
            .await
            .map_err(|_| "认证刷新超时")??
    } else {
        None
    };
    if let Some(auth) = replacement.as_deref() {
        headers = llm_provider::build_auth_headers(&endpoint.api_format, auth)?;
        if uses_codex_chatgpt_backend(endpoint, auth) {
            if let Some(id) = session_id {
                headers.insert(
                    "session_id",
                    id.to_string().parse().map_err(|_| "会话标识无效")?,
                );
            }
        }
        response =
            dispatch_request(client, endpoint, auth, headers.clone(), body, deadline).await?;
    }
    Ok((response, replacement, headers))
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
    let deepseek_responses_endpoint = uses_deepseek_v4_responses(endpoint).then(|| LlmEndpoint {
        provider: endpoint.provider.clone(),
        api_url: responses_api_url(&endpoint.api_url),
        model: endpoint.model.clone(),
        timeout_secs: endpoint.timeout_secs,
        api_format: endpoint.api_format.clone(),
    });
    let endpoint = deepseek_responses_endpoint.as_ref().unwrap_or(endpoint);
    let mut headers = llm_provider::build_auth_headers(&endpoint.api_format, api_key)
        .map_err(|e| format!("构建请求头失败: {e}"))?;
    if uses_grok_build_oauth_backend(endpoint, api_key) {
        if let Some(token) = grok_build_oauth_service::decode_grok_build_oauth_access_token(api_key)
        {
            headers = grok_build_oauth_service::grok_cli_request_headers(&token)
                .map_err(|e| format!("构建 Grok Build 请求头失败: {e}"))?;
        }
    }
    if uses_codex_chatgpt_backend(endpoint, api_key) {
        if let Some(session_id) = options.session_id {
            let header = session_id.to_string();
            if let Ok(value) = header.parse::<reqwest::header::HeaderValue>() {
                headers.insert("session_id", value);
            }
        }
    }
    let mut request_body =
        adapt_body_for_backend(endpoint, api_key, body, options.openai_fast_mode);
    if cached_output_token_limit_unsupported(endpoint) {
        strip_output_token_limits(&mut request_body);
    }
    let timeout = dynamic_timeout(
        endpoint.timeout_secs,
        text_len,
        &request_body,
        options.web_search,
    );
    let deadline = tokio::time::Instant::now() + timeout;
    let transport_stream = request_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let requested_stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let mut remember_initial_auto_reasoning_strategy = true;

    struct ReasoningRetryContext<'a> {
        http_client: &'a reqwest::Client,
        endpoint: &'a LlmEndpoint,
        api_key: &'a str,
        headers: &'a reqwest::header::HeaderMap,
        deadline: tokio::time::Instant,
        mode: LlmReasoningMode,
    }

    async fn retry_after_reasoning_rejection(
        ctx: &ReasoningRetryContext<'_>,
        base_body: &Value,
        initial_status: reqwest::StatusCode,
        mut error_message: String,
    ) -> Result<reqwest::Response, String> {
        let endpoint = ctx.endpoint;
        let is_responses_api = uses_responses_api(endpoint);
        if llm_provider::cached_auto_reasoning_strategy(endpoint, is_responses_api, ctx.mode)
            == Some(llm_provider::AutoReasoningStrategy::NoControls)
        {
            return Err(format!(
                "API 返回错误 {}: {}",
                initial_status, error_message
            ));
        }

        let mut output_token_limit_unsupported = false;
        for (strategy, mut fallback_body) in llm_provider::auto_reasoning_fallback_bodies(
            endpoint,
            is_responses_api,
            base_body,
            ctx.mode,
        ) {
            log::warn!(
                "当前模型拒绝推理参数，尝试自动探测策略 {}: provider={}, model={}, err={}",
                strategy.strategy_name(),
                endpoint.provider,
                endpoint.model,
                error_message
            );
            let retry_response = dispatch_request(
                ctx.http_client,
                endpoint,
                ctx.api_key,
                ctx.headers.clone(),
                &fallback_body,
                ctx.deadline,
            )
            .await?;
            if retry_response.status().is_success() {
                llm_provider::remember_auto_reasoning_strategy(
                    endpoint,
                    is_responses_api,
                    ctx.mode,
                    strategy,
                );
                return Ok(retry_response);
            }

            let mut status = retry_response.status();
            let mut body_text = read_error_body(retry_response, ctx.deadline).await?;
            error_message = extract_api_error_message(endpoint, &body_text);
            if looks_like_output_token_limit_unsupported_error(&error_message)
                && has_output_token_limit(&fallback_body)
            {
                output_token_limit_unsupported = true;
                log::warn!(
                    "当前后端不支持输出长度参数，已移除后继续推理参数探测: provider={}, model={}, err={}",
                    endpoint.provider,
                    endpoint.model,
                    error_message
                );
                strip_output_token_limits(&mut fallback_body);
                let retry_response = dispatch_request(
                    ctx.http_client,
                    endpoint,
                    ctx.api_key,
                    ctx.headers.clone(),
                    &fallback_body,
                    ctx.deadline,
                )
                .await?;
                if retry_response.status().is_success() {
                    remember_output_token_limit_unsupported(endpoint);
                    llm_provider::remember_auto_reasoning_strategy(
                        endpoint,
                        is_responses_api,
                        ctx.mode,
                        strategy,
                    );
                    return Ok(retry_response);
                }

                status = retry_response.status();
                body_text = read_error_body(retry_response, ctx.deadline).await?;
                error_message = extract_api_error_message(endpoint, &body_text);
            }
            if !llm_provider::looks_like_reasoning_unsupported_error(&error_message) {
                return Err(format!("API 返回错误 {}: {}", status, error_message));
            }
        }

        log::warn!(
            "当前模型不支持推理参数，已移除后自动重试: provider={}, model={}, err={}",
            endpoint.provider,
            endpoint.model,
            error_message
        );
        let mut fallback_body = base_body.clone();
        llm_provider::strip_reasoning_controls(&mut fallback_body);
        if output_token_limit_unsupported {
            strip_output_token_limits(&mut fallback_body);
        }
        let retry_response = dispatch_request(
            ctx.http_client,
            endpoint,
            ctx.api_key,
            ctx.headers.clone(),
            &fallback_body,
            ctx.deadline,
        )
        .await?;
        if !retry_response.status().is_success() {
            let mut status = retry_response.status();
            let mut body_text = read_error_body(retry_response, ctx.deadline).await?;
            let mut error_message = extract_api_error_message(endpoint, &body_text);
            if looks_like_output_token_limit_unsupported_error(&error_message)
                && has_output_token_limit(&fallback_body)
            {
                log::warn!(
                    "当前后端不支持输出长度参数，已移除后继续无推理参数重试: provider={}, model={}, err={}",
                    endpoint.provider,
                    endpoint.model,
                    error_message
                );
                strip_output_token_limits(&mut fallback_body);
                let retry_response = dispatch_request(
                    ctx.http_client,
                    endpoint,
                    ctx.api_key,
                    ctx.headers.clone(),
                    &fallback_body,
                    ctx.deadline,
                )
                .await?;
                if retry_response.status().is_success() {
                    remember_output_token_limit_unsupported(endpoint);
                    if llm_provider::is_auto_reasoning_endpoint(endpoint, is_responses_api) {
                        llm_provider::remember_auto_reasoning_strategy(
                            endpoint,
                            is_responses_api,
                            ctx.mode,
                            llm_provider::AutoReasoningStrategy::NoControls,
                        );
                    }
                    return Ok(retry_response);
                }
                status = retry_response.status();
                body_text = read_error_body(retry_response, ctx.deadline).await?;
                error_message = extract_api_error_message(endpoint, &body_text);
            }
            return Err(format!("API 返回错误 {}: {}", status, error_message));
        }
        if output_token_limit_unsupported {
            remember_output_token_limit_unsupported(endpoint);
        }
        if llm_provider::is_auto_reasoning_endpoint(endpoint, is_responses_api) {
            llm_provider::remember_auto_reasoning_strategy(
                endpoint,
                is_responses_api,
                ctx.mode,
                llm_provider::AutoReasoningStrategy::NoControls,
            );
        }
        Ok(retry_response)
    }

    let (mut response, recovered_auth, request_headers) = dispatch_with_auth_recovery(
        http_client,
        endpoint,
        api_key,
        headers,
        &request_body,
        deadline,
        options.session_id,
        || async {
            if let Some((app, state)) = options.auth_context {
                codex_oauth_service::recover_rejected_auth(app, state, &endpoint.provider, api_key)
                    .await
            } else {
                Ok(None)
            }
        },
    )
    .await?;
    headers = request_headers;
    let api_key = recovered_auth.as_deref().unwrap_or(api_key);

    if !response.status().is_success() {
        let mut status = response.status();
        let mut body_text = read_error_body(response, deadline).await?;
        let mut error_message = extract_api_error_message(endpoint, &body_text);
        let mut successful_retry: Option<reqwest::Response> = None;
        let reasoning_retry_context = ReasoningRetryContext {
            http_client,
            endpoint,
            api_key,
            headers: &headers,
            deadline,
            mode: options.reasoning_mode,
        };

        if is_retryable_overload_error(status, &error_message) {
            for delay_ms in RETRYABLE_429_DELAYS_MS {
                log::warn!(
                    "LLM 请求遇到可重试的 429，延迟 {}ms 后重试: provider={}, model={}, err={}",
                    delay_ms,
                    endpoint.provider,
                    endpoint.model,
                    error_message
                );
                tokio::time::timeout_at(
                    deadline,
                    tokio::time::sleep(Duration::from_millis(*delay_ms)),
                )
                .await
                .map_err(|_| "请求超时".to_string())?;
                let retry_response = dispatch_request(
                    http_client,
                    endpoint,
                    api_key,
                    headers.clone(),
                    &request_body,
                    deadline,
                )
                .await?;
                if retry_response.status().is_success() {
                    successful_retry = Some(retry_response);
                    break;
                }
                status = retry_response.status();
                let retry_body_text = read_error_body(retry_response, deadline).await?;
                error_message = extract_api_error_message(endpoint, &retry_body_text);
                if !is_retryable_overload_error(status, &error_message) {
                    break;
                }
            }
        }

        if let Some(retry_response) = successful_retry {
            response = retry_response;
        } else if looks_like_output_token_limit_unsupported_error(&error_message)
            && has_output_token_limit(&request_body)
        {
            log::warn!(
                "当前后端不支持输出长度参数，已移除后自动重试: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                error_message
            );
            let mut fallback_body = request_body.clone();
            strip_output_token_limits(&mut fallback_body);
            response = dispatch_request(
                http_client,
                endpoint,
                api_key,
                headers.clone(),
                &fallback_body,
                deadline,
            )
            .await?;
            if !response.status().is_success() {
                status = response.status();
                body_text = read_error_body(response, deadline).await?;
                error_message = extract_api_error_message(endpoint, &body_text);
                if options.reasoning_mode != LlmReasoningMode::ProviderDefault
                    && llm_provider::looks_like_reasoning_unsupported_error(&error_message)
                {
                    remember_initial_auto_reasoning_strategy = false;
                    response = retry_after_reasoning_rejection(
                        &reasoning_retry_context,
                        &fallback_body,
                        status,
                        error_message,
                    )
                    .await?;
                    remember_output_token_limit_unsupported(endpoint);
                } else {
                    return Err(format!("API 返回错误 {}: {}", status, error_message));
                }
            } else {
                remember_output_token_limit_unsupported(endpoint);
            }
        } else if options.reasoning_mode != LlmReasoningMode::ProviderDefault
            && llm_provider::looks_like_reasoning_unsupported_error(&error_message)
        {
            remember_initial_auto_reasoning_strategy = false;
            response = retry_after_reasoning_rejection(
                &reasoning_retry_context,
                &request_body,
                status,
                error_message,
            )
            .await?;
        } else {
            return Err(format!("API 返回错误 {}: {}", status, error_message));
        }
    }

    if remember_initial_auto_reasoning_strategy
        && options.reasoning_mode != LlmReasoningMode::ProviderDefault
    {
        let is_responses_api = uses_responses_api(endpoint);
        if llm_provider::is_auto_reasoning_endpoint(endpoint, is_responses_api) {
            if let Some(strategy) = llm_provider::applied_auto_reasoning_strategy(&request_body) {
                llm_provider::remember_auto_reasoning_strategy(
                    endpoint,
                    is_responses_api,
                    options.reasoning_mode,
                    strategy,
                );
            }
        }
    }

    // 根据 body 中实际是否启用了 stream 来决定响应解析方式
    // （build_llm_body 可能因供应商限制而跳过 stream，如 Cerebras json_object 不兼容流式）
    if transport_stream {
        if requested_stream && app_handle.is_none() {
            return Err("流式请求缺少 app_handle".to_string());
        }
        let stream_app_handle = if requested_stream { app_handle } else { None };
        let progress_timeout = stream_progress_timeout(options);
        let total_timeout = stream_total_timeout(options);
        match endpoint.api_format {
            ApiFormat::Anthropic => {
                read_anthropic_sse_stream(
                    endpoint,
                    response,
                    stream_app_handle,
                    options.stream_event,
                    options.session_id,
                    progress_timeout,
                    total_timeout,
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
                        progress_timeout,
                        total_timeout,
                        codex_oauth_service::is_chatgpt_plan_auth(api_key),
                    )
                    .await
                } else {
                    read_sse_stream(
                        endpoint,
                        response,
                        stream_app_handle,
                        options.stream_event,
                        options.session_id,
                        progress_timeout,
                        total_timeout,
                    )
                    .await
                }
            }
        }
    } else {
        let json: Value = tokio::time::timeout_at(deadline, response.json())
            .await
            .map_err(|_| "响应读取超时".to_string())?
            .map_err(|e| format!("响应解析失败: {}", e))?;
        ensure_non_empty_llm_content(
            extract_content(endpoint, &json).unwrap_or_default(),
            endpoint,
            "non_stream",
        )
    }
}

#[cfg(test)]
mod auth_recovery_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn plan_auth(token: &str) -> String {
        use base64::Engine;
        format!(
            "openai-chatgpt-plan:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                serde_json::json!({"access_token":token,"account_id":"oaiapp_test"}).to_string()
            )
        )
    }

    #[tokio::test]
    async fn http_auth_recovery_retries_once_with_new_credentials_and_same_body() {
        for (first, second, count) in [(401, 200, 2), (401, 401, 2), (403, 200, 1), (402, 200, 1)] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = LlmEndpoint {
                provider: "openai".into(),
                api_url: format!("http://{}/responses", listener.local_addr().unwrap()),
                model: "test".into(),
                timeout_secs: 5,
                api_format: ApiFormat::OpenaiCompat,
            };
            let server = tokio::spawn(async move {
                let mut bodies = Vec::new();
                for i in 0..count {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    loop {
                        let mut buffer = [0; 1024];
                        let n = socket.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        request.extend_from_slice(&buffer[..n]);
                        if let Some(offset) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers =
                                String::from_utf8_lossy(&request[..offset]).to_ascii_lowercase();
                            let length: usize = headers
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length: "))
                                .unwrap()
                                .parse()
                                .unwrap();
                            if request.len() >= offset + 4 + length {
                                assert!(headers.contains(if i == 0 {
                                    "authorization: bearer stale"
                                } else {
                                    "authorization: bearer fresh"
                                }));
                                bodies.push(request[offset + 4..].to_vec());
                                break;
                            }
                        }
                    }
                    let status = if i == 0 { first } else { second };
                    socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes()).await.unwrap();
                }
                if bodies.len() == 2 {
                    assert_eq!(bodies[0], bodies[1]);
                }
            });
            let callbacks = Arc::new(AtomicUsize::new(0));
            let callback_count = callbacks.clone();
            let auth = plan_auth("stale");
            let headers =
                llm_provider::build_auth_headers(&ApiFormat::OpenaiCompat, &auth).unwrap();
            let (response, replacement, _) = dispatch_with_auth_recovery(
                &reqwest::Client::new(),
                &endpoint,
                &auth,
                headers,
                &serde_json::json!({"input":[{"role":"user","content":"same request"}]}),
                tokio::time::Instant::now() + Duration::from_secs(3),
                None,
                || async move {
                    callback_count.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(plan_auth("fresh")))
                },
            )
            .await
            .unwrap();
            assert_eq!(
                response.status().as_u16(),
                if count == 2 { second } else { first }
            );
            assert_eq!(callbacks.load(Ordering::SeqCst), usize::from(first == 401));
            assert_eq!(replacement.is_some(), first == 401);
            server.await.unwrap();
        }
    }
}
