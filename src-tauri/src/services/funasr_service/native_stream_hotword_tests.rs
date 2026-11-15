use super::native_stream::NativeStreamClient;
use super::{send_command_to_server, ServerCommand, TranscriptionResult};
use crate::state::user_profile::{CorrectionPattern, CorrectionSource, HotWord, HotWordSource};
use crate::state::{AppState, FunasrProcess};
use crate::utils::AppError;
use base64::Engine;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const SESSION_ID: u64 = 37;
const SAMPLE_COUNT: usize = 1600;
const RAW_PREFIX: &str = "请用 get hub";
const RAW_PREVIEW: &str = " 打开 open cloud";
const RAW_FINAL: &str = "请用 get hub 打开 open cloud。";
const CORRECTED_FINAL: &str = "请用 github 打开 openclaw。";
const FIXTURE_ENV: &str = "LIGHT_WHISPER_TEST_HOTWORD_STREAM";

fn write_reply(mut response: Value, request_id: &Value) {
    response["request_id"] = request_id.clone();
    println!("{response}");
    std::io::stdout().flush().unwrap();
}

#[test]
#[ignore = "subprocess fixture, invoked by the native stream hotword tests"]
fn native_stream_hotword_test_server() {
    let Ok(script) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    let steps: Vec<Value> = serde_json::from_str(&script).unwrap();
    let mut steps = steps.into_iter();
    let mut actions = Vec::new();
    let mut sample_count = 0;
    println!("{{\"test_ready\":true}}");
    std::io::stdout().flush().unwrap();

    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let action = request["action"].as_str().unwrap();
        if action == "status" {
            write_reply(
                json!({"success": true, "text": serde_json::to_string(&actions).unwrap()}),
                &request["request_id"],
            );
            continue;
        }
        actions.push(action.to_string());
        let step = steps.next().expect("unexpected stream command");
        assert_eq!(step["action"], request["action"]);
        assert_eq!(request["session_id"], SESSION_ID);
        if action == "stream_start" {
            assert_eq!(request["hot_words"], json!(["github", "openclaw"]));
        } else if action == "stream_feed" {
            assert_eq!(request["offset"], sample_count);
            assert_eq!(request["audio_format"], "pcm_s16le");
            assert_eq!(request["sample_rate"], 16_000);
            let pcm = base64::engine::general_purpose::STANDARD
                .decode(request["audio_base64"].as_str().unwrap())
                .unwrap();
            assert_eq!(pcm.len() % 2, 0);
            sample_count += pcm.len() / 2;
        }
        if let Some(stale) = step.get("stale_response") {
            let stale_request_id = json!(request["request_id"].as_u64().unwrap() + 1000);
            write_reply(stale.clone(), &stale_request_id);
        }
        write_reply(step["response"].clone(), &request["request_id"]);
    }
}

fn response(text: &str, sample_count: usize, is_final: bool, engine: &str) -> Value {
    json!({
        "success": true,
        "session_id": SESSION_ID,
        "sample_count": sample_count,
        "final": is_final,
        "text": text,
        "tentative_text": "",
        "language": null,
        "engine": engine,

        "duration": 9999.0,
    })
}

fn script(final_response: Value) -> Vec<Value> {
    let mut feed = response(RAW_PREFIX, SAMPLE_COUNT, false, "confucius4-r2t2");
    feed["tentative_text"] = json!(RAW_PREVIEW);
    feed["language"] = json!("zh");
    vec![
        json!({"action": "stream_start", "response": response("", 0, false, "confucius4-r2t2")}),
        json!({"action": "stream_feed", "response": feed}),
        json!({"action": "stream_finish", "response": final_response}),
    ]
}

fn qwen_final() -> Value {
    response(RAW_FINAL, SAMPLE_COUNT, true, "qwen3-asr-0.6b")
}

