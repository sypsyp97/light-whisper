use super::{probe_decision, probe_llm};
use crate::services::jev_service_tests::{spawn_json_server, CapturedRequest};
use crate::services::llm_client::LlmRequestOptions;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::{ApiFormat, JevConfig, JevConnection, JevProvider};
use base64::Engine;
use serde_json::Value;
use std::time::Duration;
use tokio::task::JoinHandle;

const KEY: &str = "connection-test-key-sentinel";
const BODY_SECRET: &str = "private-body-sentinel";
const CHAT_OK: &[u8] = br#"{"choices":[{"message":{"content":"OK"},"finish_reason":"stop"}]}"#;
const RESPONSES_OK: &[u8] = br#"{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}]}"#;
const ANTHROPIC_OK: &[u8] =
    br#"{"content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn"}"#;
const DECISION_OK: &[u8] = br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.98,"polish":0.01,"uncertain":0.01}}}}"#;
const PRIVATE_ERROR: &[u8] =
    br#"{"error":{"message":"private-body-sentinel connection-test-key-sentinel"}}"#;
// Wire fixture: deterministic 32x32 solid-red RGB PNG, never a screen capture.
const SYNTHETIC_RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKElEQVR4nO3NsQ0AAAzCMP5/un0CNkuZ41wybXsHAAAAAAAAAAAAxR4yw/wuPL6QkAAAAABJRU5ErkJggg==";

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn endpoint(url: String, api_format: ApiFormat) -> LlmEndpoint {
    LlmEndpoint {
        provider: "custom-connection-test".into(),
        api_url: url,
        model: "selected-test-model".into(),
        timeout_secs: 120,
        api_format,
    }
}

fn decision_config(provider: JevProvider, url: String) -> JevConfig {
    JevConfig {
        provider,
        connections: [(
            provider,
            JevConnection {
                base_url: url,
                model: "selected-decision-model".into(),
            },
        )]
        .into(),
        ..JevConfig::default()
    }
}

async fn captured(mut server: JoinHandle<CapturedRequest>) -> CapturedRequest {
    match tokio::time::timeout(Duration::from_secs(1), &mut server).await {
        Ok(result) => result.expect("local HTTP fixture must finish"),
        Err(_) => {
            server.abort();
            panic!("connection probe did not issue an inference POST");
        }
    }
}

fn header<'a>(request: &'a CapturedRequest, name: &str) -> Option<&'a str> {
    request.headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn assert_sanitized_error(result: Result<(), String>) -> String {
    let error = result.expect_err("an unusable response must not pass the connection check");
    assert!(!error.trim().is_empty());
    for secret in [KEY, BODY_SECRET] {
        assert!(!error.contains(secret), "probe error exposed private data");
    }
    error
}

fn user_content(body: &Value) -> &Value {
    &body
        .get("messages")
        .or_else(|| body.get("input"))
        .and_then(Value::as_array)
        .expect("probe must send messages or Responses input")
        .iter()
        .find(|message| message["role"] == "user")
        .expect("probe must send user input")["content"]
}

#[tokio::test]
async fn llm_posts_selected_model_and_auth_for_each_supported_wire_format() {
    for (format, path, response) in [
        (ApiFormat::OpenaiCompat, "/v1/chat/completions", CHAT_OK),
        (ApiFormat::OpenaiCompat, "/v1/responses", RESPONSES_OK),
        (ApiFormat::Anthropic, "/v1/messages", ANTHROPIC_OK),
    ] {
        let (origin, server) = spawn_json_server("200 OK", response).await;
        let selected = endpoint(format!("{origin}{path}"), format.clone());
        let result = probe_llm(
            &client(),
            &selected,
            KEY,
            false,
            LlmRequestOptions::default(),
        )
        .await;
        assert_eq!(result, Ok(()), "valid completed response at {path}");
        let request = captured(server).await;
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, path);
        assert_eq!(request.body["model"], "selected-test-model");
        if format == ApiFormat::Anthropic {
            assert_eq!(header(&request, "x-api-key"), Some(KEY));
        } else {
            assert_eq!(
                header(&request, "authorization"),
                Some(format!("Bearer {KEY}").as_str())
            );
        }
        let serialized = request.body.to_string();
        assert!(!serialized.contains("image_url"));
        assert!(!serialized.contains("input_image"));
        assert!(!serialized.contains("\"type\":\"image\""));
        assert!(!serialized.contains(KEY));
        let input = user_content(&request.body);
        let nonempty_text = input.as_str().is_some_and(|text| !text.trim().is_empty())
            || input.as_array().is_some_and(|parts| {
                parts.iter().any(|part| {
                    matches!(part["type"].as_str(), Some("text" | "input_text"))
                        && part["text"]
                            .as_str()
                            .is_some_and(|text| !text.trim().is_empty())
                })
            });
        assert!(
            nonempty_text,
            "probe must contain nonempty synthetic user text at {path}"
        );
    }
}

