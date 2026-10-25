use super::native_stream::{NativeStreamClient, NativeStreamUpdate};
use super::{send_command_to_server, ServerCommand};
use crate::state::{AppState, FunasrProcess};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::os::windows::io::AsRawHandle;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tauri::Listener;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

// Run the test binary itself as a tiny IPC server; no Python/GPU/model dependency.
#[test]
#[ignore = "subprocess fixture, invoked by the IPC recovery tests"]
fn ipc_test_server() {
    let Ok(mode) = std::env::var("LIGHT_WHISPER_TEST_IPC_MODE") else {
        return;
    };
    if mode == "closed_stdin" {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(std::io::stdin().as_raw_handle());
        }
    }
    println!("{{\"test_ready\":true}}");
    std::io::stdout().flush().unwrap();
    if mode == "closed_stdout" {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(std::io::stdout().as_raw_handle());
        }
    }
    if mode.starts_with("closed_") || mode == "parked_stdin" {
        loop {
            std::thread::park();
        }
    }
    let mut stream_pcm = Vec::new();
    let mut stream_packets = Vec::new();
    let mut stream_session = 0;
    for line in std::io::stdin().lock().lines() {
        let request: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        if mode == "native_stream_audit" {
            let action = request["action"].as_str().unwrap();
            match action {
                "stream_start" => {
                    stream_session = request["session_id"].as_u64().unwrap();
                    stream_pcm.clear();
                    stream_packets.clear();
                }
                "stream_feed" => {
                    let raw = base64::engine::general_purpose::STANDARD
                        .decode(request["audio_base64"].as_str().unwrap())
                        .unwrap();
                    stream_packets.push(serde_json::json!({
                        "offset": request["offset"],
                        "count": raw.len() / 2,
                        "preview": request.get("preview"),
                        "audio_format": request["audio_format"],
                        "sample_rate": request["sample_rate"],
                    }));
                    stream_pcm.extend_from_slice(&raw);
                }
                "stream_finish" | "stream_cancel" => {}
                _ => panic!("unexpected stream audit action: {action}"),
            }
            let tentative = if action == "stream_feed" {
                serde_json::json!({
                    "packets": stream_packets,
                    "pcm_sha256": format!("{:x}", Sha256::digest(&stream_pcm)),
                })
                .to_string()
            } else {
                String::new()
            };
            println!(
                "{}",
                serde_json::json!({
                    "request_id": request["request_id"],
                    "success": true,
                    "session_id": stream_session,
                    "sample_count": stream_pcm.len() / 2,
                    "text": if action == "stream_finish" { "finished" } else { "" },
                    "tentative_text": tentative,
                    "language": "en",
                    "final": matches!(action, "stream_finish" | "stream_cancel"),
                })
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        println!(
            "{}",
            serde_json::json!({
                "request_id": request["request_id"],
                "success": mode != "application_error",
                "error": (mode == "application_error").then_some("invalid audio"),
            })
        );
        std::io::stdout().flush().unwrap();
    }
}

pub(crate) async fn test_process(mode: &str) -> FunasrProcess {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "services::funasr_service::ipc_recovery_tests::ipc_test_server",
            "--ignored",
            "--nocapture",
        ])
        .env("LIGHT_WHISPER_TEST_IPC_MODE", mode)
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
    assert!(
        child.try_wait().unwrap().is_none(),
        "fixture must still live"
    );
    FunasrProcess {
        child,
        stdin,
        stdout,
    }
}

pub(crate) fn assert_stream_packet_audit(
    update: &NativeStreamUpdate,
    samples: &[i16],
    preview_modes: &[Option<bool>],
) {
    let audit: serde_json::Value = serde_json::from_str(&update.tentative_text).unwrap();
    let packets = audit["packets"].as_array().unwrap();
    assert_eq!(packets.len(), preview_modes.len());
    let mut offset = 0;
    for (packet, preview) in packets.iter().zip(preview_modes) {
        let count = packet["count"].as_u64().unwrap() as usize;
        assert!(
            (1..=5120).contains(&count),
            "protocol packet exceeded 320 ms"
        );
        assert_eq!(packet["offset"], serde_json::json!(offset));
        assert_eq!(packet["preview"], serde_json::json!(preview));
        assert_eq!(packet["audio_format"], "pcm_s16le");
        assert_eq!(packet["sample_rate"], 16_000);
        offset += count;
    }
    assert_eq!(offset, samples.len());
    assert_eq!(update.sample_count, samples.len());
    let expected_pcm = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect::<Vec<_>>();
    assert_eq!(
        audit["pcm_sha256"],
        format!("{:x}", Sha256::digest(&expected_pcm)),
        "all PCM bytes, including the last sample, must arrive once in order"
    );
}

