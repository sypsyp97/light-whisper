use super::{evaluate_polish_expansion, PolishOverrides};
use crate::services::jev_service::JevProvider;
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::timeout;

const ORIGINAL: &str = "原始听写内容";
const TEST_API_KEY: &str = "jev-routing-test-key";

const PASS_AND_SCREEN_UNNEEDED: &[u8] = br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.95,"polish":0.04,"uncertain":0.01}},"screen":{"type":"choice","choice":"unneeded","probabilities":{"needed":0.04,"unneeded":0.95,"uncertain":0.01}}}}"#;
const PASS_AND_SCREEN_NEEDED: &[u8] = br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.95,"polish":0.04,"uncertain":0.01}},"screen":{"type":"choice","choice":"needed","probabilities":{"needed":0.95,"unneeded":0.04,"uncertain":0.01}}}}"#;
const POLISH_AND_SCREEN_UNNEEDED: &[u8] = br#"{"answers":{"route":{"type":"choice","choice":"polish","probabilities":{"pass":0.04,"polish":0.95,"uncertain":0.01}},"screen":{"type":"choice","choice":"unneeded","probabilities":{"needed":0.04,"unneeded":0.95,"uncertain":0.01}}}}"#;
const SCREEN_UNNEEDED: &[u8] = br#"{"answers":{"screen":{"type":"choice","choice":"unneeded","probabilities":{"needed":0.04,"unneeded":0.95,"uncertain":0.01}}}}"#;

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
            .expect("local routing server should read request headers");
        assert!(count > 0, "request ended before headers completed");
        request_bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = find_subsequence(&request_bytes, b"\r\n\r\n") {
            break position + 4;
        }
    };

    let headers = String::from_utf8(request_bytes[..headers_end].to_vec())
        .expect("request headers should be UTF-8");
    let content_length = header_value(&headers, "content-length")
        .expect("request should include content length")
        .parse::<usize>()
        .expect("content length should be numeric");
    while request_bytes.len() < headers_end + content_length {
        let count = stream
            .read(&mut chunk)
            .await
            .expect("local routing server should read request body");
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
) -> (
    String,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<CapturedRequest>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("local routing server should bind");
    let address = listener
        .local_addr()
        .expect("local routing server should expose an address");
    let status = status.to_string();
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_task = Arc::clone(&request_count);

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("local routing server should accept");
        request_count_for_task.fetch_add(1, Ordering::SeqCst);
        let captured = read_request(&mut stream).await;
        let response_headers = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        stream
            .write_all(response_headers.as_bytes())
            .await
            .expect("local routing server should write response headers");
        stream
            .write_all(response)
            .await
            .expect("local routing server should write response body");
        captured
    });

    (format!("http://{address}"), request_count, handle)
}

async fn finish_json_server(server: tokio::task::JoinHandle<CapturedRequest>) -> CapturedRequest {
    timeout(Duration::from_secs(1), server)
        .await
        .expect("routing server should receive one request")
        .expect("routing server should finish")
}

async fn spawn_counting_server() -> (
    String,
    Arc<AtomicUsize>,
    oneshot::Receiver<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("counting routing server should bind");
    let address = listener
        .local_addr()
        .expect("counting routing server should expose an address");
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_task = Arc::clone(&request_count);
    let (request_seen_tx, request_seen_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        request_count_for_task.fetch_add(1, Ordering::SeqCst);
        let _ = request_seen_tx.send(());
        let _ = read_request(&mut stream).await;
    });

    (
        format!("http://{address}"),
        request_count,
        request_seen_rx,
        handle,
    )
}

async fn spawn_stalled_body_server() -> (
    String,
    Arc<AtomicUsize>,
    oneshot::Receiver<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("stalled routing server should bind");
    let address = listener
        .local_addr()
        .expect("stalled routing server should expose an address");
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_task = Arc::clone(&request_count);
    let (request_seen_tx, request_seen_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        request_count_for_task.fetch_add(1, Ordering::SeqCst);
        let _ = read_request(&mut stream).await;
        let partial_response =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\nConnection: close\r\n\r\n{\"partial\":";
        stream
            .write_all(partial_response)
            .await
            .expect("stalled routing server should write partial body");
        let _ = request_seen_tx.send(());
        std::future::pending::<()>().await;
    });

    (
        format!("http://{address}"),
        request_count,
        request_seen_rx,
        handle,
    )
}

