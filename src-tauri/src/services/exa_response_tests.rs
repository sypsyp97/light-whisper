use super::parse_exa_response;

const SAFE_SENTINEL: &str = "exa-safe-synthetic-secret";

fn direct_response(content: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": content,
    })
    .to_string()
}

fn text_content(text: &str) -> serde_json::Value {
    serde_json::json!({
        "content": [{
            "type": "text",
            "text": text,
        }],
    })
}

fn assert_error_code(raw: &str, code: &str) {
    let error =
        parse_exa_response(raw).expect_err("malformed or failed Exa response must be rejected");

    assert!(error.contains(code), "error must contain {code}: {error}");
    assert!(
        !error.contains(raw),
        "error must not expose the complete response body: {error}"
    );
    assert!(
        !error.contains(SAFE_SENTINEL),
        "error must not expose response secrets: {error}"
    );
}

#[test]
fn accepts_direct_json_with_meaningful_labeled_search_result() {
    let raw = direct_response(text_content(
        "Title: Rust Release Notes\nURL: https://example.test/rust\nPublished Date: 2026-09-21\nText: Rust shipped a safe synthetic release note.",
    ));

    let results = parse_exa_response(&raw).expect("valid direct JSON should parse");

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].title, "Rust Release Notes",
        "the labeled title should be preserved"
    );
    assert_eq!(results[0].url, "https://example.test/rust");
    assert_eq!(
        results[0].content,
        "Rust shipped a safe synthetic release note."
    );
    assert_eq!(results[0].published_date.as_deref(), Some("2026-09-21"));
}

#[test]
fn accepts_sse_labeled_json_and_ignores_done_marker() {
    let payload = direct_response(text_content(
        "Title: SSE result\nURL: https://example.test/sse\nHighlights: The final SSE payload is usable.",
    ));
    let raw = format!("event: message\ndata: {payload}\n\ndata: [DONE]\n\n",);

    let results = parse_exa_response(&raw).expect("SSE JSON followed by [DONE] should parse");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "SSE result");
    assert_eq!(results[0].url, "https://example.test/sse");
    assert_eq!(results[0].content, "The final SSE payload is usable.");
}

#[test]
fn accepts_genuine_empty_result_content() {
    let raw = direct_response(serde_json::json!({ "content": [] }));

    let results =
        parse_exa_response(&raw).expect("an explicit empty result is a valid empty search");

    assert!(results.is_empty());
}

#[test]
fn classifies_observed_rate_limit_meta_without_leaking_response_body() {
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "_meta": { "ai.exa/rateLimited": true },
            "content": [{
                "type": "text",
                "text": format!("Rate limit diagnostic: {SAFE_SENTINEL}"),
            }],
        },
    })
    .to_string();

    assert_error_code(&raw, "SEARCH_RATE_LIMITED");
}

#[test]
fn classifies_json_rpc_error_without_leaking_response_body() {
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32000,
            "message": format!("upstream diagnostic {SAFE_SENTINEL}"),
        },
    })
    .to_string();

    assert_error_code(&raw, "SEARCH_PROVIDER_ERROR");
}

#[test]
fn classifies_result_error_without_leaking_response_body() {
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "isError": true,
            "content": [{
                "type": "text",
                "text": format!("provider diagnostic {SAFE_SENTINEL}"),
            }],
        },
    })
    .to_string();

    assert_error_code(&raw, "SEARCH_PROVIDER_ERROR");
}

#[test]
fn rejects_missing_result_as_invalid_response_without_leaking_response_body() {
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "note": SAFE_SENTINEL,
    })
    .to_string();

    assert_error_code(&raw, "SEARCH_INVALID_RESPONSE");
}

#[test]
fn rejects_nonempty_unrecognized_text_as_invalid_response() {
    let raw = direct_response(text_content(&format!(
        "This is not an Exa labeled result: {SAFE_SENTINEL}"
    )));

    assert_error_code(&raw, "SEARCH_INVALID_RESPONSE");
}
