use std::io::{BufRead, Read, Write};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;

use super::{send_command_impl, sync_audio_processing_at_start, ServerCommand};
use crate::state::FunasrProcess;
use crate::utils::paths;

const PEER_ERROR: &str = "audio-start-peer-rejected-setting";

fn run_isolated(test_name: &str) -> bool {
    const CHILD_ENV: &str = "LIGHT_WHISPER_AUDIO_START_TEST_CHILD";
    if std::env::var(CHILD_ENV).as_deref() == Ok(test_name) {
        return false;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "light-whisper-audio-start-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env("LIGHT_WHISPER_DATA_DIR", &dir)
        .env(CHILD_ENV, test_name)
        .output()
        .unwrap();
    for filename in ["engine.json", "funasr_stderr.log"] {
        let file = dir.join(filename);
        if file.exists() {
            std::fs::remove_file(file).unwrap();
        }
    }
    std::fs::remove_dir(dir).unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

#[test]
#[ignore = "subprocess fixture, invoked by audio processing startup tests"]
fn audio_processing_start_test_peer() {
    let Ok(address) = std::env::var("LIGHT_WHISPER_AUDIO_START_TEST_GATE") else {
        return;
    };
    let mode = std::env::var("LIGHT_WHISPER_AUDIO_START_TEST_MODE").unwrap();
    // The loopback side channel releases an acknowledgement without sleeps.
    let mut gate = std::net::TcpStream::connect(address).unwrap();
    if mode == "bootstrap" {
        writeln!(
            gate,
            "{}",
            json!({
                "audio_processing": std::env::var("LIGHT_WHISPER_AUDIO_PROCESSING").unwrap(),
                "engine": std::env::var("LIGHT_WHISPER_ASR_ENGINE").unwrap(),
                "data_dir": std::env::var("LIGHT_WHISPER_DATA_DIR").unwrap(),
                "model_cache": std::env::var("HF_HUB_CACHE").unwrap(),
            })
        )
        .unwrap();
        gate.flush().unwrap();
    }
    println!("{{\"test_ready\":true}}");
    std::io::stdout().flush().unwrap();
    let mut requests = Vec::new();
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        requests.push(request.clone());
        let response = match request["action"].as_str().unwrap() {
            "set_audio_processing" => {
                if mode == "gated" {
                    writeln!(gate, "{request}").unwrap();
                    gate.flush().unwrap();
                    let mut release = [0];
                    gate.read_exact(&mut release).unwrap();
                    assert_eq!(release, [1]);
                }
                match mode.as_str() {
                    "error" => json!({
                        "request_id": request["request_id"],
                        "success": false,
                        "error": PEER_ERROR,
                    }),
                    "missing_success" => json!({ "request_id": request["request_id"] }),
                    _ => json!({ "request_id": request["request_id"], "success": true }),
                }
            }
            "status" => json!({
                "request_id": request["request_id"],
                "success": true,
                "message": serde_json::to_string(&requests).unwrap(),
            }),
            action => panic!("unexpected audio-start action: {action}"),
        };
        println!("{response}");
        std::io::stdout().flush().unwrap();
    }
}

async fn test_peer(mode: &str) -> (FunasrProcess, BufReader<TcpStream>) {
    configured_test_peer(mode, |_| {}).await
}

async fn configured_test_peer(
    mode: &str,
    configure: impl FnOnce(&mut Command),
) -> (FunasrProcess, BufReader<TcpStream>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "services::funasr_service::audio_processing_start_tests::audio_processing_start_test_peer",
            "--ignored",
            "--nocapture",
        ])
        .env(
            "LIGHT_WHISPER_AUDIO_START_TEST_GATE",
            listener.local_addr().unwrap().to_string(),
        )
        .env("LIGHT_WHISPER_AUDIO_START_TEST_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure(&mut command);
    let mut child = command
        .creation_flags(0x08000000)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let gate = tokio::time::timeout(Duration::from_secs(10), async {
        let (gate, _) = listener.accept().await.unwrap();
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).await.unwrap(), 0);
            if line.contains("\"test_ready\":true") {
                break;
            }
        }
        gate
    })
    .await
    .expect("audio-start peer must initialize");
    (
        FunasrProcess {
            child,
            stdin,
            stdout,
        },
        BufReader::new(gate),
    )
}

async fn audit(process: &mut FunasrProcess) -> Vec<Value> {
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        send_command_impl(process, &ServerCommand::Status),
    )
    .await
    .expect("following status must complete")
    .unwrap();
    assert_eq!(response.success, Some(true));
    serde_json::from_str(response.message.as_deref().unwrap()).unwrap()
}

