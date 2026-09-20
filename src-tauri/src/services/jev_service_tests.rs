use super::jev_service::{self, JevProvider};
use crate::services::ai_polish_service::{evaluate_polish_gate, PolishOutcome, PolishOverrides};
use crate::state::user_profile::UserProfile;
use crate::state::{user_profile::PolishStructureLevel, AppState};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

const ORIGINAL: &str = "原始听写内容，不应被 gate 改写";
const POLICY_CONTEXT: &str = "按当前润色策略判断原文是否已经满足要求";
const TEST_API_KEY: &str = "jev-test-key";

struct CapturedRequest {
    method: String,
    path: String,
    headers: String,
    body: Value,
}

fn header_value(headers: &str, name: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> CapturedRequest {
    let mut request_bytes = Vec::new();
    let mut chunk = [0_u8; 2048];
    let headers_end = loop {
        let count = stream
            .read(&mut chunk)
            .await
            .expect("mock server should read request headers");
        assert!(count > 0, "request ended before headers completed");
        request_bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = find_subsequence(&request_bytes, b"\r\n\r\n") {
            break position + 4;
        }
    };

    let headers = String::from_utf8(request_bytes[..headers_end].to_vec())
        .expect("request headers should be valid UTF-8");
    let content_length = header_value(&headers, "content-length")
        .expect("request should include content length")
        .parse::<usize>()
        .expect("content length should be numeric");
    while request_bytes.len() < headers_end + content_length {
        let count = stream
            .read(&mut chunk)
            .await
            .expect("mock server should read request body");
        assert!(count > 0, "request ended before body completed");
        request_bytes.extend_from_slice(&chunk[..count]);
    }

    let request_line = headers.lines().next().expect("request should have a line");
    let mut request_line_parts = request_line.split_whitespace();
    let method = request_line_parts
        .next()
        .expect("request should have a method")
        .to_string();
    let path = request_line_parts
        .next()
        .expect("request should have a path")
        .to_string();
    let body =
        serde_json::from_slice::<Value>(&request_bytes[headers_end..headers_end + content_length])
            .expect("request body should be JSON");

    CapturedRequest {
        method,
        path,
        headers,
        body,
    }
}

async fn spawn_json_server(
    status: &str,
    response: &'static [u8],
) -> (String, tokio::task::JoinHandle<CapturedRequest>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("mock server should bind");
    let address = listener
        .local_addr()
        .expect("mock server should expose an address");
    let status = status.to_string();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("mock server should accept");
        let captured = read_request(&mut stream).await;
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        stream
            .write_all(headers.as_bytes())
            .await
            .expect("mock server should write response headers");
        stream
            .write_all(response)
            .await
            .expect("mock server should write response body");
        captured
    });

    (format!("http://{address}"), handle)
}

async fn spawn_counting_json_server(
    status: &str,
    response: &'static [u8],
) -> (
    String,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<CapturedRequest>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("counting mock server should bind");
    let address = listener
        .local_addr()
        .expect("counting mock server should expose an address");
    let status = status.to_string();
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_task = Arc::clone(&request_count);

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("counting mock server should accept");
        request_count_for_task.fetch_add(1, Ordering::SeqCst);
        let captured = read_request(&mut stream).await;
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        stream
            .write_all(headers.as_bytes())
            .await
            .expect("counting mock server should write response headers");
        stream
            .write_all(response)
            .await
            .expect("counting mock server should write response body");
        captured
    });

    (format!("http://{address}"), request_count, handle)
}

async fn spawn_stalled_server() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("stalled mock server should bind");
    let address = listener
        .local_addr()
        .expect("stalled mock server should expose an address");
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_task = Arc::clone(&request_count);

    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            request_count_for_task.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                drop(stream);
            });
        }
    });

    (format!("http://{address}"), request_count, handle)
}

