use super::llm_client::{self, build_llm_body, LlmRequestOptions, LlmUserInput};
use super::llm_provider::{self, AutoReasoningStrategy, LlmEndpoint};
use crate::state::user_profile::{ApiFormat, LlmReasoningMode};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn unknown_openai_compat_endpoint(provider: &str, api_url: &str, model: &str) -> LlmEndpoint {
    LlmEndpoint {
        provider: provider.to_string(),
        api_url: api_url.to_string(),
        model: model.to_string(),
        timeout_secs: 10,
        api_format: ApiFormat::OpenaiCompat,
    }
}

fn reasoning_options(mode: LlmReasoningMode) -> LlmRequestOptions<'static> {
    LlmRequestOptions {
        reasoning_mode: mode,
        ..LlmRequestOptions::default()
    }
}

fn assert_no_openai_reasoning_keys(body: &Value) {
    assert!(
        body.get("reasoning").is_none(),
        "chat-completions auto negotiation should not send Responses reasoning payload: {body}"
    );
    assert!(
        body.get("thinking").is_none(),
        "initial auto negotiation must start with generic OpenAI-compatible key before provider fallbacks: {body}"
    );
    assert!(
        body.get("chat_template_kwargs").is_none(),
        "initial auto negotiation must not jump to template-specific fallback: {body}"
    );
    assert!(
        body.get("enable_thinking").is_none() && body.get("thinking_budget").is_none(),
        "initial auto negotiation must not jump to enable_thinking fallback: {body}"
    );
    assert!(
        body.get("disable_reasoning").is_none(),
        "initial auto negotiation must not jump to disable_reasoning fallback: {body}"
    );
}

fn parse_header_value(headers: &str, name: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name
            .trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn read_json_request(stream: &mut TcpStream) -> Value {
    let mut request_bytes = Vec::new();
    let mut chunk = [0_u8; 1024];

    let headers_end = loop {
        let count = stream
            .read(&mut chunk)
            .await
            .expect("request should be readable");
        assert!(count > 0, "request ended before headers completed");
        request_bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = find_subsequence(&request_bytes, b"\r\n\r\n") {
            break position + 4;
        }
    };

    let headers = std::str::from_utf8(&request_bytes[..headers_end])
        .expect("request headers should be valid utf-8");
    let content_length = parse_header_value(headers, "Content-Length")
        .expect("request should include content length")
        .parse::<usize>()
        .expect("content length should be numeric");

    while request_bytes.len() < headers_end + content_length {
        let count = stream
            .read(&mut chunk)
            .await
            .expect("request body should be readable");
        assert!(count > 0, "request ended before body completed");
        request_bytes.extend_from_slice(&chunk[..count]);
    }

    serde_json::from_slice::<Value>(&request_bytes[headers_end..headers_end + content_length])
        .expect("request body should be valid json")
}

async fn write_json_response(stream: &mut TcpStream, status: &str, body: &[u8]) {
    let response_headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response_headers.as_bytes())
        .await
        .expect("response headers should be writable");
    stream
        .write_all(body)
        .await
        .expect("response body should be writable");
}

