use super::{send_command_to_server, ServerCommand};
use crate::state::{AppState, FunasrProcess};
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
    for line in std::io::stdin().lock().lines() {
        let request: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
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

async fn test_process(mode: &str) -> FunasrProcess {
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