fn expansion_state(route_enabled: bool, screen_routing: bool) -> AppState {
    let state = AppState::new();
    state.update_profile_mut(|profile| {
        profile.jev.enabled = route_enabled;
        profile.jev.provider = JevProvider::TypeSafe;
        profile.jev.screen_routing = screen_routing;
        profile.custom_prompt = Some("policy marker".to_string());
    });
    state
}

fn expansion_overrides(screen_requested: bool, explicit: bool) -> PolishOverrides {
    PolishOverrides {
        allow_jev_gate: true,
        screen_context_enabled: Some(screen_requested),
        screen_context_explicit: explicit,
        ..PolishOverrides::default()
    }
}

fn assert_common_request(captured: &CapturedRequest) {
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/systemone");
    assert_eq!(
        header_value(&captured.headers, "authorization").as_deref(),
        Some("Bearer jev-routing-test-key")
    );
    assert_eq!(captured.body["state"]["text"], json!(ORIGINAL));
    assert!(captured.body["state"]["polishing_policy"]
        .as_str()
        .is_some_and(|policy| policy.contains("policy marker")));
}

fn question_ids(body: &Value) -> Vec<String> {
    let mut ids = body["questions"]
        .as_object()
        .expect("request should contain question object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[tokio::test]
async fn partial_screen_answer_cannot_bypass_polishing() {
    let state = expansion_state(true, true);
    let overrides = expansion_overrides(true, false);
    let response = br#"{"answers":{"route":{"type":"choice","choice":"pass","probabilities":{"pass":0.95,"polish":0.04,"uncertain":0.01}}}}"#;
    let (endpoint, count, server) = spawn_json_server("200 OK", response).await;
    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await
    .expect("eligible routing should return a fallback decision");
    finish_json_server(server).await;
    assert!(!decision.skip_polish);
    assert_eq!(decision.screen_allowed, None);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn auto_route_and_screen_share_one_request_and_preserve_policy() {
    let state = expansion_state(true, true);
    let overrides = expansion_overrides(true, false);
    let (endpoint, request_count, server) =
        spawn_json_server("200 OK", PASS_AND_SCREEN_UNNEEDED).await;

    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await
    .expect("eligible auto routing should return a decision");
    let captured = finish_json_server(server).await;

    assert!(decision.skip_polish);
    assert_eq!(decision.screen_allowed, Some(false));
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
    assert_common_request(&captured);
    assert_eq!(question_ids(&captured.body), vec!["route", "screen"]);
}

#[tokio::test]
async fn route_pass_with_screen_needed_keeps_normal_processing_and_screen_context() {
    let state = expansion_state(true, true);
    let overrides = expansion_overrides(true, false);
    let (endpoint, request_count, server) =
        spawn_json_server("200 OK", PASS_AND_SCREEN_NEEDED).await;

    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await
    .expect("eligible routing should return a decision");
    let captured = finish_json_server(server).await;

    assert!(!decision.skip_polish);
    assert_eq!(decision.screen_allowed, Some(true));
    assert_eq!(question_ids(&captured.body), vec!["route", "screen"]);
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn polish_on_with_screen_auto_asks_only_screen_and_never_skips() {
    let state = expansion_state(false, true);
    let overrides = expansion_overrides(true, false);
    let (endpoint, request_count, server) =
        spawn_json_server("200 OK", POLISH_AND_SCREEN_UNNEEDED).await;

    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await
    .expect("screen auto should return a decision when polish is on");
    let captured = finish_json_server(server).await;

    assert!(!decision.skip_polish);
    assert_eq!(decision.screen_allowed, Some(false));
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
    assert_eq!(question_ids(&captured.body), vec!["screen"]);
}

#[tokio::test]
async fn translation_auto_asks_only_screen_and_never_skips_polish() {
    let state = expansion_state(true, true);
    let mut overrides = expansion_overrides(true, false);
    overrides.translation_target = Some(Some("German".to_string()));
    let (endpoint, request_count, server) = spawn_json_server("200 OK", SCREEN_UNNEEDED).await;

    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await
    .expect("translation screen auto should return a decision");
    let captured = finish_json_server(server).await;

    assert!(!decision.skip_polish);
    assert_eq!(decision.screen_allowed, Some(false));
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
    assert_eq!(question_ids(&captured.body), vec!["screen"]);
}

#[tokio::test]
async fn screen_request_false_omits_screen_question_and_all_gates_skip_network() {
    let state = expansion_state(true, true);
    let overrides = expansion_overrides(false, false);
    let (endpoint, request_count, server) =
        spawn_json_server("200 OK", POLISH_AND_SCREEN_UNNEEDED).await;
    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await
    .expect("route auto should remain eligible without a requested screen");
    let captured = finish_json_server(server).await;

    assert!(!decision.skip_polish);
    assert_eq!(decision.screen_allowed, None);
    assert_eq!(question_ids(&captured.body), vec!["route"]);
    assert_eq!(request_count.load(Ordering::SeqCst), 1);

    let state = expansion_state(false, false);
    let (endpoint, request_count, mut request_seen, server) = spawn_counting_server().await;
    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await;

    assert!(decision.is_none());
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
    assert!(matches!(
        request_seen.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    server.abort();
}

#[tokio::test]
async fn explicit_screen_request_keeps_screen_enabled_after_unneeded_answer() {
    let state = expansion_state(false, true);
    let overrides = expansion_overrides(true, true);
    let (endpoint, request_count, server) = spawn_json_server("200 OK", SCREEN_UNNEEDED).await;

    let decision = evaluate_polish_expansion(
        &state,
        ORIGINAL,
        &overrides,
        JevProvider::TypeSafe,
        TEST_API_KEY,
        Some(endpoint.as_str()),
    )
    .await
    .expect("explicit screen request should remain eligible");
    let captured = finish_json_server(server).await;

    assert!(!decision.skip_polish);
    assert_eq!(decision.screen_allowed, Some(true));
    assert_eq!(question_ids(&captured.body), vec!["screen"]);
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn malformed_or_stalled_response_falls_back_without_skip_or_duplicate_call() {
    let cases = [("malformed", "200 OK", br#"not json"#.as_slice())];
    for (label, status, response) in cases {
        let state = expansion_state(true, true);
        let overrides = expansion_overrides(true, false);
        let (endpoint, request_count, server) = spawn_json_server(status, response).await;
        let decision = evaluate_polish_expansion(
            &state,
            ORIGINAL,
            &overrides,
            JevProvider::TypeSafe,
            TEST_API_KEY,
            Some(endpoint.as_str()),
        )
        .await
        .expect("eligible malformed response should preserve baseline decision");
        let _captured = finish_json_server(server).await;

        assert!(
            !decision.skip_polish,
            "{label} response must not skip polish"
        );
        assert_eq!(decision.screen_allowed, None);
        assert_eq!(request_count.load(Ordering::SeqCst), 1);
    }

    let state = expansion_state(true, true);
    let overrides = expansion_overrides(true, false);
    let (endpoint, request_count, request_seen, server) = spawn_stalled_body_server().await;
    let started = Instant::now();
    let decision = timeout(
        Duration::from_secs(2),
        evaluate_polish_expansion(
            &state,
            ORIGINAL,
            &overrides,
            JevProvider::TypeSafe,
            TEST_API_KEY,
            Some(endpoint.as_str()),
        ),
    )
    .await
    .expect("stalled body should honor the bounded expansion deadline")
    .expect("eligible stalled response should preserve baseline decision");
    timeout(Duration::from_millis(250), request_seen)
        .await
        .expect("stalled server should observe one request")
        .expect("stalled request notification should arrive");

    assert!(!decision.skip_polish);
    assert_eq!(decision.screen_allowed, None);
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
    assert!(started.elapsed() < Duration::from_millis(1_800));
    server.abort();
}