async fn spawn_reasoning_fallback_server() -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("fallback server should bind");
    let address = listener
        .local_addr()
        .expect("fallback server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..2 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            if request_index == 0 {
                write_json_response(
                    &mut stream,
                    "400 Bad Request",
                    br#"{"error":{"message":"reasoning_effort is unsupported","param":"reasoning_effort"}}"#,
                )
                .await;
            } else {
                write_json_response(
                    &mut stream,
                    "200 OK",
                    br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                )
                .await;
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_reasoning_three_step_server() -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("three-step server should bind");
    let address = listener
        .local_addr()
        .expect("three-step server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..3 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            match request_index {
                0 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning_effort is unsupported","param":"reasoning_effort"}}"#,
                    )
                    .await;
                }
                1 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"thinking is unsupported","param":"thinking"}}"#,
                    )
                    .await;
                }
                _ => {
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                    )
                    .await;
                }
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_no_controls_cache_second_request_server(
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("no-controls cache server should bind");
    let address = listener
        .local_addr()
        .expect("no-controls cache server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..4 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            match request_index {
                0 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning_effort is unsupported","param":"reasoning_effort"}}"#,
                    )
                    .await;
                }
                1 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"thinking is unsupported","param":"thinking"}}"#,
                    )
                    .await;
                }
                _ => {
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                    )
                    .await;
                }
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_fallback_non_reasoning_error_server() -> (String, tokio::task::JoinHandle<Vec<Value>>)
{
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("non-reasoning error server should bind");
    let address = listener
        .local_addr()
        .expect("non-reasoning error server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..2 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            if request_index == 0 {
                write_json_response(
                    &mut stream,
                    "400 Bad Request",
                    br#"{"error":{"message":"reasoning_effort is unsupported","param":"reasoning_effort"}}"#,
                )
                .await;
            } else {
                write_json_response(
                    &mut stream,
                    "401 Unauthorized",
                    br#"{"error":{"message":"invalid api key","code":"invalid_api_key"}}"#,
                )
                .await;
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_cached_strategy_rejection_server() -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("cached strategy rejection server should bind");
    let address = listener
        .local_addr()
        .expect("cached strategy rejection server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..2 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            if request_index == 0 {
                write_json_response(
                    &mut stream,
                    "400 Bad Request",
                    br#"{"error":{"message":"thinking is unsupported","param":"thinking"}}"#,
                )
                .await;
            } else {
                write_json_response(
                    &mut stream,
                    "200 OK",
                    br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                )
                .await;
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_cached_no_controls_error_server() -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("cached no-controls error server should bind");
    let address = listener
        .local_addr()
        .expect("cached no-controls error server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        let (mut stream, _) = listener.accept().await.expect("server should accept");
        let body = read_json_request(&mut stream).await;
        bodies.push(body);
        write_json_response(
            &mut stream,
            "400 Bad Request",
            br#"{"error":{"message":"reasoning is unsupported for this deployment","param":"reasoning"}}"#,
        )
        .await;
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_off_template_fallback_server() -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("off fallback server should bind");
    let address = listener
        .local_addr()
        .expect("off fallback server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..2 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            if request_index == 0 {
                write_json_response(
                    &mut stream,
                    "400 Bad Request",
                    br#"{"error":{"message":"thinking is unsupported","param":"thinking"}}"#,
                )
                .await;
            } else {
                write_json_response(
                    &mut stream,
                    "200 OK",
                    br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                )
                .await;
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_off_template_rejection_server() -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("off terminal fallback server should bind");
    let address = listener
        .local_addr()
        .expect("off terminal fallback server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..3 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            match request_index {
                0 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"thinking is unsupported","param":"thinking"}}"#,
                    )
                    .await;
                }
                1 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"Extra inputs are not permitted","param":"chat_template_kwargs"}}"#,
                    )
                    .await;
                }
                _ => {
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                    )
                    .await;
                }
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_responses_max_tokens_then_reasoning_server(
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("responses compatibility server should bind");
    let address = listener
        .local_addr()
        .expect("responses compatibility server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..4 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            match request_index {
                0 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"max_output_tokens is not supported","param":"max_output_tokens"}}"#,
                    )
                    .await;
                }
                1 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning is unsupported","param":"reasoning"}}"#,
                    )
                    .await;
                }
                2 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning_effort is unsupported","param":"reasoning_effort"}}"#,
                    )
                    .await;
                }
                _ => {
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
                    )
                    .await;
                }
            }
        }
        bodies
    });

    (format!("http://{address}/v1/responses"), handle)
}

async fn spawn_responses_combined_cache_server() -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("responses combined cache server should bind");
    let address = listener
        .local_addr()
        .expect("responses combined cache server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..5 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            match request_index {
                0 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"max_output_tokens is not supported","param":"max_output_tokens"}}"#,
                    )
                    .await;
                }
                1 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning is unsupported","param":"reasoning"}}"#,
                    )
                    .await;
                }
                2 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning_effort is unsupported","param":"reasoning_effort"}}"#,
                    )
                    .await;
                }
                _ => {
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
                    )
                    .await;
                }
            }
        }
        bodies
    });

    (format!("http://{address}/v1/responses"), handle)
}

async fn spawn_responses_max_tokens_cache_server() -> (String, tokio::task::JoinHandle<Vec<Value>>)
{
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("responses max-token cache server should bind");
    let address = listener
        .local_addr()
        .expect("responses max-token cache server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..3 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            if request_index == 0 {
                write_json_response(
                    &mut stream,
                    "400 Bad Request",
                    br#"{"error":{"message":"max_output_tokens is not supported","param":"max_output_tokens"}}"#,
                )
                .await;
            } else {
                write_json_response(
                    &mut stream,
                    "200 OK",
                    br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
                )
                .await;
            }
        }
        bodies
    });

    (format!("http://{address}/v1/responses"), handle)
}

async fn spawn_chat_max_completion_tokens_cache_server(
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("chat max-completion cache server should bind");
    let address = listener
        .local_addr()
        .expect("chat max-completion cache server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..3 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            if request_index == 0 {
                write_json_response(
                    &mut stream,
                    "400 Bad Request",
                    br#"{"error":{"message":"max completion tokens is not supported","param":"max_completion_tokens"}}"#,
                )
                .await;
            } else {
                write_json_response(
                    &mut stream,
                    "200 OK",
                    br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                )
                .await;
            }
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_token_limit_error_without_token_field_server(
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("token-limit guard server should bind");
    let address = listener
        .local_addr()
        .expect("token-limit guard server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        let (mut stream, _) = listener.accept().await.expect("server should accept");
        bodies.push(read_json_request(&mut stream).await);
        write_json_response(
            &mut stream,
            "400 Bad Request",
            br#"{"error":{"message":"max_tokens is not supported","param":"max_tokens"}}"#,
        )
        .await;

        if let Ok(Ok((mut stream, _))) =
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept()).await
        {
            bodies.push(read_json_request(&mut stream).await);
            write_json_response(
                &mut stream,
                "200 OK",
                br#"{"choices":[{"message":{"content":"unexpected retry"}}]}"#,
            )
            .await;
        }
        bodies
    });

    (format!("http://{address}/v1/chat/completions"), handle)
}