fn gate_state(
    provider: JevProvider,
    enabled: bool,
    translation_target: Option<&str>,
    custom_prompt: Option<&str>,
    structure_level: PolishStructureLevel,
) -> AppState {
    let state = AppState::new();
    state.update_profile_mut(|profile| {
        profile.jev.enabled = enabled;
        profile.jev.provider = provider;
        profile.translation_target = translation_target.map(str::to_string);
        profile.custom_prompt = custom_prompt.map(str::to_string);
        profile.polish_structure_level = structure_level;
    });
    state
}

fn enabled_gate_overrides() -> PolishOverrides {
    PolishOverrides {
        allow_jev_gate: true,
        ..PolishOverrides::default()
    }
}

async fn assert_no_gate_request(
    server: &mut tokio::task::JoinHandle<CapturedRequest>,
    request_count: &Arc<AtomicUsize>,
) {
    assert!(timeout(Duration::from_millis(100), &mut *server)
        .await
        .is_err());
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
    server.abort();
}

fn request_body(request: &reqwest::Request) -> Value {
    serde_json::from_slice(
        request
            .body()
            .and_then(|body| body.as_bytes())
            .expect("Jev request should carry a JSON body"),
    )
    .expect("Jev request body should be JSON")
}

fn assert_common_request_contract(request: &reqwest::Request) {
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(
        request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value: &reqwest::header::HeaderValue| value.to_str().ok()),
        Some("Bearer jev-test-key")
    );
    let body = request_body(request);
    assert!(
        body.get("state").is_some(),
        "request must include the structured state object"
    );
    assert_eq!(
        body["state"],
        json!({"text": ORIGINAL, "polishing_policy": POLICY_CONTEXT})
    );
    assert_eq!(body["questions"]["route"]["type"], json!("choice"));
    let criteria = body["questions"]["route"]["criteria"]
        .as_object()
        .expect("route criteria should be an object");
    assert_eq!(criteria.len(), 3);
    for key in ["pass", "polish", "uncertain"] {
        assert!(criteria.contains_key(key), "missing route criterion {key}");
    }
}

#[test]
fn profile_jev_defaults_off_and_contains_no_provider_secrets() {
    let profile = UserProfile::default();
    assert!(!profile.jev.enabled, "Jev must be opt-in by default");
    assert_eq!(profile.jev.provider, JevProvider::TypeSafe);

    let serialized = serde_json::to_value(profile).expect("profile should serialize");
    let jev = serialized
        .get("jev")
        .expect("profile should contain the nested Jev config");
    assert_eq!(jev.get("enabled"), Some(&json!(false)));
    assert_eq!(jev.get("provider"), Some(&json!("typesafe")));
    for secret_name in [
        "api_key",
        "typesafe_api_key",
        "openrouter_api_key",
        "vercel_api_key",
    ] {
        assert!(
            jev.get(secret_name).is_none(),
            "provider secret {secret_name} must not be serialized in the profile"
        );
    }
}

#[test]
fn build_request_matches_typesafe_official_contract() {
    let request = jev_service::build_request(
        &reqwest::Client::new(),
        JevProvider::TypeSafe,
        TEST_API_KEY,
        ORIGINAL,
        POLICY_CONTEXT,
        None,
    )
    .expect("TypeSafe request should build");

    assert_eq!(
        request.url().as_str(),
        "https://api.typesafe.ai/v1/systemone"
    );
    assert_common_request_contract(&request);
    assert_eq!(request_body(&request)["model"], json!("jev-1.13.0"));
}

#[test]
fn build_request_matches_openrouter_alpha_contract() {
    let request = jev_service::build_request(
        &reqwest::Client::new(),
        JevProvider::OpenRouter,
        TEST_API_KEY,
        ORIGINAL,
        POLICY_CONTEXT,
        None,
    )
    .expect("OpenRouter request should build");

    assert_eq!(
        request.url().as_str(),
        "https://openrouter.ai/api/alpha/decisions"
    );
    assert_common_request_contract(&request);
    assert_eq!(request_body(&request)["model"], json!("typesafe/jev-1.13"));
}