async fn start_client(script: Vec<Value>) -> (AppState, NativeStreamClient) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "services::funasr_service::native_stream_hotword_tests::native_stream_hotword_test_server",
            "--ignored",
            "--nocapture",
        ])
        .env(FIXTURE_ENV, serde_json::to_string(&script).unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).await.unwrap(), 0);
            if line.contains("\"test_ready\":true") {
                break;
            }
        }
    })
    .await
    .expect("IPC fixture startup");

    let state = AppState::default();
    state.update_profile_mut(|profile| {
        for text in ["github", "openclaw"] {
            profile.hot_words.push(HotWord {
                text: text.to_string(),
                weight: 3,
                source: HotWordSource::User,
                use_count: 10,
                last_used: 0,
            });
        }
        profile.correction_patterns.push(CorrectionPattern {
            original: "open cloud".to_string(),
            corrected: "openclaw".to_string(),
            count: 33,
            last_seen: 0,
            source: CorrectionSource::User,
        });
    });
    *state.engine.funasr_process.lock().await = Some(FunasrProcess {
        child,
        stdin,
        stdout,
    });
    state.set_funasr_ready(true);
    let client = tokio::time::timeout(
        Duration::from_secs(5),
        NativeStreamClient::start(&state, None, SESSION_ID, String::new(), Some("en".into())),
    )
    .await
    .expect("stream start must reply")
    .unwrap();
    (state, client)
}

async fn finish(
    client: &mut NativeStreamClient,
    state: &AppState,
) -> Result<TranscriptionResult, AppError> {
    tokio::time::timeout(Duration::from_secs(5), client.finish(state, None))
        .await
        .expect("stream finish must reply")
}

async fn assert_closed(
    client: &mut NativeStreamClient,
    state: &AppState,
    expected_actions: &[&str],
) {
    assert!(finish(client, state).await.is_err());
    assert!(client.feed(state, None, &[1]).await.is_err());
    assert!(client.cancel(state, None).await.is_err());
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        send_command_to_server::<tauri::Wry>(state, &ServerCommand::Status, None),
    )
    .await
    .expect("fixture status must reply")
    .unwrap();
    let actions: Vec<String> = serde_json::from_str(status.text.as_deref().unwrap()).unwrap();
    assert_eq!(actions, expected_actions);
}

#[tokio::test]
async fn qwen_native_finish_corrects_profile_terms_after_raw_stream_validation() {
    for (final_language, expected_language) in [(None, "zh"), (Some(""), "zh"), (Some("de"), "de")]
    {
        let mut final_response = qwen_final();
        final_response["language"] = json!(final_language);
        let (state, mut client) = start_client(script(final_response)).await;
        let preview = client.feed(&state, None, &[1; SAMPLE_COUNT]).await.unwrap();
        assert_eq!(preview.text, RAW_PREFIX);
        assert_eq!(preview.tentative_text, RAW_PREVIEW);
        assert!(!preview.is_final);

        let result = finish(&mut client, &state).await.unwrap();
        assert!(result.success);
        assert_eq!(result.error, None);
        assert_eq!(result.duration, Some(SAMPLE_COUNT as f64 / 16_000.0));
        assert_eq!(result.language.as_deref(), Some(expected_language));
        assert_closed(
            &mut client,
            &state,
            &["stream_start", "stream_feed", "stream_finish"],
        )
        .await;
        assert_eq!(result.text, CORRECTED_FINAL);
    }
}

#[tokio::test]
async fn native_finish_uses_final_response_engine_and_leaves_other_text_raw() {
    for engine in [Some("confucius4-r2t2"), None] {
        let mut final_response = qwen_final();
        if let Some(engine) = engine {
            final_response["engine"] = json!(engine);
        } else {
            final_response.as_object_mut().unwrap().remove("engine");
        }
        let mut steps = script(final_response);

        steps[0]["response"]["engine"] = json!("qwen3-asr-0.6b");
        steps[1]["response"]["engine"] = json!("qwen3-asr-0.6b");
        let (state, mut client) = start_client(steps).await;
        client.feed(&state, None, &[1; SAMPLE_COUNT]).await.unwrap();
        let result = finish(&mut client, &state).await.unwrap();
        assert_eq!(result.text, RAW_FINAL, "final engine: {engine:?}");
        assert_eq!(result.language.as_deref(), Some("zh"));
        assert_eq!(result.duration, Some(SAMPLE_COUNT as f64 / 16_000.0));
    }
}