async fn spawn_responses_reasoning_then_max_tokens_server(
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("responses reverse compatibility server should bind");
    let address = listener
        .local_addr()
        .expect("responses reverse compatibility server should have a local address");

    let handle = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for request_index in 0..4 {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let body = read_json_request(&mut stream).await;
            bodies.push(body);

            match request_index {
                0 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning is unsupported","param":"reasoning"}}"#,
                    )
                    .await;
                }
                1 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"max_output_tokens is not supported","param":"max_output_tokens"}}"#,
                    )
                    .await;
                }
                2 => {
                    write_json_response(
                        &mut stream,
                        "400 Bad Request",
                        br#"{"error":{"message":"reasoning_effort is unsupported","param":"reasoning_effort"}}"#,
                    )
                    .await;
                }
                _ => {
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
                    )
                    .await;
                }
            }
        }
        bodies
    });

    (format!("http://{address}/v1/responses"), handle)
}

#[test]
fn unknown_openai_compatible_chat_endpoint_starts_with_generic_reasoning_effort() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "future-lab/arbitrary-reasoner-2030",
    );

    let support = llm_provider::reasoning_support(&endpoint, false);
    assert!(support.supported);
    assert_eq!(
        support.strategy.as_deref(),
        Some("auto_openai_compat_probe")
    );

    let body = build_llm_body(
        &endpoint,
        "system",
        &LlmUserInput::from("hello"),
        reasoning_options(LlmReasoningMode::Deep),
    );

    assert_eq!(body["reasoning_effort"], json!("high"));
    assert_no_openai_reasoning_keys(&body);
}

#[test]
fn unknown_openai_compatible_gpt5_proxy_still_uses_auto_reasoning() {
    let endpoint = unknown_openai_compat_endpoint(
        "future-gpt-proxy",
        "https://future-gateway.example/v1/chat/completions",
        "gpt-5.2",
    );

    let support = llm_provider::reasoning_support(&endpoint, false);
    assert_eq!(
        support.strategy.as_deref(),
        Some("auto_openai_compat_probe"),
        "unknown OpenAI-compatible GPT-shaped proxies still need runtime negotiation and caching"
    );

    let rejected_body = json!({
        "model": endpoint.model,
        "messages": [],
        "reasoning_effort": "high",
    });
    let fallbacks = llm_provider::auto_reasoning_fallback_bodies(
        &endpoint,
        false,
        &rejected_body,
        LlmReasoningMode::Deep,
    );

    assert_eq!(fallbacks.len(), 1);
    assert_eq!(fallbacks[0].0, AutoReasoningStrategy::TopLevelThinking);
}

#[test]
fn unknown_openai_compatible_chat_off_starts_with_boolean_thinking_disable() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-off",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "future-lab/arbitrary-reasoner-2030",
    );

    let body = build_llm_body(
        &endpoint,
        "system",
        &LlmUserInput::from("hello"),
        reasoning_options(LlmReasoningMode::Off),
    );

    assert_eq!(body["thinking"], json!({ "type": "disabled" }));
    assert!(
        body.get("reasoning_effort").is_none() && body.get("reasoning").is_none(),
        "Off should start with an actual boolean disable control, not an effort downgrade: {body}"
    );
}

#[test]
fn unknown_openai_compatible_responses_off_starts_with_boolean_disable() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-responses-off",
        "https://hub.nhr.fau.de/api/llmgw/v1/responses",
        "future-lab/arbitrary-reasoner-2030",
    );

    let body = build_llm_body(
        &endpoint,
        "system",
        &LlmUserInput::from("hello"),
        reasoning_options(LlmReasoningMode::Off),
    );

    assert_eq!(body["thinking"], json!({ "type": "disabled" }));
    assert!(
        body.get("reasoning").is_none() && body.get("reasoning_effort").is_none(),
        "Off must not be represented as low reasoning effort on unknown Responses endpoints: {body}"
    );
}

#[test]
fn unknown_openai_compatible_responses_endpoint_starts_with_reasoning_payload() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-responses",
        "https://hub.nhr.fau.de/api/llmgw/v1/responses",
        "future-lab/arbitrary-reasoner-2030",
    );

    let support = llm_provider::reasoning_support(&endpoint, true);
    assert!(support.supported);
    assert_eq!(
        support.strategy.as_deref(),
        Some("auto_openai_compat_probe")
    );

    let body = build_llm_body(
        &endpoint,
        "system",
        &LlmUserInput::from("hello"),
        reasoning_options(LlmReasoningMode::Balanced),
    );

    assert_eq!(body["reasoning"], json!({ "effort": "medium" }));
    assert!(
        body.get("reasoning_effort").is_none(),
        "Responses auto negotiation should use reasoning payload, got {body}"
    );
    assert!(
        body.get("thinking").is_none()
            && body.get("chat_template_kwargs").is_none()
            && body.get("enable_thinking").is_none()
            && body.get("disable_reasoning").is_none(),
        "Responses initial body must not skip ahead to fallback protocols: {body}"
    );
}

