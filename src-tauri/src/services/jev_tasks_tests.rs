use super::jev_service::JevProvider;
use super::jev_tasks;
use crate::state::user_profile::{JevConfig, UserProfile};
use serde_json::{json, Map, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::timeout;

const TEST_API_KEY: &str = "jev-test-key";
const TEST_STATE: &str = "state marker";

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

async fn spawn_counting_server(
    status: &str,
    response: &'static [u8],
) -> (
    String,
    Arc<AtomicUsize>,
    oneshot::Receiver<()>,
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
    let (request_seen_tx, request_seen_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("counting mock server should accept");
        request_count_for_task.fetch_add(1, Ordering::SeqCst);
        let _ = request_seen_tx.send(());
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

    (
        format!("http://{address}"),
        request_count,
        request_seen_rx,
        handle,
    )
}

async fn spawn_stalled_server() -> (
    String,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
    oneshot::Receiver<()>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("stalled mock server should bind");
    let address = listener
        .local_addr()
        .expect("stalled mock server should expose an address");
    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_task = Arc::clone(&request_count);
    let (request_seen_tx, request_seen_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        request_count_for_task.fetch_add(1, Ordering::SeqCst);
        let _captured = read_request(&mut stream).await;
        let partial_response =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\nConnection: close\r\n\r\n{\"partial\":";
        stream
            .write_all(partial_response)
            .await
            .expect("stalled mock server should write partial body");
        let _ = request_seen_tx.send(());
        std::future::pending::<()>().await;
    });

    (
        format!("http://{address}"),
        request_count,
        handle,
        request_seen_rx,
    )
}

fn answer_payload(id: &str, choice: &str, probabilities: &[(&str, f64)]) -> Value {
    let mut probability_map = Map::new();
    for (name, probability) in probabilities {
        probability_map.insert((*name).to_string(), json!(probability));
    }

    let mut answer = Map::new();
    answer.insert("type".to_string(), json!("choice"));
    answer.insert("choice".to_string(), json!(choice));
    answer.insert("probabilities".to_string(), Value::Object(probability_map));

    let mut answers = Map::new();
    answers.insert(id.to_string(), Value::Object(answer));
    json!({"answers": answers})
}

fn assert_choice_question(value: &Value, id: &str, expected_choices: &[&str]) {
    let question = value
        .get(id)
        .unwrap_or_else(|| panic!("question payload should contain {id}"));
    assert_eq!(question.get("type"), Some(&json!("choice")));
    let instructions = question
        .get("instructions")
        .and_then(Value::as_str)
        .expect("choice question should include instructions");
    assert!(!instructions.trim().is_empty());
    let criteria = question
        .get("criteria")
        .and_then(Value::as_object)
        .expect("choice question should include criteria");
    let mut actual = criteria.keys().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = expected_choices.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected);
    for (choice, description) in criteria {
        assert!(!choice.trim().is_empty());
        assert!(!description
            .as_str()
            .expect("choice criterion should be text")
            .trim()
            .is_empty());
    }
}

fn assert_exact_question_ids(value: &Value, expected_ids: &[&str]) {
    let object = value
        .as_object()
        .expect("question payload should be an object");
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = expected_ids.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

async fn assert_no_request(
    server: &mut tokio::task::JoinHandle<CapturedRequest>,
    request_count: &Arc<AtomicUsize>,
    request_seen: &mut oneshot::Receiver<()>,
) {
    tokio::task::yield_now().await;
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
    assert!(matches!(
        request_seen.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    server.abort();
}

#[test]
fn question_builders_expose_the_required_independent_choice_sets() {
    let screen = jev_tasks::screen_questions();
    assert_exact_question_ids(&screen, &["screen"]);
    assert_choice_question(&screen, "screen", &["needed", "unneeded", "uncertain"]);

    let search = jev_tasks::search_questions();
    assert_exact_question_ids(&search, &["search"]);
    assert_choice_question(&search, "search", &["needed", "unneeded", "uncertain"]);

    let audit = jev_tasks::audit_questions();
    assert_exact_question_ids(&audit, &["negation", "numbers", "entities", "intent"]);
    let audit_instructions = ["negation", "numbers", "entities", "intent"]
        .into_iter()
        .map(|id| {
            audit[id]["instructions"]
                .as_str()
                .expect("audit instruction should be text")
                .to_string()
        })
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        audit_instructions.len(),
        4,
        "audit checks should ask distinct questions"
    );
    for id in ["negation", "numbers", "entities", "intent"] {
        assert_choice_question(&audit, id, &["preserved", "changed", "uncertain"]);
    }

    let corrections = jev_tasks::correction_questions(3);
    assert_exact_question_ids(&corrections, &["rule_0", "rule_1", "rule_2"]);
    for id in ["rule_0", "rule_1", "rule_2"] {
        assert_choice_question(&corrections, id, &["valid", "invalid", "uncertain"]);
    }
}

#[test]
fn correction_question_count_zero_has_no_phantom_rule() {
    let questions = jev_tasks::correction_questions(0);
    assert_eq!(questions.as_object().map(Map::len), Some(0));
}

#[test]
fn confident_choice_requires_a_complete_confident_allowed_winner() {
    let choices = ["needed", "unneeded", "uncertain"];
    let valid = answer_payload(
        "screen",
        "unneeded",
        &[("needed", 0.05), ("unneeded", 0.93), ("uncertain", 0.02)],
    );
    assert_eq!(
        jev_tasks::confident_choice(&valid, "screen", &choices, 0.90),
        Some("unneeded".to_string())
    );

    let sum_lower_boundary = answer_payload(
        "screen",
        "unneeded",
        &[("needed", 0.05), ("unneeded", 0.90), ("uncertain", 0.03)],
    );
    let sum_upper_boundary = answer_payload(
        "screen",
        "unneeded",
        &[("needed", 0.05), ("unneeded", 0.90), ("uncertain", 0.07)],
    );
    for payload in [sum_lower_boundary, sum_upper_boundary] {
        assert_eq!(
            jev_tasks::confident_choice(&payload, "screen", &choices, 0.90),
            Some("unneeded".to_string()),
            "a distribution at the 0.02 sum tolerance boundary should remain valid"
        );
    }

    let tie_at_winner = answer_payload(
        "screen",
        "unneeded",
        &[("needed", 0.45), ("unneeded", 0.45), ("uncertain", 0.10)],
    );
    assert_eq!(
        jev_tasks::confident_choice(&tie_at_winner, "screen", &choices, 0.40),
        Some("unneeded".to_string()),
        "an allowed winner may tie another probability when it is greater than or equal"
    );

    let malformed = [
        ("missing answer", json!({"answers": {}})),
        (
            "wrong type",
            json!({"answers":{"screen":{"type":"freeform","choice":"unneeded","probabilities":{"needed":0.05,"unneeded":0.93,"uncertain":0.02}}}}),
        ),
        (
            "unknown winner",
            answer_payload(
                "screen",
                "other",
                &[("needed", 0.05), ("unneeded", 0.93), ("uncertain", 0.02)],
            ),
        ),
        (
            "missing probability",
            answer_payload(
                "screen",
                "unneeded",
                &[("needed", 0.05), ("unneeded", 0.95)],
            ),
        ),
        (
            "extra probability",
            answer_payload(
                "screen",
                "unneeded",
                &[
                    ("needed", 0.04),
                    ("unneeded", 0.94),
                    ("uncertain", 0.01),
                    ("other", 0.01),
                ],
            ),
        ),
        (
            "out of range probability",
            answer_payload(
                "screen",
                "unneeded",
                &[("needed", -0.01), ("unneeded", 1.0), ("uncertain", 0.01)],
            ),
        ),
        (
            "sum outside tolerance",
            answer_payload(
                "screen",
                "unneeded",
                &[("needed", 0.01), ("unneeded", 0.95), ("uncertain", 0.01)],
            ),
        ),
        (
            "sum just below lower boundary",
            answer_payload(
                "screen",
                "unneeded",
                &[("needed", 0.05), ("unneeded", 0.90), ("uncertain", 0.029)],
            ),
        ),
        (
            "sum just above upper boundary",
            answer_payload(
                "screen",
                "unneeded",
                &[("needed", 0.05), ("unneeded", 0.90), ("uncertain", 0.071)],
            ),
        ),
        (
            "winner below threshold",
            answer_payload(
                "screen",
                "unneeded",
                &[("needed", 0.08), ("unneeded", 0.89), ("uncertain", 0.03)],
            ),
        ),
    ];

    for (label, payload) in malformed {
        assert_eq!(
            jev_tasks::confident_choice(&payload, "screen", &choices, 0.90),
            None,
            "malformed or contradictory payload should fail closed: {label}"
        );
    }

    let winner_is_not_highest = answer_payload(
        "screen",
        "unneeded",
        &[("needed", 0.51), ("unneeded", 0.48), ("uncertain", 0.01)],
    );
    assert_eq!(
        jev_tasks::confident_choice(&winner_is_not_highest, "screen", &choices, 0.40),
        None,
        "a choice below another allowed probability must not win"
    );

    let uncertain = answer_payload(
        "screen",
        "uncertain",
        &[("needed", 0.01), ("unneeded", 0.01), ("uncertain", 0.98)],
    );
    assert_eq!(
        jev_tasks::confident_choice(&uncertain, "screen", &choices, 0.90),
        Some("uncertain".to_string()),
        "a confident uncertain answer must remain uncertain for callers to preserve baseline behavior"
    );
}

#[test]
fn screen_and_search_precedence_preserve_explicit_modes_and_fallbacks() {
    assert!(!jev_tasks::screen_allowed(false, false, Some("unneeded")));
    assert!(!jev_tasks::screen_allowed(false, true, Some("unneeded")));
    assert!(jev_tasks::screen_allowed(true, true, Some("unneeded")));
    assert!(!jev_tasks::screen_allowed(true, false, Some("unneeded")));
    assert!(jev_tasks::screen_allowed(true, false, Some("needed")));
    assert!(jev_tasks::screen_allowed(true, false, Some("uncertain")));
    assert!(jev_tasks::screen_allowed(true, false, None));

    assert!(!jev_tasks::search_allowed(
        false,
        Some(true),
        true,
        Some("needed")
    ));
    assert!(!jev_tasks::search_allowed(
        true,
        Some(false),
        true,
        Some("needed")
    ));
    assert!(jev_tasks::search_allowed(
        true,
        Some(true),
        false,
        Some("unneeded")
    ));
    assert!(jev_tasks::search_allowed(true, None, false, Some("needed")));
    assert!(!jev_tasks::search_allowed(
        true,
        None,
        true,
        Some("unneeded")
    ));
    assert!(jev_tasks::search_allowed(
        true,
        None,
        true,
        Some("uncertain")
    ));
    assert!(!jev_tasks::search_allowed(true, None, false, None));
    assert!(jev_tasks::search_allowed(true, None, true, None));
}

#[test]
fn jev_feature_defaults_migrate_legacy_profiles_without_changing_gate_settings() {
    assert!(!JevConfig::default().enabled);

    let legacy = json!({"enabled": true, "provider": "openrouter"});
    let config: JevConfig = serde_json::from_value(legacy).expect("legacy Jev config should load");

    assert!(config.enabled);
    assert_eq!(config.provider, JevProvider::OpenRouter);
    assert!(!config.screen_routing);
    assert!(!config.correction_review);
    assert!(!config.search_routing);
    assert!(!config.polish_audit);

    let profile = UserProfile {
        jev: config.clone(),
        ..UserProfile::default()
    };
    let serialized = serde_json::to_value(profile).expect("profile should serialize");
    let persisted = &serialized["jev"];
    assert_eq!(persisted["enabled"], json!(true));
    assert_eq!(persisted["provider"], json!("openrouter"));
    for field in [
        "screen_routing",
        "correction_review",
        "search_routing",
        "polish_audit",
    ] {
        assert_eq!(
            persisted[field],
            json!(false),
            "new field {field} should default off"
        );
    }

    let mut missing_jev =
        serde_json::to_value(UserProfile::default()).expect("default profile should serialize");
    missing_jev
        .as_object_mut()
        .expect("serialized profile should be an object")
        .remove("jev");
    let migrated_profile: UserProfile =
        serde_json::from_value(missing_jev).expect("profile without Jev should migrate");
    assert!(!migrated_profile.jev.enabled);
    assert_eq!(migrated_profile.jev.provider, JevProvider::TypeSafe);
    assert!(!migrated_profile.jev.screen_routing);
    assert!(!migrated_profile.jev.correction_review);
    assert!(!migrated_profile.jev.search_routing);
    assert!(!migrated_profile.jev.polish_audit);
}

#[test]
fn jev_feature_flags_round_trip_independently_and_preserve_provider() {
    let fields = [
        "screen_routing",
        "correction_review",
        "search_routing",
        "polish_audit",
    ];
    for legacy_enabled in [false, true] {
        for enabled_field in fields {
            let mut payload = json!({
                "enabled": legacy_enabled,
                "provider": "vercel",
                "screen_routing": false,
                "correction_review": false,
                "search_routing": false,
                "polish_audit": false,
            });
            payload[enabled_field] = json!(true);

            let config: JevConfig =
                serde_json::from_value(payload).expect("feature config should deserialize");
            assert_eq!(config.enabled, legacy_enabled);
            assert_eq!(config.provider, JevProvider::Vercel);

            let persisted = serde_json::to_value(config).expect("feature config should serialize");
            for field in fields {
                assert_eq!(
                    persisted[field],
                    json!(field == enabled_field),
                    "feature flag {field} should be independent"
                );
            }
        }
    }
}

#[tokio::test]
async fn evaluate_uses_selected_provider_transport_and_returns_json_payload() {
    let response = br#"{"answers":{"screen":{"type":"choice","choice":"needed","probabilities":{"needed":0.95,"unneeded":0.04,"uncertain":0.01}}}}"#;
    let providers = [
        (JevProvider::TypeSafe, "/v1/systemone", Some("jev-1.13.0")),
        (
            JevProvider::OpenRouter,
            "/api/alpha/decisions",
            Some("typesafe/jev-1.13"),
        ),
        (JevProvider::Vercel, "/v4/ai/evaluation-model", None),
    ];

    for (provider, expected_path, expected_model) in providers {
        let (endpoint, server) = spawn_json_server("200 OK", response).await;
        let state = json!({"text": TEST_STATE, "app": "test"});
        let questions = jev_tasks::screen_questions();
        let result = jev_tasks::evaluate(
            &reqwest::Client::new(),
            provider,
            TEST_API_KEY,
            state.clone(),
            questions.clone(),
            Duration::from_secs(1),
            Some(endpoint.as_str()),
        )
        .await
        .expect("valid provider response should parse");
        let captured = server.await.expect("mock server should finish");

        assert_eq!(result, serde_json::from_slice::<Value>(response).unwrap());
        assert_eq!(captured.method, "POST");
        assert_eq!(captured.path, expected_path);
        assert_eq!(
            header_value(&captured.headers, "authorization").as_deref(),
            Some("Bearer jev-test-key")
        );
        assert_eq!(captured.body["state"], state);
        assert_eq!(captured.body["questions"], questions);

        if let Some(model) = expected_model {
            assert_eq!(captured.body["model"], json!(model));
        } else {
            assert!(captured.body.get("model").is_none());
            assert_eq!(
                header_value(&captured.headers, "ai-model-id").as_deref(),
                Some("typesafe-ai/jev")
            );
            assert_eq!(
                header_value(
                    &captured.headers,
                    "ai-evaluation-model-specification-version"
                )
                .as_deref(),
                Some("4")
            );
            assert_eq!(
                header_value(&captured.headers, "ai-gateway-protocol-version").as_deref(),
                Some("0.0.1")
            );
        }
    }
}

#[tokio::test]
async fn evaluate_fails_closed_for_malformed_json_and_http_errors() {
    for (status, response) in [
        ("200 OK", br#"not json"#.as_slice()),
        ("503 Service Unavailable", br#"{"error":"busy"}"#.as_slice()),
    ] {
        let (endpoint, server) = spawn_json_server(status, response).await;
        let result = jev_tasks::evaluate(
            &reqwest::Client::new(),
            JevProvider::TypeSafe,
            TEST_API_KEY,
            json!({"text": TEST_STATE}),
            jev_tasks::screen_questions(),
            Duration::from_secs(1),
            Some(endpoint.as_str()),
        )
        .await;
        let captured = timeout(Duration::from_millis(500), server)
            .await
            .expect("evaluation should contact the local mock server")
            .expect("mock server should finish");

        assert!(result.is_none(), "{status} must fail closed");
        assert_eq!(captured.path, "/v1/systemone");
    }
}

#[tokio::test]
async fn evaluate_skips_network_for_empty_key_or_questions() {
    let (endpoint, count, mut request_seen, mut server) =
        spawn_counting_server("200 OK", br#"{}"#).await;
    let result = jev_tasks::evaluate(
        &reqwest::Client::new(),
        JevProvider::TypeSafe,
        "",
        json!({"text": TEST_STATE}),
        jev_tasks::screen_questions(),
        Duration::from_secs(1),
        Some(endpoint.as_str()),
    )
    .await;
    assert!(result.is_none());
    assert_no_request(&mut server, &count, &mut request_seen).await;

    let (endpoint, count, mut request_seen, mut server) =
        spawn_counting_server("200 OK", br#"{}"#).await;
    let result = jev_tasks::evaluate(
        &reqwest::Client::new(),
        JevProvider::TypeSafe,
        TEST_API_KEY,
        json!({"text": TEST_STATE}),
        json!({}),
        Duration::from_secs(1),
        Some(endpoint.as_str()),
    )
    .await;
    assert!(result.is_none());
    assert_no_request(&mut server, &count, &mut request_seen).await;
}

#[tokio::test]
async fn evaluate_bounds_the_entire_stalled_body_without_retrying() {
    let (endpoint, request_count, server, request_seen) = spawn_stalled_server().await;
    let started = Instant::now();
    let result = timeout(
        Duration::from_secs(1),
        jev_tasks::evaluate(
            &reqwest::Client::new(),
            JevProvider::TypeSafe,
            TEST_API_KEY,
            json!({"text": TEST_STATE}),
            jev_tasks::screen_questions(),
            Duration::from_millis(100),
            Some(endpoint.as_str()),
        ),
    )
    .await
    .expect("bounded Jev evaluation should return before the outer test timeout");

    assert!(result.is_none());
    assert!(started.elapsed() < Duration::from_millis(900));
    timeout(Duration::from_millis(250), request_seen)
        .await
        .expect("the local server should observe one request before the deadline")
        .expect("request notification should remain open until observed");
    assert_eq!(request_count.load(Ordering::SeqCst), 1);
    server.abort();
}

#[test]
fn screen_topic_words_do_not_override_auto_decisions() {
    for text in [
        "我买了一个新屏幕",
        "写一个截图工具的介绍",
        "Write a screenplay",
        "I bought a screen",
        "Describe current windowing systems",
    ] {
        assert!(!jev_tasks::contains_explicit_screen_request(text), "{text}");
        assert!(!jev_tasks::screen_allowed(
            true,
            jev_tasks::contains_explicit_screen_request(text),
            Some("unneeded")
        ));
    }
    for text in [
        "帮我解释屏幕上的错误",
        "看看当前窗口",
        "解释这张截图",
        "What is on my screen?",
        "Look at the screen.",
    ] {
        assert!(jev_tasks::contains_explicit_screen_request(text), "{text}");
        assert!(jev_tasks::screen_allowed(
            true,
            jev_tasks::contains_explicit_screen_request(text),
            Some("unneeded")
        ));
    }
}
