use super::{jev_service, jev_service_tests::spawn_json_server, jev_tasks};
use crate::state::user_profile::JevConfig;
use serde_json::{json, Value};
use std::time::Duration;

const KEY: &str = "private-key-sentinel";
const STATE: &str = "private-state-sentinel";

fn assert_warning<T: std::fmt::Debug>(result: Result<T, String>, endpoint: &str) {
    let warning = result.expect_err("service failure must preserve a warning");
    assert!(!warning.trim().is_empty());
    for secret in [KEY, STATE, "private-body-sentinel", endpoint] {
        assert!(
            !warning.contains(secret),
            "warning leaked private input: {warning}"
        );
    }
}

async fn task(endpoint: &str) -> Result<Option<Value>, String> {
    jev_tasks::evaluate_with_warning(
        &reqwest::Client::builder().no_proxy().build().unwrap(),
        &JevConfig::default(),
        KEY,
        json!({"text": STATE}),
        jev_tasks::screen_questions(),
        Duration::from_secs(1),
        Some(endpoint),
    )
    .await
}

async fn gate(endpoint: &str) -> Result<bool, String> {
    jev_service::evaluate_with_warning(
        &reqwest::Client::builder().no_proxy().build().unwrap(),
        &JevConfig::default(),
        KEY,
        STATE,
        "test policy",
        Some(endpoint.to_string()),
    )
    .await
}

#[tokio::test]
async fn gate_http_and_malformed_answers_preserve_sanitized_warnings() {
    for (status, body) in [
        (
            "503 Service Unavailable",
            br#"{"error":"private-body-sentinel"}"#.as_slice(),
        ),
        ("200 OK", b"private-body-sentinel".as_slice()),
        ("200 OK", b"{}".as_slice()),
        (
            "200 OK",
            br#"{"answers":{"route":{"type":"choice","choice":"pass"}}}"#.as_slice(),
        ),
    ] {
        let (endpoint, server) = spawn_json_server(status, body).await;
        let result = gate(&endpoint).await;
        server.await.unwrap();
        assert_warning(result, &endpoint);
    }
}

#[tokio::test]
async fn task_http_and_malformed_answers_preserve_sanitized_warnings() {
    for (status, body) in [
        ("503 Service Unavailable", br#"{"error":"private-body-sentinel"}"#.as_slice()),
        ("200 OK", b"private-body-sentinel".as_slice()),
        ("200 OK", b"{}".as_slice()),
        ("200 OK", br#"{"answers":{"screen":{"type":"choice","choice":"needed"}}}"#.as_slice()),
        ("200 OK", br#"{"answers":{"search":{"type":"choice","choice":"needed","probabilities":{"needed":0.9,"unneeded":0.05,"uncertain":0.05}}}}"#.as_slice()),
    ] {
        let (endpoint, server) = spawn_json_server(status, body).await;
        let result = task(&endpoint).await;
        server.await.unwrap();
        assert_warning(result, &endpoint);
    }
}

#[tokio::test]
async fn transport_refusal_preserves_a_sanitized_warning() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    assert_warning(task(&endpoint).await, &endpoint);
    assert_warning(gate(&endpoint).await, &endpoint);
}

#[tokio::test]
async fn valid_gate_low_confidence_uncertain_and_nonpass_remain_quiet() {
    for (body, expected) in [
        (br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.5,"polish":0.3,"uncertain":0.2}}}}"#.as_slice(), false),
        (br#"{"answers":{"route":{"type":"choice","choice":"uncertain","probabilities":{"pass":0.01,"polish":0.01,"uncertain":0.98}}}}"#.as_slice(), false),
        (br#"{"answers":{"route":{"type":"choice","choice":"polish","probabilities":{"pass":0.01,"polish":0.98,"uncertain":0.01}}}}"#.as_slice(), false),
        (br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.98,"polish":0.01,"uncertain":0.01}}}}"#.as_slice(), true),
    ] {
        let (endpoint, server) = spawn_json_server("200 OK", body).await;
        assert_eq!(gate(&endpoint).await, Ok(expected));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn valid_task_low_confidence_uncertain_and_nonwinning_choice_remain_quiet() {
    for body in [
        br#"{"answers":{"screen":{"type":"choice","choice":"needed","probabilities":{"needed":0.5,"unneeded":0.3,"uncertain":0.2}}}}"#.as_slice(),
        br#"{"answers":{"screen":{"type":"choice","choice":"uncertain","probabilities":{"needed":0.01,"unneeded":0.01,"uncertain":0.98}}}}"#.as_slice(),
        br#"{"answers":{"screen":{"type":"choice","choice":"needed","probabilities":{"needed":0.01,"unneeded":0.98,"uncertain":0.01}}}}"#.as_slice(),
    ] {
        let (endpoint, server) = spawn_json_server("200 OK", body).await;
        assert_eq!(task(&endpoint).await, Ok(Some(serde_json::from_slice(body).unwrap())));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn empty_questions_are_a_quiet_noop_even_without_a_key() {
    let result = jev_tasks::evaluate_with_warning(
        &reqwest::Client::new(),
        &JevConfig::default(),
        "",
        json!({}),
        json!({}),
        Duration::from_millis(100),
        Some("http://127.0.0.1:1"),
    )
    .await;
    assert_eq!(result, Ok(None));
}