#[test]
fn unknown_openai_compatible_fallback_bodies_try_one_standard_alternate() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-fallbacks",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "future-lab/fallback-probe-model",
    );
    let rejected_body = json!({
        "model": endpoint.model,
        "messages": [],
        "reasoning_effort": "high",
    });

    let fallbacks = llm_provider::auto_reasoning_fallback_bodies(
        &endpoint,
        false,
        &rejected_body,
        LlmReasoningMode::Deep,
    );

    assert_eq!(fallbacks.len(), 1);
    assert_eq!(fallbacks[0].0, AutoReasoningStrategy::TopLevelThinking);
    assert_eq!(fallbacks[0].1["thinking"], json!({ "type": "enabled" }));
    assert!(
        fallbacks[0].1.get("reasoning_effort").is_none(),
        "fallback body must remove the rejected reasoning_effort key: {fallbacks:?}"
    );
    assert!(
        fallbacks[0].1.get("chat_template_kwargs").is_none()
            && fallbacks[0].1.get("enable_thinking").is_none()
            && fallbacks[0].1.get("disable_reasoning").is_none(),
        "speed-first negotiation should not spend extra retries on provider-specific extension keys: {fallbacks:?}"
    );
}

#[test]
fn unknown_openai_compatible_off_fallback_tries_template_thinking_disable_once() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-off-fallback",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "future-lab/off-fallback-model",
    );
    let rejected_body = json!({
        "model": endpoint.model,
        "messages": [],
        "thinking": { "type": "disabled" },
    });

    let fallbacks = llm_provider::auto_reasoning_fallback_bodies(
        &endpoint,
        false,
        &rejected_body,
        LlmReasoningMode::Off,
    );

    assert_eq!(fallbacks.len(), 1);
    assert_eq!(fallbacks[0].0, AutoReasoningStrategy::ChatTemplateThinking);
    assert_eq!(
        fallbacks[0].1["chat_template_kwargs"],
        json!({ "thinking": false })
    );
    assert!(
        fallbacks[0].1.get("thinking").is_none(),
        "fallback body must remove the rejected thinking control: {fallbacks:?}"
    );
}

#[test]
fn unknown_openai_compatible_responses_fallback_tries_chat_effort_once() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-responses-fallback",
        "https://hub.nhr.fau.de/api/llmgw/v1/responses",
        "future-lab/fallback-probe-model",
    );
    let rejected_body = json!({
        "model": endpoint.model,
        "input": [],
        "reasoning": { "effort": "medium" },
    });

    let fallbacks = llm_provider::auto_reasoning_fallback_bodies(
        &endpoint,
        true,
        &rejected_body,
        LlmReasoningMode::Balanced,
    );

    assert_eq!(fallbacks.len(), 1);
    assert_eq!(
        fallbacks[0].0,
        AutoReasoningStrategy::OpenaiChatReasoningEffort
    );
    assert_eq!(fallbacks[0].1["reasoning_effort"], json!("medium"));
    assert!(
        fallbacks[0].1.get("reasoning").is_none(),
        "fallback body must remove the rejected reasoning payload: {fallbacks:?}"
    );
}

#[tokio::test]
async fn send_llm_request_caches_successful_auto_fallback_strategy() {
    let (api_url, server) = spawn_reasoning_fallback_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "runtime-fallback-cache-provider",
        &api_url,
        "future-lab/runtime-cache-model",
    );
    let options = reasoning_options(LlmReasoningMode::Deep);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("fallback request should succeed against the local server");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(response, "ok");
    assert_eq!(captured_bodies.len(), 2);
    assert_eq!(captured_bodies[0]["reasoning_effort"], json!("high"));
    assert!(captured_bodies[0].get("reasoning").is_none());
    assert_eq!(captured_bodies[1]["thinking"], json!({ "type": "enabled" }));
    assert!(captured_bodies[1].get("reasoning_effort").is_none());
    assert_eq!(
        llm_provider::cached_auto_reasoning_strategy(&endpoint, false, LlmReasoningMode::Deep),
        Some(AutoReasoningStrategy::TopLevelThinking),
        "the successful fallback strategy must remain cached for the next request"
    );
}

