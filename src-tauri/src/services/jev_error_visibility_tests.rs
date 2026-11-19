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

tokio::task_local! {
    static WARNINGS: std::cell::RefCell<Vec<(log::Level, String)>>;
}

struct WarningCapture;

impl log::Log for WarningCapture {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let _ = WARNINGS.try_with(|warnings| {
                warnings
                    .borrow_mut()
                    .push((record.level(), format!("{}", record.args())));
            });
        }
    }

    fn flush(&self) {}
}

pub(super) async fn capture_warning<F: std::future::Future>(
    operation: F,
) -> (F::Output, Vec<(log::Level, String)>) {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        log::set_logger(&WarningCapture).expect("install test capture");
        log::set_max_level(log::LevelFilter::Warn);
    });
    WARNINGS
        .scope(Default::default(), async {
            let result = operation.await;
            (result, WARNINGS.with(|records| records.take()))
        })
        .await
}

pub(super) fn only_warning<'a>(records: &'a [(log::Level, String)], marker: &str) -> &'a str {
    assert_eq!(
        records.len(),
        1,
        "expected exactly one {marker} warning, got {records:?}"
    );
    let (level, message) = &records[0];
    assert_eq!(*level, log::Level::Warn);
    assert!(message.contains(marker), "{message}");
    for sensitive in [
        KEY,
        STATE,
        "private-prompt-sentinel",
        "private-body-sentinel",
        "private-model-sentinel",
        "private-path-sentinel",
        "private-query-sentinel",
        "127.0.0.1",
    ] {
        assert!(!message.contains(sensitive), "log exposed {sensitive}");
    }
    message
}

#[tokio::test]
async fn decision_model_diagnostic_http_failure_from_gate() {
    use crate::state::user_profile::{JevConnection, JevProvider};
    let provider = JevProvider::OpenRouter;
    let config = JevConfig {
        provider,
        connections: [(
            provider,
            JevConnection {
                base_url: String::new(),
                model: "private-model-sentinel".into(),
            },
        )]
        .into(),
        ..Default::default()
    };
    let (endpoint, server) =
        spawn_json_server("429 Too Many Requests", b"private-body-sentinel").await;
    let (result, records) = capture_warning(jev_service::evaluate_with_warning(
        &reqwest::Client::builder().no_proxy().build().unwrap(),
        &config,
        KEY,
        STATE,
        "private-prompt-sentinel",
        Some(format!("{endpoint}/private-path-sentinel")),
    ))
    .await;
    let request = server.await.unwrap();
    assert!(request.body.to_string().contains("private-prompt-sentinel"));
    assert!(request.body.to_string().contains(STATE));
    assert!(request.headers.contains(KEY));
    assert!(request.path.contains("private-path-sentinel"));
    assert_eq!(
        result,
        Err("决策服务返回 HTTP 429，请检查 API Key、权限或服务状态后重试。".into())
    );
    let warning = only_warning(&records, "decision_model").to_ascii_lowercase();
    assert!(
        warning.contains("provider=openrouter") && warning.contains("429"),
        "{warning}"
    );
}

