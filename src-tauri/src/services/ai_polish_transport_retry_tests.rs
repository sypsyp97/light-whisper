use super::ai_polish_service::ai_polish_transport_plan;
use super::llm_client::LlmRequestOptions;
use crate::state::user_profile::LlmReasoningMode;

fn assert_plan_stage(
    stage: &LlmRequestOptions<'static>,
    stream: bool,
    json_output: bool,
    stream_event: Option<&str>,
    reasoning_mode: LlmReasoningMode,
    session_id: u64,
) {
    assert_eq!(stage.stream, stream);
    assert_eq!(stage.json_output, json_output);
    assert_eq!(stage.stream_event, stream_event);
    assert_eq!(stage.reasoning_mode, reasoning_mode);
    assert_eq!(stage.session_id, Some(session_id));
    assert!(!stage.web_search);
}

#[test]
fn ai_polish_transport_plan_uses_nostream_json_before_stream_nojson_without_partial_pref() {
    let reasoning_mode = LlmReasoningMode::Balanced;
    let session_id = 42;
    let plan = ai_polish_transport_plan(reasoning_mode, session_id, false);

    assert_plan_stage(
        &plan[0],
        true,
        true,
        Some("ai-polish-status"),
        reasoning_mode,
        session_id,
    );
    assert_plan_stage(&plan[1], false, true, None, reasoning_mode, session_id);
    assert_plan_stage(
        &plan[2],
        true,
        false,
        Some("ai-polish-status"),
        reasoning_mode,
        session_id,
    );
    assert_plan_stage(&plan[3], false, false, None, reasoning_mode, session_id);
}

#[test]
fn ai_polish_transport_plan_uses_stream_nojson_before_nostream_json_with_partial_pref() {
    let reasoning_mode = LlmReasoningMode::Deep;
    let session_id = 7;
    let plan = ai_polish_transport_plan(reasoning_mode, session_id, true);

    assert_plan_stage(
        &plan[0],
        true,
        true,
        Some("ai-polish-status"),
        reasoning_mode,
        session_id,
    );
    assert_plan_stage(
        &plan[1],
        true,
        false,
        Some("ai-polish-status"),
        reasoning_mode,
        session_id,
    );
    assert_plan_stage(&plan[2], false, true, None, reasoning_mode, session_id);
    assert_plan_stage(&plan[3], false, false, None, reasoning_mode, session_id);
}