#[tokio::test]
async fn fau_kimi_auto_reasoning_falls_back_to_no_controls_after_two_rejections() {
    let (api_url, server) = spawn_reasoning_three_step_server().await;
    let endpoint = unknown_openai_compat_endpoint("fau", &api_url, "moonshotai/Kimi-K2.6");
    let options = reasoning_options(LlmReasoningMode::Deep);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("fallback request should succeed against the local server");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(response, "ok");
    assert_eq!(captured_bodies.len(), 3);
    assert_eq!(captured_bodies[0]["reasoning_effort"], json!("high"));
    assert!(captured_bodies[0].get("reasoning").is_none());
    assert_eq!(captured_bodies[1]["thinking"], json!({ "type": "enabled" }));
    assert!(captured_bodies[1].get("reasoning").is_none());
    assert!(captured_bodies[1].get("reasoning_effort").is_none());
    assert!(captured_bodies[2].get("reasoning").is_none());
    assert!(captured_bodies[2].get("reasoning_effort").is_none());
    assert!(captured_bodies[2].get("thinking").is_none());
    assert_eq!(
        llm_provider::cached_auto_reasoning_strategy(&endpoint, false, LlmReasoningMode::Deep),
        Some(AutoReasoningStrategy::NoControls),
        "after the speed-first retries are exhausted, the endpoint should learn to send no reasoning controls"
    );
}

#[tokio::test]
async fn fau_kimi_no_controls_cache_makes_next_request_single_shot() {
    let (api_url, server) = spawn_no_controls_cache_second_request_server().await;
    let endpoint =
        unknown_openai_compat_endpoint("fau-no-controls-cache", &api_url, "moonshotai/Kimi-K2.6");
    let options = reasoning_options(LlmReasoningMode::Deep);
    let first_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let first_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &first_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("first request should learn the terminal no-controls strategy");
    assert_eq!(first_response, "ok");

    let second_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
    let second_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &second_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("cached no-controls strategy should make the second request succeed immediately");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(second_response, "ok");
    assert_eq!(captured_bodies.len(), 4);
    assert_eq!(captured_bodies[0]["reasoning_effort"], json!("high"));
    assert_eq!(captured_bodies[1]["thinking"], json!({ "type": "enabled" }));
    assert!(captured_bodies[2].get("reasoning_effort").is_none());
    assert!(captured_bodies[2].get("thinking").is_none());
    assert!(captured_bodies[3].get("reasoning").is_none());
    assert!(captured_bodies[3].get("reasoning_effort").is_none());
    assert!(captured_bodies[3].get("thinking").is_none());
    assert!(captured_bodies[3].get("chat_template_kwargs").is_none());
}

#[tokio::test]
async fn auto_reasoning_fallback_non_reasoning_error_does_not_strip_or_cache() {
    let (api_url, server) = spawn_fallback_non_reasoning_error_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "auto-non-reasoning-error",
        &api_url,
        "future-lab/non-reasoning-error-model",
    );
    let options = reasoning_options(LlmReasoningMode::Deep);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let error = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect_err("non-reasoning fallback errors must stop compatibility retries");

    let captured_bodies = server.await.expect("server task should complete");

    assert!(
        error.contains("invalid api key"),
        "unexpected error: {error}"
    );
    assert_eq!(captured_bodies.len(), 2);
    assert_eq!(captured_bodies[0]["reasoning_effort"], json!("high"));
    assert_eq!(captured_bodies[1]["thinking"], json!({ "type": "enabled" }));
    assert_eq!(
        llm_provider::cached_auto_reasoning_strategy(&endpoint, false, LlmReasoningMode::Deep),
        None,
        "auth/quota/server errors must not poison the reasoning strategy cache"
    );
}

#[tokio::test]
async fn cached_auto_fallback_rejection_does_not_retry_same_strategy() {
    let (api_url, server) = spawn_cached_strategy_rejection_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "cached-strategy-rejection",
        &api_url,
        "future-lab/cached-strategy-rejection-model",
    );
    llm_provider::remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::TopLevelThinking,
    );

    let options = reasoning_options(LlmReasoningMode::Deep);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("cached fallback rejection should go directly to no-controls");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(response, "ok");
    assert_eq!(captured_bodies.len(), 2);
    assert_eq!(captured_bodies[0]["thinking"], json!({ "type": "enabled" }));
    assert!(captured_bodies[1].get("thinking").is_none());
    assert!(captured_bodies[1].get("reasoning_effort").is_none());
    assert_eq!(
        llm_provider::cached_auto_reasoning_strategy(&endpoint, false, LlmReasoningMode::Deep),
        Some(AutoReasoningStrategy::NoControls)
    );
}

#[tokio::test]
async fn cached_no_controls_reasoning_error_does_not_retry_same_body() {
    let (api_url, server) = spawn_cached_no_controls_error_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "cached-no-controls-error",
        &api_url,
        "future-lab/cached-no-controls-error-model",
    );
    llm_provider::remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::NoControls,
    );

    let options = reasoning_options(LlmReasoningMode::Deep);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let error = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect_err("cached no-controls state should not retry an identical terminal request");

    let captured_bodies = server.await.expect("server task should complete");

    assert!(
        error.contains("reasoning is unsupported"),
        "unexpected error: {error}"
    );
    assert_eq!(captured_bodies.len(), 1);
    assert!(captured_bodies[0].get("reasoning").is_none());
    assert!(captured_bodies[0].get("reasoning_effort").is_none());
    assert!(captured_bodies[0].get("thinking").is_none());
    assert!(captured_bodies[0].get("chat_template_kwargs").is_none());
}