#[test]
fn build_request_matches_vercel_evaluation_model_contract() {
    let request = jev_service::build_request(
        &reqwest::Client::new(),
        JevProvider::Vercel,
        TEST_API_KEY,
        ORIGINAL,
        POLICY_CONTEXT,
        None,
    )
    .expect("Vercel request should build");

    assert_eq!(
        request.url().as_str(),
        "https://ai-gateway.vercel.sh/v4/ai/evaluation-model"
    );
    assert_common_request_contract(&request);
    assert!(
        request_body(&request).get("model").is_none(),
        "Vercel must select Jev through ai-model-id rather than a model body field"
    );
    assert_eq!(
        request
            .headers()
            .get(reqwest::header::HeaderName::from_static("ai-model-id"))
            .and_then(|value: &reqwest::header::HeaderValue| value.to_str().ok()),
        Some("typesafe-ai/jev")
    );
    assert_eq!(
        request
            .headers()
            .get(reqwest::header::HeaderName::from_static(
                "ai-evaluation-model-specification-version",
            ))
            .and_then(|value: &reqwest::header::HeaderValue| value.to_str().ok()),
        Some("4")
    );
    assert_eq!(
        request
            .headers()
            .get(reqwest::header::HeaderName::from_static(
                "ai-gateway-protocol-version",
            ))
            .and_then(|value: &reqwest::header::HeaderValue| value.to_str().ok()),
        Some("0.0.1")
    );
}

#[test]
fn parse_decision_accepts_pass_at_the_ninety_percent_boundary() {
    let response = json!({
        "answers": {
            "route": {
                "type": "choice",
                "choice": "pass",
                "probabilities": {"pass": 0.90, "polish": 0.10, "uncertain": 0.0}
            }
        }
    });

    assert!(jev_service::parse_decision(&response.to_string()));
}

#[test]
fn parse_decision_fails_conservatively_for_invalid_or_uncertain_results() {
    let responses = [
        json!({"answers":{"route":{"type":"choice","choice":"polish","probabilities":{"pass":0.99,"polish":0.01,"uncertain":0.0}}}}),
        json!({"answers":{"route":{"type":"choice","choice":"unknown","probabilities":{"pass":0.99,"polish":0.01,"uncertain":0.0}}}}),
        json!({"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.899,"polish":0.101,"uncertain":0.0}}}}),
        json!({"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":1.01,"polish":-0.01,"uncertain":0.0}}}}),
        json!({"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.95,"polish":0.04,"uncertain":0.50}}}}),
        json!({"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.99,"polish":0.01}}}}),
        json!({"answers":{"route":{"type":"freeform","choice":"pass","probabilities":{"pass":0.99,"polish":0.01,"uncertain":0.0}}}}),
    ];

    for response in responses {
        assert!(
            !jev_service::parse_decision(&response.to_string()),
            "invalid or uncertain Jev result must continue normal polish: {response}"
        );
    }
    assert!(!jev_service::parse_decision(""));
    assert!(!jev_service::parse_decision("not json"));
}

#[tokio::test]
async fn evaluate_calls_the_selected_provider_and_accepts_a_valid_pass() {
    let response = br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.95,"polish":0.04,"uncertain":0.01}}}}"#;
    let (endpoint, server) = spawn_json_server("200 OK", response).await;

    let should_skip = jev_service::evaluate(
        &reqwest::Client::new(),
        JevProvider::OpenRouter,
        TEST_API_KEY,
        ORIGINAL,
        POLICY_CONTEXT,
        Some(endpoint),
    )
    .await;
    let captured = server.await.expect("mock server should finish");

    assert!(
        should_skip,
        "a confident pass should skip the polish request"
    );
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/alpha/decisions");
    assert_eq!(
        header_value(&captured.headers, "authorization").as_deref(),
        Some("Bearer jev-test-key")
    );
    assert_eq!(captured.body["model"], json!("typesafe/jev-1.13"));
}