#[tokio::test]
async fn llm_vision_posts_a_fixed_synthetic_image() {
    let mut previous_image = None;
    for _ in 0..2 {
        let (origin, server) = spawn_json_server("200 OK", CHAT_OK).await;
        let selected = endpoint(
            format!("{origin}/v1/chat/completions"),
            ApiFormat::OpenaiCompat,
        );
        assert_eq!(
            probe_llm(
                &client(),
                &selected,
                KEY,
                true,
                LlmRequestOptions::default()
            )
            .await,
            Ok(())
        );
        let request = captured(server).await;
        let content = user_content(&request.body)
            .as_array()
            .expect("vision input must contain parts");
        let image = content
            .iter()
            .find(|part| part["type"] == "image_url")
            .expect("vision probe must include an image")["image_url"]["url"]
            .as_str()
            .unwrap();
        assert!(image.starts_with("data:image/"));
        let (_, encoded) = image
            .split_once(";base64,")
            .expect("image must be embedded");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let expected = base64::engine::general_purpose::STANDARD
            .decode(SYNTHETIC_RED_PNG)
            .unwrap();
        assert_eq!(
            bytes, expected,
            "vision probe must send the complete synthetic red PNG"
        );
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 32);
        assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 32);
        if let Some(previous) = &previous_image {
            assert_eq!(
                image, previous,
                "connection checks must reuse a fixed synthetic image"
            );
        }
        previous_image = Some(image.to_string());
    }
}