async fn check_coalesced_stream_packets(preview: bool) {
    let state = AppState::default();
    *state.engine.funasr_process.lock().await = Some(test_process("native_stream_audit").await);
    state.set_funasr_ready(true);
    let mut client =
        NativeStreamClient::start(&state, None, 7, "context".into(), Some("en".into()))
            .await
            .unwrap();
    let samples = (0..160_004)
        .map(|index| (index % 65536) as i16)
        .collect::<Vec<_>>();
    client.feed(&state, None, &samples[..3]).await.unwrap();
    let update = client
        .feed_coalesced(&state, None, &samples[3..], preview)
        .await
        .unwrap();
    let mut expected_modes = vec![None];
    expected_modes.extend(std::iter::repeat_n(Some(false), 31));
    expected_modes.push(Some(preview));
    assert_stream_packet_audit(&update, &samples, &expected_modes);
    let empty = client
        .feed_coalesced(&state, None, &[], preview)
        .await
        .unwrap();
    assert_eq!(empty.tentative_text, update.tentative_text);
    assert_eq!(empty.sample_count, samples.len());
    let final_result = client.finish(&state, None).await.unwrap();
    assert_eq!(final_result.text, "finished");
    assert_eq!(final_result.language.as_deref(), Some("en"));
    assert_eq!(final_result.duration, Some(samples.len() as f64 / 16_000.0));
    assert!(client
        .feed_coalesced(&state, None, &[1], preview)
        .await
        .is_err());
}

#[tokio::test]
async fn native_stream_coalesced_live_packets_preview_only_the_latest_packet() {
    check_coalesced_stream_packets(true).await;
}

#[tokio::test]
async fn native_stream_coalesced_stop_packets_never_request_preview() {
    check_coalesced_stream_packets(false).await;
}

#[tokio::test]
async fn native_stream_ordinary_feed_keeps_every_r2t2_packet_in_order() {
    let state = AppState::default();
    *state.engine.funasr_process.lock().await = Some(test_process("native_stream_audit").await);
    state.set_funasr_ready(true);
    let mut client = NativeStreamClient::start(&state, None, 8, String::new(), None)
        .await
        .unwrap();
    let samples = (0..10_241)
        .map(|index| (index as i16).wrapping_mul(17))
        .collect::<Vec<_>>();
    let update = client.feed(&state, None, &samples).await.unwrap();
    assert_stream_packet_audit(&update, &samples, &[None, None, None]);
    client.cancel(&state, None).await.unwrap();
    assert!(client.finish(&state, None).await.is_err());
}

#[tokio::test]
async fn broken_pipe_invalidates_live_process_and_allows_a_fresh_server() {
    let app = tauri::test::mock_app();

    for mode in ["closed_stdin", "closed_stdout"] {
        let state = AppState::default();
        *state.engine.funasr_process.lock().await = Some(test_process(mode).await);
        state.set_funasr_ready(true);
        state.set_inline_audio_transport(Some(true));
        let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let received = events.clone();
        let listener = app.listen("funasr-status", move |event| {
            received.lock().push(
                serde_json::from_str::<serde_json::Value>(event.payload())
                    .expect("valid engine status payload"),
            );
        });

        let error = tokio::time::timeout(
            Duration::from_secs(5),
            send_command_to_server(&state, &ServerCommand::Status, Some(app.handle())),
        )
        .await
        .expect("broken IPC must return promptly")
        .expect_err("closed pipe must fail");
        assert!(
            error.to_string().contains("刷新 stdin 缓冲区失败")
                || error.to_string().contains("写入命令到 FunASR 失败")
                || error.to_string().contains("stdout 已关闭"),
            "unexpected failure: {error}"
        );
        assert!(!state.is_funasr_ready(), "broken IPC cannot remain ready");
        assert!(state.engine.funasr_process.lock().await.is_none());
        assert_eq!(state.inline_audio_transport(), None);
        assert_eq!(events.lock().len(), 1, "one crash notification per failure");
        assert_eq!(events.lock()[0]["status"], "crashed");
        app.unlisten(listener);

        *state.engine.funasr_process.lock().await = Some(test_process("healthy").await);
        state.set_funasr_ready(true);
        let response = send_command_to_server(&state, &ServerCommand::Status, Some(app.handle()))
            .await
            .expect("fresh server must accept commands");
        assert_eq!(response.success, Some(true));
        assert!(state.is_funasr_ready());
    }
}

#[tokio::test]
async fn application_error_keeps_the_healthy_ipc_server() {
    let app = tauri::test::mock_app();
    let state = AppState::default();
    *state.engine.funasr_process.lock().await = Some(test_process("application_error").await);
    state.set_funasr_ready(true);
    state.set_inline_audio_transport(Some(true));

    let response = send_command_to_server(&state, &ServerCommand::Status, Some(app.handle()))
        .await
        .expect("valid error response is not a transport failure");
    assert_eq!(response.success, Some(false));
    assert!(state.is_funasr_ready());
    assert!(state.engine.funasr_process.lock().await.is_some());
    assert_eq!(state.inline_audio_transport(), Some(true));
}