#[tokio::test]
async fn evaluate_falls_back_for_http200_malformed_nonpass_and_uncertain_results() {
    let cases: [(&str, &'static [u8]); 3] = [
        ("malformed", br#"not json"#),
        (
            "nonpass",
            br#"{"answers":{"route":{"type":"choice","choice":"polish","probabilities":{"pass":0.04,"polish":0.95,"uncertain":0.01}}}}"#,
        ),
        (
            "uncertain",
            br#"{"answers":{"route":{"type":"choice","choice":"uncertain","probabilities":{"pass":0.05,"polish":0.05,"uncertain":0.90}}}}"#,
        ),
    ];

    for (label, response) in cases {
        let (endpoint, server) = spawn_json_server("200 OK", response).await;
        let should_skip = jev_service::evaluate(
            &reqwest::Client::new(),
            JevProvider::TypeSafe,
            TEST_API_KEY,
            ORIGINAL,
            POLICY_CONTEXT,
            Some(endpoint),
        )
        .await;
        let captured = server.await.expect("mock server should finish");

        assert!(
            !should_skip,
            "{label} Jev response must continue normal polish"
        );
        assert_eq!(captured.path, "/v1/systemone");
    }
}

#[tokio::test]
async fn evaluate_returns_false_on_provider_http_errors() {
    let (endpoint, server) =
        spawn_json_server("503 Service Unavailable", br#"{"error":"busy"}"#).await;

    let should_skip = jev_service::evaluate(
        &reqwest::Client::new(),
        JevProvider::TypeSafe,
        TEST_API_KEY,
        ORIGINAL,
        POLICY_CONTEXT,
        Some(endpoint),
    )
    .await;
    let captured = server.await.expect("mock server should finish");

    assert!(
        !should_skip,
        "HTTP errors must fall back to ordinary polish"
    );
    assert_eq!(captured.path, "/v1/systemone");
}

#[test]
fn build_request_instructions_judge_the_original_without_generating_output() {
    let request = jev_service::build_request(
        &reqwest::Client::new(),
        JevProvider::TypeSafe,
        TEST_API_KEY,
        ORIGINAL,
        POLICY_CONTEXT,
        Some("http://127.0.0.1:40123"),
    )
    .expect("TypeSafe request should build");
    let body = request_body(&request);
    let instructions = body["questions"]["route"]["instructions"]
        .as_str()
        .expect("route instructions should be a string");
    let normalized = instructions.to_ascii_lowercase();

    assert!(!instructions.trim().is_empty());
    assert!(
        normalized.contains("judge"),
        "instructions must ask Jev to judge the original"
    );
    assert!(
        normalized.contains("not generate")
            || normalized.contains("do not generate")
            || normalized.contains("do not rewrite")
            || instructions.contains("不要生成")
            || instructions.contains("不要改写"),
        "instructions must forbid generating replacement output"
    );
}

#[tokio::test]
async fn evaluate_polish_gate_requires_explicit_opt_in_and_preserves_exact_passthrough() {
    let state = gate_state(
        JevProvider::TypeSafe,
        true,
        None,
        None,
        PolishStructureLevel::Off,
    );
    let response = br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.95,"polish":0.04,"uncertain":0.01}}}}"#;
    let (endpoint, count, mut server) = spawn_counting_json_server("200 OK", response).await;
    let default_result: Option<PolishOutcome> = evaluate_polish_gate(
        &state,
        "  原始文本  \n",
        &PolishOverrides::default(),
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint),
    )
    .await;

    assert!(
        default_result.is_none(),
        "allow_jev_gate must default to false"
    );
    assert_no_gate_request(&mut server, &count).await;

    let disabled_state = gate_state(
        JevProvider::TypeSafe,
        false,
        None,
        None,
        PolishStructureLevel::Off,
    );
    let (endpoint, count, mut server) = spawn_counting_json_server("200 OK", response).await;
    let result: Option<PolishOutcome> = evaluate_polish_gate(
        &disabled_state,
        "  原始文本  \n",
        &enabled_gate_overrides(),
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint),
    )
    .await;
    assert!(result.is_none(), "a disabled profile gate must not run");
    assert_no_gate_request(&mut server, &count).await;

    let (endpoint, count, server) = spawn_counting_json_server("200 OK", response).await;
    let overrides = enabled_gate_overrides();
    let original = "  原始文本  \n";
    let result = evaluate_polish_gate(
        &state,
        original,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint),
    )
    .await
    .expect("a confident pass should produce a passthrough outcome");
    let captured = server.await.expect("mock server should finish");

    assert_eq!(result.text, original);
    assert!(!result.executed);
    assert!(result.provider.is_none());
    assert!(result.model.is_none());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(captured.path, "/v1/systemone");
}