#[tokio::test]
async fn off_mode_caches_successful_template_disable_fallback() {
    let (api_url, server) = spawn_off_template_fallback_server().await;
    let endpoint = unknown_openai_compat_endpoint("fau", &api_url, "moonshotai/Kimi-K2.6");
    let options = reasoning_options(LlmReasoningMode::Off);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("template fallback request should succeed against the local server");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(response, "ok");
    assert_eq!(captured_bodies.len(), 2);
    assert_eq!(
        captured_bodies[0]["thinking"],
        json!({ "type": "disabled" })
    );
    assert_eq!(
        captured_bodies[1]["chat_template_kwargs"],
        json!({ "thinking": false })
    );
    assert!(captured_bodies[1].get("thinking").is_none());
    assert_eq!(
        llm_provider::cached_auto_reasoning_strategy(&endpoint, false, LlmReasoningMode::Off),
        Some(AutoReasoningStrategy::ChatTemplateThinking),
        "successful vLLM/SGLang-style Kimi disable strategy should be cached for Off"
    );
}

#[tokio::test]
async fn off_mode_template_rejection_falls_back_to_no_controls() {
    let (api_url, server) = spawn_off_template_rejection_server().await;
    let endpoint =
        unknown_openai_compat_endpoint("fau-off-terminal", &api_url, "moonshotai/Kimi-K2.6");
    let options = reasoning_options(LlmReasoningMode::Off);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("template rejection should fall back to no reasoning controls");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(response, "ok");
    assert_eq!(captured_bodies.len(), 3);
    assert_eq!(
        captured_bodies[0]["thinking"],
        json!({ "type": "disabled" })
    );
    assert_eq!(
        captured_bodies[1]["chat_template_kwargs"],
        json!({ "thinking": false })
    );
    assert!(captured_bodies[2].get("thinking").is_none());
    assert!(captured_bodies[2].get("chat_template_kwargs").is_none());
    assert!(captured_bodies[2].get("reasoning").is_none());
    assert!(captured_bodies[2].get("reasoning_effort").is_none());
    assert_eq!(
        llm_provider::cached_auto_reasoning_strategy(&endpoint, false, LlmReasoningMode::Off),
        Some(AutoReasoningStrategy::NoControls),
        "after both Off controls are rejected, the endpoint should learn to send no reasoning controls"
    );
}

#[tokio::test]
async fn responses_retry_can_strip_max_tokens_then_reasoning_controls() {
    let (api_url, server) = spawn_responses_max_tokens_then_reasoning_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "responses-compat",
        &api_url,
        "future-lab/responses-compat-model",
    );
    let options = reasoning_options(LlmReasoningMode::Balanced);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("compatibility retries should succeed against the local server");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(response, "ok");
    assert_eq!(captured_bodies.len(), 4);
    assert!(captured_bodies[0].get("max_output_tokens").is_some());
    assert!(captured_bodies[0].get("reasoning").is_some());
    assert!(captured_bodies[1].get("max_output_tokens").is_none());
    assert!(captured_bodies[1].get("reasoning").is_some());
    assert!(captured_bodies[2].get("max_output_tokens").is_none());
    assert_eq!(captured_bodies[2]["reasoning_effort"], json!("medium"));
    assert!(captured_bodies[2].get("reasoning").is_none());
    assert!(captured_bodies[3].get("max_output_tokens").is_none());
    assert!(captured_bodies[3].get("reasoning").is_none());
    assert!(captured_bodies[3].get("reasoning_effort").is_none());
}

#[tokio::test]
async fn responses_max_output_tokens_strip_is_cached_after_success() {
    let (api_url, server) = spawn_responses_max_tokens_cache_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "responses-max-output-cache",
        &api_url,
        "future-lab/responses-max-output-cache-model",
    );
    let options = reasoning_options(LlmReasoningMode::ProviderDefault);
    let first_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let first_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &first_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("first request should learn to strip max_output_tokens");
    assert_eq!(first_response, "ok");

    let second_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
    let second_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &second_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("cached max_output_tokens stripping should make the second request single-shot");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(second_response, "ok");
    assert_eq!(captured_bodies.len(), 3);
    assert!(captured_bodies[0].get("max_output_tokens").is_some());
    assert!(captured_bodies[1].get("max_output_tokens").is_none());
    assert!(captured_bodies[2].get("max_output_tokens").is_none());
}

