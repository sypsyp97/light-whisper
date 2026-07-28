use std::fs;
use std::path::PathBuf;

fn interim_source() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/services/audio_service/interim.rs");
    fs::read_to_string(path).expect("interim.rs must be readable")
}

fn expected_local_segments<'a>(previous: Option<&str>, current: &'a str) -> (&'a str, &'a str) {
    let Some(previous) = previous.filter(|text| !text.is_empty()) else {
        return ("", current);
    };

    let stable_bytes = previous
        .chars()
        .zip(current.chars())
        .take_while(|(left, right)| left == right)
        .map(|(_, ch)| ch.len_utf8())
        .sum();
    current.split_at(stable_bytes)
}

#[test]
fn stability_fields_apply_after_the_online_engine_guard_without_a_model_gate() {
    let source = interim_source();

    assert!(
        source.contains("if paths::is_online_engine(&engine)"),
        "online engines must continue to skip the interim transcription loop"
    );
    assert!(
        !source.contains("is_qwen3_asr"),
        "stability presentation must not be restricted to a particular local model family"
    );
}

#[test]
fn local_interim_source_emits_stability_fields() {
    let source = interim_source();

    assert!(
        source.contains("stableText") && source.contains("tentativeText"),
        "local interim events must emit camelCase stableText and tentativeText fields"
    );
}

#[test]
fn every_attempt_advances_its_recording_checkpoint_before_result_handling() {
    let source = interim_source();
    let request_end = source
        .find("let transcription_result = funasr_service::transcribe_pcm16")
        .expect("interim loop must retain the transcription attempt result");
    let checkpoint = source[request_end..]
        .find("last_sample_count = current_count;")
        .expect("every attempted window must advance the recording checkpoint")
        + request_end;
    let result_match = source[request_end..]
        .find("match transcription_result")
        .expect("interim loop must handle the retained result")
        + request_end;

    assert!(checkpoint < result_match);
}

#[test]
fn first_hypothesis_is_entirely_tentative() {
    let text = "今天天气很好";
    let (stable, tentative) = expected_local_segments(None, text);

    assert_eq!(stable, "");
    assert_eq!(tentative, text);
    assert_eq!(format!("{stable}{tentative}"), text);
}

#[test]
fn common_prefix_is_utf8_safe() {
    let current = "你好，世界呀";
    let (stable, tentative) = expected_local_segments(Some("你好，世界啊"), current);

    assert_eq!(stable, "你好，世界");
    assert_eq!(tentative, "呀");
    assert_eq!(format!("{stable}{tentative}"), current);
    assert!(current.is_char_boundary(stable.len()));
}

#[test]
fn rewritten_tail_never_leaks_from_the_previous_hypothesis() {
    let current = "我们明天去上班";
    let (stable, tentative) = expected_local_segments(Some("我们明天去上海"), current);

    assert_eq!(stable, "我们明天去上");
    assert_eq!(tentative, "班");
    assert_eq!(format!("{stable}{tentative}"), current);
}