#[tokio::test]
async fn evaluate_polish_gate_bypasses_required_execution_provider_mismatch_and_empty_key() {
    let state = gate_state(
        JevProvider::TypeSafe,
        true,
        None,
        None,
        PolishStructureLevel::Off,
    );
    let cases = [
        (
            "require_execution",
            true,
            JevProvider::TypeSafe,
            TEST_API_KEY,
        ),
        (
            "provider_mismatch",
            false,
            JevProvider::OpenRouter,
            TEST_API_KEY,
        ),
        ("empty_key", false, JevProvider::TypeSafe, ""),
    ];

    for (label, require_execution, provider, api_key) in cases {
        let (endpoint, count, mut server) = spawn_counting_json_server(
            "200 OK",
            br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.99,"polish":0.01,"uncertain":0.0}}}}"#,
        )
        .await;
        let mut overrides = enabled_gate_overrides();
        overrides.require_execution = require_execution;
        let result: Option<PolishOutcome> = evaluate_polish_gate(
            &state,
            ORIGINAL,
            &overrides,
            provider,
            api_key,
            Some(endpoint),
        )
        .await;

        assert!(result.is_none(), "{label} must bypass the Jev gate");
        assert_no_gate_request(&mut server, &count).await;
    }
}

#[tokio::test]
async fn evaluate_polish_gate_bypasses_all_effective_translation_modes() {
    let cases = [
        (Some("German"), None),
        (None, Some(Some("English".to_string()))),
    ];

    for (global_translation, override_translation) in cases {
        let state = gate_state(
            JevProvider::TypeSafe,
            true,
            global_translation,
            None,
            PolishStructureLevel::Off,
        );
        let (endpoint, count, mut server) = spawn_counting_json_server(
            "200 OK",
            br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.99,"polish":0.01,"uncertain":0.0}}}}"#,
        )
        .await;
        let mut overrides = enabled_gate_overrides();
        overrides.translation_target = override_translation;
        let result: Option<PolishOutcome> = evaluate_polish_gate(
            &state,
            ORIGINAL,
            &overrides,
            JevProvider::TypeSafe,
            TEST_API_KEY,
            Some(endpoint),
        )
        .await;

        assert!(result.is_none(), "effective translation must bypass Jev");
        assert_no_gate_request(&mut server, &count).await;
    }
}

#[tokio::test]
async fn evaluate_polish_gate_allows_gate_when_translation_override_disables_global_translation() {
    let state = gate_state(
        JevProvider::TypeSafe,
        true,
        Some("German"),
        None,
        PolishStructureLevel::Off,
    );
    let (endpoint, count, server) = spawn_counting_json_server(
        "200 OK",
        br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.99,"polish":0.01,"uncertain":0.0}}}}"#,
    )
    .await;
    let mut overrides = enabled_gate_overrides();
    overrides.translation_target = Some(None);
    let original = "  原文不需要翻译  ";
    let result: Option<PolishOutcome> = evaluate_polish_gate(
        &state,
        original,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint),
    )
    .await;
    let result = result.expect("disabling global translation should leave Jev eligible");
    let captured = server.await.expect("mock server should finish");

    assert_eq!(result.text, original);
    assert!(!result.executed);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(captured.path, "/v1/systemone");
}