#[tokio::test]
async fn chat_max_completion_tokens_strip_is_cached_after_success() {
    let (api_url, server) = spawn_chat_max_completion_tokens_cache_server().await;
    let endpoint = LlmEndpoint {
        provider: "cerebras".to_string(),
        api_url,
        model: "gpt-oss-120b".to_string(),
        timeout_secs: 10,
        api_format: ApiFormat::OpenaiCompat,
    };
    let options = reasoning_options(LlmReasoningMode::ProviderDefault);
    let first_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let first_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &first_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("first request should learn to strip max_completion_tokens");
    assert_eq!(first_response, "ok");

    let second_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
    let second_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &second_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("cached max_completion_tokens stripping should make the second request single-shot");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(second_response, "ok");
    assert_eq!(captured_bodies.len(), 3);
    assert!(captured_bodies[0].get("max_completion_tokens").is_some());
    assert!(captured_bodies[1].get("max_completion_tokens").is_none());
    assert!(captured_bodies[2].get("max_completion_tokens").is_none());
}

#[tokio::test]
async fn token_limit_error_without_token_field_does_not_retry_identical_body() {
    let (api_url, server) = spawn_token_limit_error_without_token_field_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "token-limit-guard-provider",
        &api_url,
        "future-lab/token-limit-guard-model",
    );
    let options = reasoning_options(LlmReasoningMode::ProviderDefault);
    let body = json!({
        "model": endpoint.model,
        "messages": [
            { "role": "user", "content": "hello" }
        ]
    });

    let error = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect_err("a token-limit-looking error without a token field should fail fast");
    let captured_bodies = server.await.expect("server task should complete");

    assert!(
        error.contains("max_tokens is not supported"),
        "original API error should be returned without compatibility retry: {error}"
    );
    assert_eq!(captured_bodies.len(), 1);
    assert!(captured_bodies[0].get("max_tokens").is_none());
    assert!(captured_bodies[0].get("max_completion_tokens").is_none());
    assert!(captured_bodies[0].get("max_output_tokens").is_none());
}

#[tokio::test]
async fn responses_no_controls_and_max_output_caches_make_next_request_single_shot() {
    let (api_url, server) = spawn_responses_combined_cache_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "responses-combined-cache",
        &api_url,
        "future-lab/responses-combined-cache-model",
    );
    let options = reasoning_options(LlmReasoningMode::Balanced);
    let first_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let first_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &first_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("first request should learn both terminal compatibility decisions");
    assert_eq!(first_response, "ok");

    let second_body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
    let second_response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &second_body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("combined caches should make the second request single-shot");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(second_response, "ok");
    assert_eq!(captured_bodies.len(), 5);
    assert!(captured_bodies[0].get("max_output_tokens").is_some());
    assert!(captured_bodies[0].get("reasoning").is_some());
    assert!(captured_bodies[1].get("max_output_tokens").is_none());
    assert!(captured_bodies[1].get("reasoning").is_some());
    assert!(captured_bodies[2].get("max_output_tokens").is_none());
    assert!(captured_bodies[2].get("reasoning_effort").is_some());
    assert!(captured_bodies[3].get("max_output_tokens").is_none());
    assert!(captured_bodies[3].get("reasoning").is_none());
    assert!(captured_bodies[3].get("reasoning_effort").is_none());
    assert!(captured_bodies[4].get("max_output_tokens").is_none());
    assert!(captured_bodies[4].get("reasoning").is_none());
    assert!(captured_bodies[4].get("reasoning_effort").is_none());
}

#[tokio::test]
async fn responses_retry_can_handle_reasoning_then_max_tokens_rejections() {
    let (api_url, server) = spawn_responses_reasoning_then_max_tokens_server().await;
    let endpoint = unknown_openai_compat_endpoint(
        "responses-reverse-compat",
        &api_url,
        "future-lab/responses-reverse-compat-model",
    );
    let options = reasoning_options(LlmReasoningMode::Balanced);
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        "sk-test",
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("compatibility retries should succeed regardless of rejection order");

    let captured_bodies = server.await.expect("server task should complete");

    assert_eq!(response, "ok");
    assert_eq!(captured_bodies.len(), 4);
    assert!(captured_bodies[0].get("max_output_tokens").is_some());
    assert!(captured_bodies[0].get("reasoning").is_some());
    assert!(captured_bodies[1].get("max_output_tokens").is_some());
    assert_eq!(captured_bodies[1]["reasoning_effort"], json!("medium"));
    assert!(captured_bodies[2].get("max_output_tokens").is_none());
    assert_eq!(captured_bodies[2]["reasoning_effort"], json!("medium"));
    assert!(captured_bodies[3].get("max_output_tokens").is_none());
    assert!(captured_bodies[3].get("reasoning").is_none());
    assert!(captured_bodies[3].get("reasoning_effort").is_none());
}

#[test]
fn successful_auto_strategy_cache_is_keyed_by_endpoint_and_model_not_provider_id() {
    let api_url = "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions";
    let model = "future-lab/cache-probe-model";
    let original_provider = unknown_openai_compat_endpoint("renamable-provider-a", api_url, model);
    let renamed_provider = unknown_openai_compat_endpoint("renamable-provider-b", api_url, model);

    llm_provider::remember_auto_reasoning_strategy(
        &original_provider,
        false,
        LlmReasoningMode::Balanced,
        AutoReasoningStrategy::OpenaiResponsesReasoning,
    );

    let mut next_body = json!({});
    llm_provider::apply_reasoning_controls(
        &renamed_provider,
        false,
        &mut next_body,
        LlmReasoningMode::Balanced,
    );

    assert_eq!(
        next_body["reasoning"],
        json!({ "effort": "medium" }),
        "a successful auto-detected strategy must be reused for the same endpoint/model even if the local provider id changes"
    );
    assert!(
        next_body.get("reasoning_effort").is_none(),
        "provider-id cache misses reintroduce the hard-coded first probe instead of using the learned strategy: {next_body}"
    );
}

