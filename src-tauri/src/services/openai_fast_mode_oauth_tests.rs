use crate::services::llm_client::{self, LlmRequestOptions, LlmUserInput};
use crate::services::llm_provider::{self, LlmEndpoint};
use crate::state::user_profile::{ApiFormat, LlmReasoningMode};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const OAUTH_DERIVED_OPENAI_API_KEY_PREFIX: &str = "openai-codex-oauth-api-key:";

struct CapturedRequest {
    authorization: Option<String>,
    body: Value,
}

fn wrapped_oauth_derived_api_key(real_api_key: &str) -> String {
    format!("{OAUTH_DERIVED_OPENAI_API_KEY_PREFIX}{real_api_key}")
}

fn parse_header_value(headers: &str, name: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        if header_name.trim().eq_ignore_ascii_case(name) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn openai_responses_endpoint(api_url: String) -> LlmEndpoint {
    LlmEndpoint {
        provider: "openai".to_string(),
        api_url,
        model: "gpt-4.1-mini".to_string(),
        timeout_secs: 5,
        api_format: ApiFormat::OpenaiCompat,
    }
}

async fn spawn_request_capture_server() -> (String, tokio::task::JoinHandle<CapturedRequest>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("capture server should bind");
    let address = listener
        .local_addr()
        .expect("capture server should have a local address");

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("server should accept");
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
            .expect("request headers should be valid utf-8")
            .to_string();
        let content_length = parse_header_value(&headers, "Content-Length")
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

        let body = serde_json::from_slice::<Value>(
            &request_bytes[headers_end..headers_end + content_length],
        )
        .expect("request body should be valid json");

        let response_body =
            br#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#;
        let response_headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        );
        stream
            .write_all(response_headers.as_bytes())
            .await
            .expect("response headers should be writable");
        stream
            .write_all(response_body)
            .await
            .expect("response body should be writable");

        CapturedRequest {
            authorization: parse_header_value(&headers, "Authorization"),
            body,
        }
    });

    (format!("http://{address}/v1/responses"), handle)
}

#[test]
fn build_auth_headers_unwraps_oauth_derived_openai_api_key() {
    let real_api_key = "sk-oauth-real-api-key";
    let wrapped = wrapped_oauth_derived_api_key(real_api_key);

    let headers = llm_provider::build_auth_headers(&ApiFormat::OpenaiCompat, &wrapped)
        .expect("wrapped OAuth-derived API keys should still build auth headers");

    assert_eq!(
        headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer sk-oauth-real-api-key"),
        "OpenAI auth headers must send the real API key value on the wire"
    );
    assert_eq!(
        headers
            .get("Content-Type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    assert!(
        headers.get("originator").is_none(),
        "OAuth-derived API keys should keep standard OpenAI-compatible headers"
    );
    assert!(
        headers.get("ChatGPT-Account-ID").is_none(),
        "OAuth-derived API keys are still API-key auth, so ChatGPT bearer headers do not apply"
    );
}

#[tokio::test]
async fn send_llm_request_injects_priority_and_unwraps_oauth_derived_openai_api_key() {
    let (api_url, server) = spawn_request_capture_server().await;
    let endpoint = openai_responses_endpoint(api_url);
    let api_key = wrapped_oauth_derived_api_key("sk-oauth-real-api-key");
    let options = LlmRequestOptions {
        reasoning_mode: LlmReasoningMode::ProviderDefault,
        openai_fast_mode: true,
        ..LlmRequestOptions::default()
    };
    let body =
        llm_client::build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);

    let response = llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        &api_key,
        &body,
        "hello".len(),
        None,
        options,
    )
    .await
    .expect("request should succeed against the local capture server");

    let captured = server.await.expect("capture task should complete");

    assert_eq!(response, "ok");
    let mut regressions = Vec::new();
    if captured.authorization.as_deref() != Some("Bearer sk-oauth-real-api-key") {
        regressions.push(format!(
            "Authorization header mismatch: got {:?}",
            captured.authorization
        ));
    }
    if captured.body.get("service_tier") != Some(&json!("priority")) {
        regressions.push(format!(
            "service_tier mismatch: got {:?} in body {}",
            captured.body.get("service_tier"),
            captured.body
        ));
    }
    assert!(
        regressions.is_empty(),
        "send_llm_request regressed for OAuth-derived OpenAI API keys: {}",
        regressions.join("; ")
    );
}