#[tokio::test]
async fn evaluate_polish_gate_includes_custom_strong_policy_and_app_context() {
    let state = gate_state(
        JevProvider::TypeSafe,
        true,
        None,
        Some("custom policy marker"),
        PolishStructureLevel::Strong,
    );
    let (endpoint, count, server) = spawn_counting_json_server(
        "200 OK",
        br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.95,"polish":0.04,"uncertain":0.01}}}}"#,
    )
    .await;
    let mut overrides = enabled_gate_overrides();
    overrides.app_context = Some("app context marker".to_string());
    let original = "  保留原始空白  ";
    let result = evaluate_polish_gate(
        &state,
        original,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint),
    )
    .await
    .expect("confident pass should produce a passthrough outcome");
    let captured = server.await.expect("mock server should finish");
    let policy = captured.body["state"]["polishing_policy"]
        .as_str()
        .expect("Jev state should include polishing policy");

    assert_eq!(result.text, original);
    assert!(!result.executed);
    assert!(policy.contains("custom policy marker"));
    assert!(policy.contains("app context marker"));
    assert!(policy.contains("level=\"strong\""));
    assert!(
        !policy.contains("<examples>"),
        "Jev policy must not include the built-in polish examples"
    );
    assert!(
        !policy.contains("<output_format>"),
        "Jev policy must not include the polish output schema"
    );
    assert!(
        !policy.contains("输出必须是一个符合"),
        "Jev policy must not retain the JSON-only invariant"
    );
    assert!(
        !policy.contains("只输出指定 JSON 对象"),
        "Jev policy must not require JSON-only output"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn evaluate_polish_gate_returns_none_for_low_confidence_and_http_errors() {
    let cases: [(&str, &str, &'static [u8]); 2] = [
        (
            "low confidence",
            "200 OK",
            br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.89,"polish":0.10,"uncertain":0.01}}}}"#,
        ),
        ("HTTP error", "503 Service Unavailable", br#"{"error":"busy"}"#),
    ];

    for (label, status, response) in cases {
        let state = gate_state(
            JevProvider::TypeSafe,
            true,
            None,
            None,
            PolishStructureLevel::Off,
        );
        let (endpoint, _count, server) = spawn_counting_json_server(status, response).await;
        let overrides = enabled_gate_overrides();
        let result: Option<PolishOutcome> = evaluate_polish_gate(
            &state,
            ORIGINAL,
            &overrides,
            JevProvider::TypeSafe,
            TEST_API_KEY,
            Some(endpoint),
        )
        .await;
        let _ = server.await.expect("mock server should finish");

        assert!(result.is_none(), "{label} must continue normal polish");
    }
}

#[tokio::test]
async fn evaluate_stalled_body_times_out_without_retrying() {
    let (endpoint, request_count, server) = spawn_stalled_server().await;
    let started = Instant::now();
    let should_skip: bool = timeout(
        Duration::from_secs(2),
        jev_service::evaluate(
            &reqwest::Client::new(),
            JevProvider::TypeSafe,
            TEST_API_KEY,
            ORIGINAL,
            POLICY_CONTEXT,
            Some(endpoint),
        ),
    )
    .await
    .expect("stalled Jev evaluation must honor the bounded deadline");

    assert!(!should_skip);
    assert!(started.elapsed() < Duration::from_millis(1_400));
    assert_eq!(
        request_count.load(Ordering::SeqCst),
        1,
        "timeouts must not retry"
    );
    server.abort();
}