#[test]
fn cached_no_controls_strategy_skips_future_auto_reasoning_params() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-no-controls",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "future-lab/no-controls-model",
    );

    llm_provider::remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::NoControls,
    );

    let mut next_body = json!({});
    llm_provider::apply_reasoning_controls(
        &endpoint,
        false,
        &mut next_body,
        LlmReasoningMode::Deep,
    );

    assert!(
        next_body.get("reasoning").is_none() && next_body.get("reasoning_effort").is_none(),
        "when a provider rejects known standard controls, later requests should avoid another slow probe: {next_body}"
    );
}

#[test]
fn cached_no_controls_strategy_prevents_future_fallback_generation() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-no-controls-fallback",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "future-lab/no-controls-fallback-model",
    );

    llm_provider::remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::NoControls,
    );

    let fallbacks = llm_provider::auto_reasoning_fallback_bodies(
        &endpoint,
        false,
        &json!({ "model": endpoint.model, "messages": [] }),
        LlmReasoningMode::Deep,
    );

    assert!(
        fallbacks.is_empty(),
        "NoControls is a learned terminal strategy and must not spend future requests probing again: {fallbacks:?}"
    );
}

#[test]
fn cached_effort_strategy_does_not_turn_off_into_low_effort() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-effort-then-off",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "future-lab/effort-then-off-model",
    );

    llm_provider::remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::OpenaiChatReasoningEffort,
    );

    let mut next_body = json!({});
    llm_provider::apply_reasoning_controls(&endpoint, false, &mut next_body, LlmReasoningMode::Off);

    assert_eq!(next_body["thinking"], json!({ "type": "disabled" }));
    assert!(
        next_body.get("reasoning_effort").is_none() && next_body.get("reasoning").is_none(),
        "a cached effort strategy must not convert Off into low effort: {next_body}"
    );
}

#[test]
fn no_controls_cache_for_deep_does_not_disable_light_effort_probe() {
    let endpoint = unknown_openai_compat_endpoint(
        "effort-mode-isolation",
        "https://mode-isolation.example/v1/chat/completions",
        "future-lab/mode-isolation-model",
    );

    llm_provider::remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::NoControls,
    );

    let mut light_body = json!({});
    llm_provider::apply_reasoning_controls(
        &endpoint,
        false,
        &mut light_body,
        LlmReasoningMode::Light,
    );

    assert_eq!(
        light_body["reasoning_effort"],
        json!("low"),
        "a terminal Deep result must not prevent a lower effort mode from probing its own supported value"
    );
}

#[test]
fn cached_template_disable_strategy_reuses_template_kwargs_for_off() {
    let endpoint = unknown_openai_compat_endpoint(
        "fau-llmgw-template-disable",
        "https://hub.nhr.fau.de/api/llmgw/v1/chat/completions",
        "moonshotai/Kimi-K2.6",
    );

    llm_provider::remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Off,
        AutoReasoningStrategy::ChatTemplateThinking,
    );

    let mut next_body = json!({});
    llm_provider::apply_reasoning_controls(&endpoint, false, &mut next_body, LlmReasoningMode::Off);

    assert_eq!(
        next_body["chat_template_kwargs"],
        json!({ "thinking": false })
    );
    assert!(next_body.get("thinking").is_none());
    assert!(next_body.get("reasoning_effort").is_none());
}

#[test]
fn known_deepseek_openai_compatible_endpoint_keeps_native_reasoning_path() {
    let endpoint = llm_provider::LlmEndpoint {
        provider: "deepseek".to_string(),
        api_url: "https://proxy.example/v1".to_string(),
        model: "deepseek-reasoner".to_string(),
        timeout_secs: 10,
        api_format: ApiFormat::OpenaiCompat,
    };

    let support = llm_provider::reasoning_support(&endpoint, false);
    assert_eq!(support.strategy.as_deref(), Some("deepseek_thinking"));

    let body = build_llm_body(
        &endpoint,
        "system",
        &LlmUserInput::from("hello"),
        reasoning_options(LlmReasoningMode::Balanced),
    );

    assert_eq!(body["thinking"], json!({ "type": "enabled" }));
    assert!(
        llm_provider::auto_reasoning_fallback_bodies(
            &endpoint,
            false,
            &body,
            LlmReasoningMode::Balanced,
        )
        .is_empty(),
        "known providers keep their native reasoning path and skip the generic OpenAI-compatible probe"
    );
}