#[tokio::test]
async fn llm_rejects_malformed_empty_and_incomplete_responses() {
    for (format, path, response) in [
        (ApiFormat::OpenaiCompat, "/v1/chat/completions", b"private-body-sentinel".as_slice()),
        (ApiFormat::OpenaiCompat, "/v1/chat/completions", br#"{"choices":[]}"#.as_slice()),
        (ApiFormat::OpenaiCompat, "/v1/chat/completions", br#"{"choices":[{"message":{"content":"   "},"finish_reason":"stop"}]}"#.as_slice()),
        (ApiFormat::OpenaiCompat, "/v1/chat/completions", br#"{"choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}"#.as_slice()),
        (ApiFormat::OpenaiCompat, "/v1/responses", br#"{"status":"incomplete","output":[{"type":"message","content":[{"type":"output_text","text":"partial"}]}]}"#.as_slice()),
        (ApiFormat::Anthropic, "/v1/messages", br#"{"content":[{"type":"text","text":"partial"}],"stop_reason":"max_tokens"}"#.as_slice()),
    ] {
        let (origin, server) = spawn_json_server("200 OK", response).await;
        let selected = endpoint(format!("{origin}{path}"), format);
        let result = probe_llm(&client(), &selected, KEY, false, LlmRequestOptions::default()).await;
        captured(server).await;
        assert_sanitized_error(result);
    }
}

#[tokio::test]
async fn llm_http_errors_report_status_without_echoing_credentials_or_body() {
    for (status, code) in [
        ("401 Unauthorized", "401"),
        ("503 Service Unavailable", "503"),
    ] {
        let (origin, server) = spawn_json_server(status, PRIVATE_ERROR).await;
        let selected = endpoint(
            format!("{origin}/v1/chat/completions"),
            ApiFormat::OpenaiCompat,
        );
        let result = probe_llm(
            &client(),
            &selected,
            KEY,
            false,
            LlmRequestOptions::default(),
        )
        .await;
        let error = assert_sanitized_error(result);
        assert!(
            error.contains(code),
            "HTTP status must remain visible: {error}"
        );
        captured(server).await;
    }
}

#[tokio::test]
async fn decision_accepts_valid_nonpass_and_low_confidence_answers_for_selected_providers() {
    for (provider, path, response) in [
        (JevProvider::TypeSafe, "/v1/systemone", DECISION_OK),
        (JevProvider::OpenRouter, "/api/alpha/decisions", br#"{"answers":{"route":{"type":"choice","choice":"polish","probabilities":{"pass":0.01,"polish":0.98,"uncertain":0.01}}}}"#.as_slice()),
        (JevProvider::Vercel, "/v4/ai/evaluation-model", br#"{"answers":{"route":{"type":"choice","choice":"uncertain","probabilities":{"pass":0.01,"polish":0.01,"uncertain":0.98}}}}"#.as_slice()),
        (JevProvider::Liquid, "/decisions/v1/systemone", br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.5,"polish":0.3,"uncertain":0.2}}}}"#.as_slice()),
        (JevProvider::Custom, "/private/Decision", br#"{"answers":{"route":{"type":"choice","choice":"polish","probabilities":{"pass":0.8,"polish":0.1,"uncertain":0.1}}}}"#.as_slice()),
    ] {
        let (origin, server) = spawn_json_server("200 OK", response).await;
        let config = decision_config(provider, format!("{origin}{path}"));
        assert_eq!(probe_decision(&client(), &config, KEY).await, Ok(()), "{provider:?}");
        let request = captured(server).await;
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, path);
        assert_eq!(header(&request, "authorization"), Some(format!("Bearer {KEY}").as_str()));
        if provider == JevProvider::Vercel {
            assert_eq!(header(&request, "ai-model-id"), Some("selected-decision-model"));
            assert!(request.body.get("model").is_none());
        } else {
            assert_eq!(request.body["model"], "selected-decision-model");
        }
        let route = &request.body["questions"]["route"];
        assert_eq!(route["type"], "choice");
        assert_eq!(route["criteria"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["pass", "polish", "uncertain"]);
        for field in ["text", "polishing_policy"] {
            assert!(!request.body["state"][field].as_str().unwrap().trim().is_empty());
        }
        assert!(!request.body.to_string().contains(KEY));
    }
}

#[tokio::test]
async fn decision_rejects_malformed_or_invalid_answer_distributions() {
    for response in [
        b"private-body-sentinel".as_slice(),
        b"{}".as_slice(),
        br#"{"answers":{"route":{"type":"choice","choice":"pass"}}}"#.as_slice(),
        br#"{"answers":{"route":{"type":"choice","choice":"unknown","probabilities":{"pass":0.98,"polish":0.01,"uncertain":0.01}}}}"#.as_slice(),
        br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.9,"polish":0.9,"uncertain":0.9}}}}"#.as_slice(),
        br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":1.2,"polish":-0.2,"uncertain":0.0}}}}"#.as_slice(),
    ] {
        let (origin, server) = spawn_json_server("200 OK", response).await;
        let config = decision_config(JevProvider::Custom, format!("{origin}/decision"));
        let result = probe_decision(&client(), &config, KEY).await;
        captured(server).await;
        assert_sanitized_error(result);
    }
}

#[tokio::test]
async fn decision_http_errors_report_status_without_echoing_credentials_or_body() {
    for (status, code) in [
        ("401 Unauthorized", "401"),
        ("503 Service Unavailable", "503"),
    ] {
        let (origin, server) = spawn_json_server(status, PRIVATE_ERROR).await;
        let config = decision_config(JevProvider::Custom, format!("{origin}/decision"));
        let error = assert_sanitized_error(probe_decision(&client(), &config, KEY).await);
        assert!(
            error.contains(code),
            "HTTP status must remain visible: {error}"
        );
        captured(server).await;
    }
}

#[tokio::test]
async fn decision_missing_key_is_rejected_without_a_request() {
    for key in ["", "   "] {
        let (origin, mut server) = spawn_json_server("200 OK", DECISION_OK).await;
        let config = decision_config(JevProvider::Custom, format!("{origin}/decision"));
        let result = probe_decision(&client(), &config, key).await;
        let sent_request = tokio::time::timeout(Duration::from_millis(100), &mut server)
            .await
            .is_ok();
        server.abort();
        assert_sanitized_error(result);
        assert!(
            !sent_request,
            "missing credentials must not trigger inference"
        );
    }
}