#[tokio::test]
async fn native_finish_rejects_invalid_final_frames_before_applying_corrections() {
    let mut invalid_responses = Vec::new();
    for field in ["success", "session_id", "sample_count", "final", "text"] {
        let mut invalid = qwen_final();
        invalid.as_object_mut().unwrap().remove(field);
        invalid_responses.push((format!("missing {field}"), invalid));
    }
    for (field, value) in [
        ("success", json!(false)),
        ("session_id", json!(SESSION_ID + 1)),
        ("sample_count", json!(SAMPLE_COUNT + 1)),
        ("final", json!(false)),
        ("tentative_text", json!(RAW_PREVIEW)),
        ("text", json!(CORRECTED_FINAL)),
    ] {
        let mut invalid = qwen_final();
        invalid[field] = value;
        invalid_responses.push((format!("invalid {field}"), invalid));
    }

    for (case, invalid) in invalid_responses {
        let (state, mut client) = start_client(script(invalid)).await;
        client.feed(&state, None, &[1; SAMPLE_COUNT]).await.unwrap();
        assert!(finish(&mut client, &state).await.is_err(), "{case}");
        assert_eq!(client.committed_text, RAW_PREFIX, "{case}");
        assert_eq!(client.sample_count, SAMPLE_COUNT, "{case}");
        assert_eq!(client.language.as_deref(), Some("zh"), "{case}");
        assert_closed(
            &mut client,
            &state,
            &["stream_start", "stream_feed", "stream_finish"],
        )
        .await;
    }
}

#[tokio::test]
async fn native_feed_rejects_rewritten_committed_prefix_without_correcting_it() {
    let mut steps = script(qwen_final());
    steps[1]["response"]["sample_count"] = json!(SAMPLE_COUNT / 2);
    steps[2] = json!({
        "action": "stream_feed",
        "response": response(CORRECTED_FINAL, SAMPLE_COUNT, false, "qwen3-asr-0.6b"),
    });
    let (state, mut client) = start_client(steps).await;
    let preview = client
        .feed(&state, None, &[1; SAMPLE_COUNT / 2])
        .await
        .unwrap();
    assert_eq!(preview.text, RAW_PREFIX);
    assert_eq!(preview.tentative_text, RAW_PREVIEW);
    assert!(client
        .feed(&state, None, &[1; SAMPLE_COUNT / 2])
        .await
        .is_err());
    assert_eq!(client.committed_text, RAW_PREFIX);
    assert_eq!(client.sample_count, SAMPLE_COUNT / 2);
    assert_closed(
        &mut client,
        &state,
        &["stream_start", "stream_feed", "stream_feed"],
    )
    .await;
}

#[tokio::test]
async fn native_finish_discards_stale_request_frame_before_the_current_reply() {
    let mut final_response = qwen_final();
    final_response["engine"] = json!("confucius4-r2t2");
    let mut steps = script(final_response);
    steps[2]["stale_response"] = response(CORRECTED_FINAL, SAMPLE_COUNT, true, "qwen3-asr-0.6b");
    let (state, mut client) = start_client(steps).await;
    client.feed(&state, None, &[1; SAMPLE_COUNT]).await.unwrap();
    let result = finish(&mut client, &state).await.unwrap();
    assert_eq!(result.text, RAW_FINAL);
    assert_closed(
        &mut client,
        &state,
        &["stream_start", "stream_feed", "stream_finish"],
    )
    .await;
}
