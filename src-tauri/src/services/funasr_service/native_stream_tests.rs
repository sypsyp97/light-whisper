use super::native_stream::validate_stream_response;
use super::protocol::{ServerCommand, ServerResponse};
use serde_json::json;

fn response(
    success: Option<bool>,
    text: Option<&str>,
    language: Option<&str>,
    session_id: Option<u64>,
    sample_count: Option<usize>,
    is_final: Option<bool>,
    error: Option<&str>,
) -> ServerResponse {
    serde_json::from_value(json!({
        "success": success,
        "text": text,
        "language": language,
        "session_id": session_id,
        "sample_count": sample_count,
        "final": is_final,
        "error": error,
    }))
    .unwrap()
}

fn valid_response(text: Option<&str>, language: Option<&str>) -> ServerResponse {
    response(
        Some(true),
        text,
        language,
        Some(5),
        Some(3),
        Some(false),
        None,
    )
}

#[test]
fn stream_commands_serialize_as_flat_snake_case_without_null_options() {
    let start = serde_json::to_value(ServerCommand::StreamStart {
        session_id: 7,
        context: "meeting".to_string(),
        hot_words: Some(vec!["Rust".to_string(), "R2T2".to_string()]),
        language: Some("zh".to_string()),
    })
    .unwrap();
    assert_eq!(
        start,
        json!({
            "action": "stream_start",
            "session_id": 7,
            "context": "meeting",
            "hot_words": ["Rust", "R2T2"],
            "language": "zh"
        })
    );

    let start_without_options = serde_json::to_value(ServerCommand::StreamStart {
        session_id: 8,
        context: String::new(),
        hot_words: None,
        language: None,
    })
    .unwrap();
    assert_eq!(
        start_without_options,
        json!({"action": "stream_start", "session_id": 8, "context": ""})
    );
    assert!(start_without_options.get("hot_words").is_none());
    assert!(start_without_options.get("language").is_none());

    let feed = serde_json::to_value(ServerCommand::StreamFeed {
        session_id: 7,
        offset: 320,
        audio_base64: "AQID".to_string(),
        audio_format: "pcm_s16le".to_string(),
        sample_rate: 16_000,
    })
    .unwrap();
    assert_eq!(
        feed,
        json!({
            "action": "stream_feed",
            "session_id": 7,
            "offset": 320,
            "audio_base64": "AQID",
            "audio_format": "pcm_s16le",
            "sample_rate": 16000
        })
    );

    assert_eq!(
        serde_json::to_value(ServerCommand::StreamFinish { session_id: 7 }).unwrap(),
        json!({"action": "stream_finish", "session_id": 7})
    );
    assert_eq!(
        serde_json::to_value(ServerCommand::StreamCancel { session_id: 7 }).unwrap(),
        json!({"action": "stream_cancel", "session_id": 7})
    );
}

#[test]
fn validator_accepts_empty_and_committed_successes_and_normalizes_empty_language() {
    let empty = validate_stream_response(
        response(
            Some(true),
            Some(""),
            Some(""),
            Some(5),
            Some(0),
            Some(false),
            None,
        ),
        5,
        0,
        false,
        "",
    )
    .unwrap();
    assert_eq!(empty.text, "");
    assert_eq!(empty.language, None);
    assert_eq!(empty.sample_count, 0);
    assert!(!empty.is_final);

    let committed = validate_stream_response(
        valid_response(Some("hello"), Some("en")),
        5,
        3,
        false,
        "hel",
    )
    .unwrap();
    assert_eq!(committed.text, "hello");
    assert_eq!(committed.language.as_deref(), Some("en"));
    assert_eq!(committed.sample_count, 3);
    assert!(!committed.is_final);
}

#[test]
fn validator_rejects_missing_or_mismatched_required_fields() {
    let mut missing_success = valid_response(Some("committed"), None);
    missing_success.success = None;
    let mut false_success = valid_response(Some("committed"), None);
    false_success.success = Some(false);
    let mut missing_session = valid_response(Some("committed"), None);
    missing_session.session_id = None;
    let mut wrong_session = valid_response(Some("committed"), None);
    wrong_session.session_id = Some(6);
    let mut missing_count = valid_response(Some("committed"), None);
    missing_count.sample_count = None;
    let mut wrong_count = valid_response(Some("committed"), None);
    wrong_count.sample_count = Some(4);
    let mut missing_final = valid_response(Some("committed"), None);
    missing_final.is_final = None;
    let mut wrong_final = valid_response(Some("committed"), None);
    wrong_final.is_final = Some(true);
    let mut missing_text = valid_response(None, None);
    missing_text.text = None;

    for invalid in [
        missing_success,
        false_success,
        missing_session,
        wrong_session,
        missing_count,
        wrong_count,
        missing_final,
        wrong_final,
        missing_text,
    ] {
        assert!(
            validate_stream_response(invalid, 5, 3, false, "committed").is_err(),
            "required response field was accepted"
        );
    }
}

#[test]
fn validator_rejects_regressed_prefix_without_echoing_response_payloads() {
    let response_text = "REWRITTEN_TEXT_SECRET";
    let response_error = "ARBITRARY_ERROR_SECRET";
    let invalid = response(
        Some(true),
        Some(response_text),
        Some("en"),
        Some(5),
        Some(4),
        Some(false),
        Some(response_error),
    );

    let error = validate_stream_response(invalid, 5, 4, false, "committed").unwrap_err();
    let rendered = format!("{error}\n{error:?}");
    assert!(!rendered.contains(response_text));
    assert!(!rendered.contains(response_error));

    let failed = response(
        Some(false),
        Some(response_text),
        None,
        Some(5),
        Some(4),
        Some(false),
        Some(response_error),
    );
    let error = validate_stream_response(failed, 5, 4, false, "committed").unwrap_err();
    let rendered = format!("{error}\n{error:?}");
    assert!(!rendered.contains(response_text));
    assert!(!rendered.contains(response_error));
}

#[test]
fn validator_accepts_empty_final_cancel_shape() {
    let cancelled = validate_stream_response(
        response(
            Some(true),
            Some(""),
            Some(""),
            Some(9),
            Some(12),
            Some(true),
            None,
        ),
        9,
        12,
        true,
        "",
    )
    .unwrap();
    assert_eq!(cancelled.text, "");
    assert_eq!(cancelled.language, None);
    assert_eq!(cancelled.sample_count, 12);
    assert!(cancelled.is_final);
}

#[test]
fn validator_rejects_nonempty_tentative_text_on_final_response() {
    let invalid = serde_json::from_value(json!({
        "success": true,
        "text": "committed",
        "tentative_text": "stale draft",
        "language": "en",
        "session_id": 5,
        "sample_count": 3,
        "final": true,
        "error": null,
    }))
    .unwrap();

    assert!(
        validate_stream_response(invalid, 5, 3, true, "committed").is_err(),
        "final responses must not carry tentative text"
    );
}

#[test]
fn validator_preserves_mutable_preview_without_committing_it() {
    for preview in ["上海", "上班", "上", ""] {
        let response = serde_json::from_value(json!({
            "success": true, "text": "明天去", "tentative_text": preview,
            "session_id": 5, "sample_count": 3, "final": false,
        }))
        .unwrap();
        let update = validate_stream_response(response, 5, 3, false, "明天去").unwrap();
        assert_eq!(update.text, "明天去");
        assert_eq!(update.tentative_text, preview);
    }
}