fn pipe_filling_command() -> ServerCommand {
    ServerCommand::StreamFeed {
        session_id: 1,
        offset: 0,
        // The Windows anonymous pipe cannot buffer this payload while its
        // live peer parks without ever reading stdin.
        audio_base64: "A".repeat(8 * 1024 * 1024),
        audio_format: "pcm_f32le".to_string(),
        sample_rate: 16_000,
        preview: None,
    }
}

#[tokio::test]
async fn blocked_stdin_obeys_command_deadline_and_allows_a_fresh_server() {
    let app = tauri::test::mock_app();
    let state = AppState::default();
    *state.engine.funasr_process.lock().await = Some(test_process("parked_stdin").await);
    state.set_funasr_ready(true);
    state.set_inline_audio_transport(Some(true));
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let received = events.clone();
    let listener = app.listen("funasr-status", move |event| {
        received
            .lock()
            .push(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap());
    });
    let command = pipe_filling_command();

    tokio::time::timeout(
        Duration::from_secs(2),
        super::send_command_to_server_with_timeout(
            &state,
            &command,
            Some(app.handle()),
            Duration::from_millis(100),
        ),
    )
    .await
    .expect("the production command deadline must cover a blocked stdin write")
    .expect_err("a server that never reads stdin must time out");

    assert!(!state.is_funasr_ready());
    assert_eq!(state.inline_audio_transport(), None);
    assert!(state.engine.funasr_process.lock().await.is_none());
    assert_eq!(events.lock().len(), 1);
    assert_eq!(events.lock()[0]["status"], "crashed");
    app.unlisten(listener);

    *state.engine.funasr_process.lock().await = Some(test_process("healthy").await);
    state.set_funasr_ready(true);
    let response = send_command_to_server(&state, &ServerCommand::Status, Some(app.handle()))
        .await
        .expect("a fresh server must acquire the released IPC lock");
    assert_eq!(response.success, Some(true));
    assert!(state.is_funasr_ready());
}

#[tokio::test]
async fn cancelled_partial_stdin_write_invalidates_the_owned_server() {
    let app = tauri::test::mock_app();
    let state = AppState::default();
    *state.engine.funasr_process.lock().await = Some(test_process("parked_stdin").await);
    state.set_funasr_ready(true);
    state.set_inline_audio_transport(Some(true));
    let command = pipe_filling_command();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(150),
            send_command_to_server(&state, &command, Some(app.handle())),
        )
        .await
        .is_err(),
        "the large command must remain pending against a non-reading peer"
    );

    let process = tokio::time::timeout(Duration::from_secs(1), state.engine.funasr_process.lock())
        .await
        .expect("cancellation must release the IPC lock");
    assert!(
        process.is_none(),
        "a partially written JSON frame cannot be reused after cancellation"
    );
    drop(process);
    assert!(!state.is_funasr_ready());
    assert_eq!(state.inline_audio_transport(), None);

    *state.engine.funasr_process.lock().await = Some(test_process("healthy").await);
    state.set_funasr_ready(true);
    assert_eq!(
        send_command_to_server(&state, &ServerCommand::Status, Some(app.handle()))
            .await
            .unwrap()
            .success,
        Some(true)
    );
}

// Formal replay ownership: separate append-only test module.
mod async_ownership_replay_tests {
    use super::*;
    use std::future::{poll_fn, Future};
    use std::pin::Pin;
    use std::task::Poll;