#[tokio::test]
async fn managed_bootstrap_disables_saved_enhancement_before_asr_initialization() {
    if run_isolated(
        "services::funasr_service::audio_processing_start_tests::managed_bootstrap_disables_saved_enhancement_before_asr_initialization",
    ) {
        return;
    }
    let model_cache = paths::get_data_dir().join("models");
    paths::write_models_dir(Some(model_cache.to_str().unwrap())).unwrap();
    for engine in ["qwen3-asr-0.6b", "confucius4-r2t2"] {
        paths::write_audio_processing_enabled(true).unwrap();
        let (mut process, mut gate) = configured_test_peer("bootstrap", |command| {
            command.env("LIGHT_WHISPER_AUDIO_PROCESSING", "1");
            super::configure_server_environment(command, engine, 0);
            let configured_audio_processing = command
                .as_std()
                .get_envs()
                .find(|(key, _)| *key == "LIGHT_WHISPER_AUDIO_PROCESSING")
                .and_then(|(_, value)| value);
            assert_eq!(
                configured_audio_processing,
                Some(std::ffi::OsStr::new("0")),
                "managed ASR must bootstrap before initializing saved enhancement"
            );
        })
        .await;
        assert!(paths::read_audio_processing_enabled());
        let mut observed = String::new();
        tokio::time::timeout(Duration::from_secs(5), gate.read_line(&mut observed))
            .await
            .expect("child must report its bootstrap environment")
            .unwrap();
        let environment: Value = serde_json::from_str(&observed).unwrap();
        assert_eq!(environment["audio_processing"], "0");
        assert_eq!(environment["engine"], engine);
        assert_eq!(
            environment["data_dir"],
            paths::strip_win_prefix(paths::get_data_dir())
        );
        assert_eq!(
            environment["model_cache"],
            paths::strip_win_prefix(&model_cache)
        );

        // A disable during ASR loading must not initialize enhancement from
        // the old saved=true value after the child has bootstrapped disabled.
        paths::write_audio_processing_enabled(false).unwrap();
        let enabled_at_spawn = environment["audio_processing"] == "1";
        tokio::time::timeout(
            Duration::from_secs(5),
            sync_audio_processing_at_start(&mut process, enabled_at_spawn),
        )
        .await
        .expect("disabled enhancement must not prevent startup reconciliation")
        .unwrap();
        let requests = audit(&mut process).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["action"], "status");
        assert!(!paths::read_audio_processing_enabled());
    }
}

#[tokio::test]
async fn changed_startup_setting_reaches_peer_and_waits_for_success_ack() {
    if run_isolated(
        "services::funasr_service::audio_processing_start_tests::changed_startup_setting_reaches_peer_and_waits_for_success_ack",
    ) {
        return;
    }
    for enabled in [true, false] {
        paths::write_audio_processing_enabled(enabled).unwrap();
        let (mut process, mut gate) = test_peer("gated").await;
        let request = {
            let sync = sync_audio_processing_at_start(&mut process, !enabled);
            tokio::pin!(sync);
            let mut observed = String::new();
            tokio::time::timeout(Duration::from_secs(5), async {
                tokio::select! {
                    biased;
                    result = &mut sync => panic!("startup sync returned before peer acknowledgement: {result:?}"),
                    result = gate.read_line(&mut observed) => assert_ne!(result.unwrap(), 0),
                }
            })
            .await
            .expect("changed setting must reach the peer");
            let request: Value = serde_json::from_str(&observed).unwrap();
            assert_eq!(request["action"], "set_audio_processing");
            assert_eq!(request["enabled"].as_bool(), Some(enabled));
            assert!(request["request_id"].as_u64().is_some_and(|id| id > 0));
            gate.get_mut().write_all(&[1]).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), &mut sync)
                .await
                .expect("startup sync must finish after successful acknowledgement")
                .unwrap();
            request
        };
        let requests = audit(&mut process).await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], request);
        assert_eq!(requests[1]["action"], "status");
        assert_ne!(requests[0]["request_id"], requests[1]["request_id"]);
        assert_eq!(paths::read_audio_processing_enabled(), enabled);
    }
}

#[tokio::test]
async fn unchanged_startup_setting_sends_no_redundant_command() {
    if run_isolated(
        "services::funasr_service::audio_processing_start_tests::unchanged_startup_setting_sends_no_redundant_command",
    ) {
        return;
    }
    for enabled in [false, true] {
        paths::write_audio_processing_enabled(enabled).unwrap();
        let (mut process, _gate) = test_peer("automatic").await;
        tokio::time::timeout(
            Duration::from_secs(5),
            sync_audio_processing_at_start(&mut process, enabled),
        )
        .await
        .expect("unchanged setting must complete")
        .unwrap();
        let requests = audit(&mut process).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["action"], "status");
    }
}

#[tokio::test]
async fn startup_setting_requires_positive_ack_and_propagates_server_error() {
    if run_isolated(
        "services::funasr_service::audio_processing_start_tests::startup_setting_requires_positive_ack_and_propagates_server_error",
    ) {
        return;
    }
    paths::write_audio_processing_enabled(true).unwrap();
    for mode in ["error", "missing_success"] {
        let (mut process, _gate) = test_peer(mode).await;
        let error = tokio::time::timeout(
            Duration::from_secs(5),
            sync_audio_processing_at_start(&mut process, false),
        )
        .await
        .expect("server rejection must complete")
        .expect_err("startup setting requires an explicit success acknowledgement");
        if mode == "error" {
            assert!(error.to_string().contains(PEER_ERROR));
        }
        let requests = audit(&mut process).await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["action"], "set_audio_processing");
        assert_eq!(requests[0]["enabled"], true);
        assert!(requests[0]["request_id"].as_u64().is_some_and(|id| id > 0));
    }
}