#[tokio::test]
async fn decision_model_diagnostic_classes_and_quiet_paths() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let config = JevConfig::default();
    let evaluate = |key: &'static str, questions, deadline, endpoint: String| {
        let client = &client;
        let config = &config;
        async move {
            capture_warning(jev_tasks::evaluate_with_warning(
                client,
                config,
                key,
                json!({"text": STATE}),
                questions,
                deadline,
                Some(&endpoint),
            ))
            .await
        }
    };
    let questions = jev_tasks::screen_questions;
    let invalid_url = "http://127.0.0.1/private-path-sentinel?q=private-query-sentinel";
    for (key, request, error, category) in [
        (
            KEY,
            json!([]),
            "决策请求格式无效，请检查配置后重试。",
            "invalid_request",
        ),
        (
            "",
            questions(),
            "决策模型未配置 API Key，请在设置中补全后重试。",
            "missing_key",
        ),
        (
            KEY,
            questions(),
            "决策请求无法构建，请检查模型和 API 配置后重试。",
            "request_build",
        ),
    ] {
        let (result, records) =
            evaluate(key, request, Duration::from_secs(1), invalid_url.into()).await;
        assert_eq!(result, Err(error.to_string()));
        let message = only_warning(&records, "decision_model");
        assert!(
            message.to_ascii_lowercase().contains("provider=typesafe"),
            "{message}"
        );
        assert!(
            message.contains(error) || message.contains(category),
            "{message}"
        );
    }
    for (body, error, category) in [
        (
            b"private-body-sentinel".as_slice(),
            "决策响应格式无效，请检查模型配置后重试。",
            "invalid_json",
        ),
        (
            b"{}".as_slice(),
            "决策响应缺少有效的判断结果，请检查模型配置后重试。",
            "invalid_answers",
        ),
    ] {
        let (endpoint, server) = spawn_json_server("200 OK", body).await;
        let (result, records) = evaluate(KEY, questions(), Duration::from_secs(2), endpoint).await;
        server.await.unwrap();
        assert_eq!(result, Err(error.into()));
        let message = only_warning(&records, "decision_model");
        assert!(
            message.contains(error) || message.contains(category),
            "{message}"
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (result, records) = evaluate(KEY, questions(), Duration::from_millis(30), endpoint).await;
    let error = "决策服务响应超时（0.03 秒），请检查网络或稍后重试。";
    assert_eq!(result, Err(error.into()));
    let message = only_warning(&records, "decision_model");
    assert!(
        message.contains(error) || message.contains("timeout"),
        "{message}"
    );
    drop(listener);
    let (url, server) = response_server(b"INVALID HTTP RESPONSE\r\n\r\n".to_vec()).await;
    let endpoint = url.split_once('?').unwrap().0.to_string();
    let (result, records) = evaluate(KEY, questions(), Duration::from_secs(2), endpoint).await;
    server.await.unwrap();
    let error = "决策服务连接失败，请检查网络和 API 地址后重试。";
    assert_eq!(result, Err(error.into()));
    let message = only_warning(&records, "decision_model");
    assert!(
        message.contains(error) || message.contains("transport"),
        "{message}"
    );
    let (result, records) =
        evaluate("", json!({}), Duration::from_secs(1), invalid_url.into()).await;
    assert_eq!(result, Ok(None));
    assert!(records.is_empty(), "{records:?}");
    let body = br#"{"answers":{"screen":{"type":"choice","choice":"needed","probabilities":{"needed":0.5,"unneeded":0.3,"uncertain":0.2}}}}"#;
    let (endpoint, server) = spawn_json_server("200 OK", body).await;
    let (result, records) = evaluate(KEY, questions(), Duration::from_secs(2), endpoint).await;
    server.await.unwrap();
    assert_eq!(result, Ok(Some(serde_json::from_slice(body).unwrap())));
    assert!(records.is_empty(), "{records:?}");
}

pub(super) async fn response_server(reply: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/private-path-sentinel?q=private-query-sentinel",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut input = [0; 2048];
        assert!(socket.read(&mut input).await.unwrap() > 0);
        socket.write_all(&reply).await.unwrap();
    });
    (url, server)
}

#[tokio::test]
async fn decision_model_diagnostic_response_read_preserves_error() {
    let (url, server) = response_server(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nprivate-body-sentinel".to_vec()).await;
    let endpoint = url.split_once('?').unwrap().0;
    let (result, records) = capture_warning(task(endpoint)).await;
    server.await.unwrap();
    let error = "决策响应读取失败，请检查网络后重试。";
    assert_eq!(result, Err(error.into()));
    let warning = only_warning(&records, "decision_model");
    assert!(
        warning.contains(error) || warning.contains("response_read"),
        "{warning}"
    );
}