    mod trace {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test-support/async_ownership_trace.rs"
        ));
    }

    #[test]
    #[ignore = "subprocess fixture, invoked by TLC IPC replay"]
    fn ipc_trace_server() {
        let Ok(gate) = std::env::var("LIGHT_WHISPER_IPC_TRACE_GATE") else {
            return;
        };
        println!("{{\"test_ready\":true}}");
        std::io::stdout().flush().unwrap();
        while !std::path::Path::new(&gate).exists() {
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap();
        std::fs::write(format!("{gate}.consumed"), b"consumed").unwrap();
        loop {
            std::thread::park();
        }
    }

    #[tokio::test]
    async fn tlc_ipc_deadline_replays_write_progress_without_restarting_the_budget() {
        for name in ["ipc-deadline", "ipc-write-deadline", "ipc-cancel"] {
            replay_ipc_schedule(name).await;
        }
    }

    async fn poll_once<F: Future>(mut operation: Pin<&mut F>) -> Poll<F::Output> {
        poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await
    }

    async fn replay_ipc_schedule(name: &str) {
        let events = trace::load(name);
        let gate = std::env::temp_dir().join(format!(
            "light-whisper-ipc-trace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let consumed = std::path::PathBuf::from(format!("{}.consumed", gate.display()));
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "services::funasr_service::ipc_recovery_tests::async_ownership_replay_tests::ipc_trace_server", "--ignored", "--nocapture"])
            .env("LIGHT_WHISPER_IPC_TRACE_GATE", &gate)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .creation_flags(0x08000000).kill_on_drop(true).spawn().unwrap();
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
        .unwrap();
        let app = tauri::test::mock_app();
        let state = AppState::default();
        *state.engine.funasr_process.lock().await = Some(FunasrProcess {
            child,
            stdin,
            stdout,
        });
        state.set_funasr_ready(true);
        state.set_inline_audio_transport(Some(true));
        let command = pipe_filling_command();
        // IpcCommandDeadline expires at tick 3. Serialization and real pipe I/O
        // must not spend model ticks; only ipc:tick advances this clock.
        tokio::time::pause();
        let mut operation = Some(Box::pin(super::super::send_command_to_server_with_timeout(
            &state,
            &command,
            Some(app.handle()),
            Duration::from_secs(3),
        )));
        let started = tokio::time::Instant::now();
        let mut ticks = 0;
        let mut handled = 0;
        let total_ticks = events
            .iter()
            .filter(|event| event.as_str() == "ipc:tick")
            .count();
        for event in &events {
            match event.as_str() {
                "ipc:begin" => {
                    assert!(poll_once(operation.as_mut().unwrap().as_mut())
                        .await
                        .is_pending());
                    assert!(state.engine.funasr_process.try_lock().is_err());
                    assert!(!consumed.exists(), "the real pipe must still be blocked");
                }
                "ipc:write:complete" => {
                    std::fs::write(&gate, b"read").unwrap();
                    let watchdog = std::time::Instant::now();
                    loop {
                        assert!(
                            poll_once(operation.as_mut().unwrap().as_mut())
                                .await
                                .is_pending(),
                            "command settled before its reply in {name}"
                        );
                        if consumed.exists() {
                            break;
                        }
                        assert!(
                            watchdog.elapsed() < Duration::from_secs(10),
                            "child must consume the actual JSON frame in {name}"
                        );
                        // Remain runnable so Tokio cannot automatically advance
                        // paused time while the real subprocess/pipe progresses.
                        std::thread::sleep(Duration::from_millis(1));
                        tokio::task::yield_now().await;
                    }
                }
                "ipc:flush:complete" => {
                    assert!(consumed.exists());
                }
                "ipc:tick" => {
                    ticks += 1;
                    tokio::time::advance(Duration::from_secs(1)).await;
                    if ticks == total_ticks {
                        // Tokio rounds deadlines up to its next millisecond.
                        tokio::time::advance(Duration::from_millis(1)).await;
                        let watchdog = std::time::Instant::now();
                        loop {
                            if let Poll::Ready(result) =
                                poll_once(operation.as_mut().unwrap().as_mut()).await
                            {
                                result.expect_err("a server with no reply must time out");
                                break;
                            }
                            assert!(
                                watchdog.elapsed() < Duration::from_secs(10),
                                "the original total command deadline must expire at tick 3 in {name}"
                            );
                            std::thread::sleep(Duration::from_millis(1));
                            tokio::task::yield_now().await;
                        }
                    } else {
                        assert!(
                            poll_once(operation.as_mut().unwrap().as_mut())
                                .await
                                .is_pending(),
                            "command must remain pending before tick 3 in {name}"
                        );
                    }
                }
                "ipc:cancel" => {
                    drop(operation.take());
                }
                _ => panic!("unmapped TLC IPC event: {event}"),
            }
            assert_eq!(
                started.elapsed(),
                Duration::from_secs(ticks as u64)
                    + if ticks > 0 && ticks == total_ticks {
                        Duration::from_millis(1)
                    } else {
                        Duration::ZERO
                    },
                "only trace ticks may advance the command clock in {name}"
            );
            handled += 1;
        }
        drop(operation);
        tokio::time::resume();
        assert_eq!(handled, events.len());
        assert!(!state.is_funasr_ready());
        assert_eq!(state.inline_audio_transport(), None);
        assert!(state.engine.funasr_process.lock().await.is_none());
        *state.engine.funasr_process.lock().await = Some(test_process("healthy").await);
        state.set_funasr_ready(true);
        assert_eq!(
            send_command_to_server(&state, &ServerCommand::Status, Some(app.handle()))
                .await
                .unwrap()
                .success,
            Some(true)
        );
        let _ = std::fs::remove_file(gate);
        let _ = std::fs::remove_file(consumed);
    }
}
