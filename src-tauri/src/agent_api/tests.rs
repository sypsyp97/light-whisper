//! Query real managed state/native pipes, plus one windowless Wry desktop-command case.
#![cfg(target_os = "windows")]

use super::controller::query;
use crate::state::user_profile::{HotWord, HotWordSource};
use crate::state::{
    AppState, FunasrProcess, PendingRecordingSession, RecordingMode, RecordingPhase, RecordingSlot,
    RecordingTrigger,
};
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetProcessId, OpenProcess, TerminateProcess, WaitForSingleObject, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

const CASE_ENV: &str = "LIGHT_WHISPER_AGENT_QUERY_CASE";
const FIXTURE_ENV: &str = "LIGHT_WHISPER_AGENT_QUERY_FIXTURE";
const OWNER_ENV: &str = "LIGHT_WHISPER_AGENT_QUERY_OWNER_PID";
const WAIT: Duration = Duration::from_secs(10);
const MAX_AUDIO: u64 = 64 * 1024 * 1024;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        // This directory is created here with a process/time identity, never user data.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Drain both pipes while the scenario runs. A full diagnostic pipe must not
// prevent the watchdog from observing/reaping its exact owned scenario.
struct Scenario {
    child: std::process::Child,
    stdout: Option<std::thread::JoinHandle<Vec<u8>>>,
    stderr: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl Scenario {
    fn spawn(command: &mut std::process::Command) -> Self {
        fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                // Retain bounded diagnostics; continue draining beyond them.
                pipe.by_ref()
                    .take(2 * 1024 * 1024)
                    .read_to_end(&mut bytes)
                    .unwrap();
                std::io::copy(&mut pipe, &mut std::io::sink()).unwrap();
                bytes
            })
        }
        let mut child = command
            .env(OWNER_ENV, std::process::id().to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = drain(child.stdout.take().unwrap());
        let stderr = drain(child.stderr.take().unwrap());
        Self {
            child,
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }

    fn output(&mut self) -> (std::process::ExitStatus, Vec<u8>, Vec<u8>) {
        let status = self.child.wait().unwrap();
        let stdout = self.stdout.take().unwrap().join().unwrap();
        let stderr = self.stderr.take().unwrap().join().unwrap();
        (status, stdout, stderr)
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        for reader in [&mut self.stdout, &mut self.stderr] {
            if let Some(reader) = reader.take() {
                let _ = reader.join();
            }
        }
    }
}

// paths::get_data_dir() caches its environment. Every scenario owns a fresh test process.
fn isolated(case: &str) -> bool {
    if std::env::var(CASE_ENV).as_deref() == Ok(case) {
        watch_owning_process();
        return true;
    }
    let root = Scratch(std::env::temp_dir().join(format!(
        "light-whisper-agent-query-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    for subdir in ["data", "temp", "control"] {
        std::fs::create_dir_all(root.0.join(subdir)).unwrap();
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &format!("agent_api::tests::{case}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CASE_ENV, case)
        .env("LIGHT_WHISPER_DATA_DIR", root.0.join("data"))
        .env("LIGHT_WHISPER_AGENT_QUERY_ROOT", &root.0)
        .env("TEMP", root.0.join("temp"))
        .env("TMP", root.0.join("temp"));
    let mut scenario = Scenario::spawn(&mut command);
    let until = Instant::now() + Duration::from_secs(45);
    loop {
        if scenario.child.try_wait().unwrap().is_some() {
            let (status, stdout, stderr) = scenario.output();
            assert!(
                status.success(),
                "isolated {case}:\n{}\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
            return false;
        }
        if Instant::now() >= until {
            scenario.child.kill().unwrap();
            let (_, stdout, stderr) = scenario.output();
            panic!(
                "isolated {case} timed out:\n{}\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

// Open while the owner is alive and before declaring readiness/acquiring
// children. The monitor waits on this exact kernel object, never a reused PID.
// Each nested scenario/fixture watches its own creator, so hard-kill cleanup
// cascades even when destructors cannot run and stdout handles were inherited.
fn watch_owning_process() {
    let pid: u32 = std::env::var(OWNER_ENV)
        .expect("owned child requires owner PID")
        .parse()
        .expect("numeric owned scenario PID");
    // SAFETY: read-only synchronization access to the live spawning owner.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!handle.is_null(), "owner must exist before child readiness");
    // Win32 kernel handles can be waited/closed from another thread. Transfer
    // sole ownership as an integer; do not add unsafe Send to any app type.
    let handle = handle as usize;
    std::thread::spawn(move || {
        let handle = handle as HANDLE;
        // SAFETY: this thread solely owns the valid handle opened above.
        let result = unsafe { WaitForSingleObject(handle, INFINITE) };
        // SAFETY: wait has returned; this is the only owner closing the handle.
        unsafe {
            CloseHandle(handle);
        }
        std::process::exit(if result == WAIT_OBJECT_0 { 0 } else { 93 });
    });
}

fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("LIGHT_WHISPER_AGENT_QUERY_ROOT").unwrap())
}

fn app(engine: &str) -> tauri::App<MockRuntime> {
    std::fs::write(
        root().join("data/engine.json"),
        json!({"engine":engine}).to_string(),
    )
    .unwrap();
    mock_builder()
        .manage(AppState::default())
        .build(mock_context(noop_assets()))
        .unwrap()
}

async fn call(
    app: &tauri::App<MockRuntime>,
    request: Value,
) -> Result<Value, crate::utils::AppError> {
    tokio::time::timeout(WAIT, query(app.handle(), request))
        .await
        .expect("query must settle")
}

fn audit() -> Vec<Value> {
    std::fs::read_to_string(root().join("control/audit.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn assert_runtime_unknown(value: &Value) {
    assert_eq!(value["runtime_observed"], false);
    assert!(
        value.get("runtime_engine").is_some_and(Value::is_null),
        "explicit unknown engine: {value}"
    );
}

fn assert_desktop(value: &Value, configured: &str) {
    assert_eq!(value["surface"], "desktop");
    assert_eq!(value["configured_engine"], configured);
}

fn assert_no_query_writes(app: &tauri::App<MockRuntime>, config: &[u8], profile: &Value) {
    assert_eq!(
        std::fs::read(root().join("data/engine.json")).unwrap(),
        config
    );
    assert_eq!(
        serde_json::to_value(app.state::<AppState>().snapshot_profile()).unwrap(),
        *profile
    );
    let mut files: Vec<_> = std::fs::read_dir(root().join("data"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    files.sort();
    assert_eq!(
        files,
        vec![std::ffi::OsString::from("engine.json")],
        "no history/profile side effects"
    );
    assert_eq!(
        std::fs::read_dir(root().join("temp")).unwrap().count(),
        0,
        "owned temporary audio removed"
    );
}

struct Native(HANDLE);

impl Native {
    fn alive(&self) -> bool {
        // SAFETY: handle opened by native(); remains owned until this guard drops.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }

    fn assert_reaped(&self) {
        // SAFETY: wait uses this guard's valid process handle; bounded to owned child.
        assert_eq!(
            unsafe { WaitForSingleObject(self.0, 10_000) },
            WAIT_OBJECT_0,
            "owned backend must terminate after owner abort/cleanup"
        );
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: handle has terminate/synchronize rights for this test's exact child.
        unsafe {
            if WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT {
                TerminateProcess(self.0, 91);
                WaitForSingleObject(self.0, 5_000);
            }
            CloseHandle(self.0);
        }
    }
}

async fn native<R: tauri::Runtime>(app: &tauri::App<R>, mode: &str) -> Native {
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "agent_api::tests::agent_query_native_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env(FIXTURE_ENV, mode)
        .env(OWNER_ENV, std::process::id().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    // SAFETY: pid belongs to the child just spawned; no process-name lookup.
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            pid,
        )
    };
    assert!(!handle.is_null());
    let owned = Native(handle);
    let stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    tokio::time::timeout(WAIT, async {
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).await.unwrap(), 0);
            if line.contains("\"fixture_ready\":true") {
                break;
            }
        }
    })
    .await
    .expect("native fixture ready");
    *app.state::<AppState>().engine.funasr_process.lock().await = Some(FunasrProcess {
        child,
        stdin,
        stdout,
    });
    app.state::<AppState>().set_funasr_ready(true);
    owned
}

async fn stop(app: &tauri::App<MockRuntime>, native: &Native) {
    crate::services::funasr_service::stop_server(app.state::<AppState>().inner())
        .await
        .unwrap();
    native.assert_reaped();
}

async fn entered() {
    tokio::time::timeout(WAIT, async {
        while !root().join("control/entered").exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual native child received request");
}

// Fixture-only containment control. Bypasses query because its baseline is
// declaration-only, and proves no claim about the controller implementation.
#[tokio::test]
#[ignore = "owned scenario for the fixture containment control"]
async fn fixture_owner_scenario() {
    if std::env::var("LIGHT_WHISPER_AGENT_FIXTURE_CONTROL").as_deref() != Ok("1") {
        return;
    }
    watch_owning_process();
    let app = app("confucius4-r2t2");
    let native = native(&app, "pending").await;
    let (path, _) = audio();
    {
        let state = app.state::<AppState>();
        let mut process = state.engine.funasr_process.lock().await;
        let process = process.as_mut().unwrap();
        let message = json!({"action":"transcribe","request_id":1,"audio_path":path});
        process
            .stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        process.stdin.flush().await.unwrap();
    }
    entered().await;
    // More than pipe capacity on each stream; owner-ready follows both writes.
    std::io::stdout()
        .write_all(&vec![b'o'; 1024 * 1024])
        .unwrap();
    std::io::stdout().flush().unwrap();
    std::io::stderr()
        .write_all(&vec![b'e'; 1024 * 1024])
        .unwrap();
    std::io::stderr().flush().unwrap();
    // SAFETY: this guard owns a still-live process handle from native().
    let pid = unsafe { GetProcessId(native.0) };
    assert_ne!(pid, 0);
    std::fs::write(root().join("control/owner-ready"), pid.to_string()).unwrap();
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[test]
fn fixture_pending_child_exits_when_owning_scenario_is_killed() {
    if !isolated("fixture_pending_child_exits_when_owning_scenario_is_killed") {
        return;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "agent_api::tests::fixture_owner_scenario",
            "--ignored",
            "--nocapture",
        ])
        .env("LIGHT_WHISPER_AGENT_FIXTURE_CONTROL", "1");
    let mut owner = Scenario::spawn(&mut command);
    let until = Instant::now() + WAIT;
    let ready = root().join("control/owner-ready");
    while !ready.exists() {
        assert!(
            owner.child.try_wait().unwrap().is_none(),
            "owned scenario must reach pending child barrier"
        );
        assert!(
            Instant::now() < until,
            "scenario must drain both diagnostic pipes and reach barrier"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let pid: u32 = std::fs::read_to_string(ready).unwrap().parse().unwrap();
    // Open while the owner retains its native guard, before terminating owner.
    // SAFETY: readiness identifies the live fixture spawned by this exact owner.
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            pid,
        )
    };
    assert!(!handle.is_null());
    let fixture = Native(handle);
    assert!(fixture.alive());
    assert!(root().join("control/entered").exists());
    assert!(!root().join("control/release").exists());
    owner.child.kill().unwrap();
    // Descendants can inherit the scenario's diagnostic write handles. Reap
    // the fixture before joining readers, so an uncontained fixture yields
    // this bounded assertion and its Native fallback still runs on failure.
    fixture.assert_reaped();
    let (_, stdout, stderr) = owner.output();
    assert!(
        stdout.len() >= 1024 * 1024,
        "stdout drained while owner lived"
    );
    assert!(
        stderr.len() >= 1024 * 1024,
        "stderr drained while owner lived"
    );
    std::fs::remove_dir_all(root()).expect("reaped owned subtree must be removable");
    assert!(!root().exists());
}

#[test]
#[ignore = "owned native JSONL fixture, invoked only by agent query tests"]
fn agent_query_native_fixture() {
    let Ok(mode) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    watch_owning_process();
    println!("{{\"fixture_ready\":true}}");
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let mut evidence = request.clone();
        if request["action"] == "transcribe" {
            let path = Path::new(request["audio_path"].as_str().expect("path transport"));
            evidence["received_audio"] = json!(std::fs::read(path).unwrap());
        }
        let mut output = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root().join("control/audit.jsonl"))
            .unwrap();
        writeln!(output, "{evidence}").unwrap();
        output.flush().unwrap();
        if mode == "pending" && request["action"] == "transcribe" {
            std::fs::write(root().join("control/entered"), b"received").unwrap();
            while !root().join("control/release").exists() {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        if mode == "eof" {
            return;
        }
        let mut response = match request["action"].as_str().unwrap() {
            "status" => json!({"success":true,"initialized":true,"model_loaded":true,
                "engine":"qwen3-asr-0.6b","device":"cpu","message":"native fixture"}),
            "stats" => json!({"success":true,"stats":{"transcription_count":7,
                "total_audio_duration":12.25,"average_inference_ms":321.5,
                "engine":"qwen3-asr-0.6b","backend":"fixture","extra_real_field":"retained"}}),
            "transcribe" => {
                json!({"success":true,"text":"合成测试 transcript","duration":0.75,"language":"de"})
            }
            "exit" => return,
            action => panic!("unexpected backend action {action}"),
        };
        if mode == "false" {
            response = json!({"success":false,"error":"native application failure"});
        } else if mode == "status_unready" {
            response = json!({"success":true,"initialized":false,"model_loaded":false,
                "engine":"qwen3-asr-0.6b","device":"cpu"});
        } else if mode == "missing_stats" {
            response.as_object_mut().unwrap().remove("stats");
        } else if mode == "text_missing" {
            response.as_object_mut().unwrap().remove("text");
        } else if mode == "text_null" {
            response["text"] = Value::Null;
        } else if mode == "text_empty" {
            response["text"] = json!("");
        } else if mode == "text_wrongtype" {
            response["text"] = json!(42);
        } else if let Some(bad) = mode.strip_prefix("stats_") {
            response["stats"] = match bad {
                "zero" => json!({"transcription_count":0,"total_audio_duration":0.0}),
                "array" => json!([]),
                "missing_count" => json!({"total_audio_duration":1.0}),
                "negative_count" => json!({"transcription_count":-1,"total_audio_duration":1.0}),
                "fraction_count" => json!({"transcription_count":1.5,"total_audio_duration":1.0}),
                "bool_count" => json!({"transcription_count":true,"total_audio_duration":1.0}),
                "missing_duration" => json!({"transcription_count":1}),
                "negative_duration" => json!({"transcription_count":1,"total_audio_duration":-1.0}),
                "string_duration" => json!({"transcription_count":1,"total_audio_duration":"1"}),
                _ => panic!("unknown fixture mode {mode}"),
            };
        }
        response["request_id"] = request["request_id"].clone();
        println!("{response}");
        std::io::stdout().flush().unwrap();
        if mode == "text_wrongtype" {
            // Existing typed decoding rejects this line; EOF bounds the
            // existing parser's wait without depending on its 60s timeout.
            return;
        }
    }
}

#[tokio::test]
async fn engine_get_reads_saved_selection_without_backend_or_writes() {
    if !isolated("engine_get_reads_saved_selection_without_backend_or_writes") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "healthy").await;
    let config = std::fs::read(root().join("data/engine.json")).unwrap();
    let profile = serde_json::to_value(app.state::<AppState>().snapshot_profile()).unwrap();
    let response = call(&app, json!({"action":"engine_get"}))
        .await
        .expect("saved engine query");
    assert_eq!(response["success"], true);
    assert_desktop(&response, "confucius4-r2t2");
    assert!(audit().is_empty());
    assert!(native.alive());
    assert_no_query_writes(&app, &config, &profile);
    stop(&app, &native).await;
}

#[tokio::test]
async fn status_observes_native_engine_separately_from_saved_selection() {
    if !isolated("status_observes_native_engine_separately_from_saved_selection") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "healthy").await;
    app.state::<AppState>().set_funasr_ready(false);
    let response = call(&app, json!({"action":"status"}))
        .await
        .expect("native status");
    assert_desktop(&response, "confucius4-r2t2");
    for key in [
        "success",
        "running",
        "ready",
        "model_loaded",
        "runtime_observed",
    ] {
        assert_eq!(response[key], true, "observed {key}: {response}");
    }
    assert_eq!(response["busy"], false);
    assert_eq!(response["runtime_engine"], "qwen3-asr-0.6b");
    assert_eq!(response["device"], "cpu");
    assert_eq!(audit().len(), 1);
    assert_eq!(audit()[0]["action"], "status");
    stop(&app, &native).await;
}

#[tokio::test]
async fn status_does_not_promote_stale_desktop_ready_over_native_unready() {
    if !isolated("status_does_not_promote_stale_desktop_ready_over_native_unready") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "status_unready").await;
    let response = call(&app, json!({"action":"status"}))
        .await
        .expect("unready status");
    assert_eq!(response["running"], true);
    assert_eq!(response["ready"], false);
    assert_eq!(response["model_loaded"], false);
    stop(&app, &native).await;
}

#[tokio::test]
async fn absent_backend_is_stopped_and_has_no_observed_runtime() {
    if !isolated("absent_backend_is_stopped_and_has_no_observed_runtime") {
        return;
    }
    let app = app("confucius4-r2t2");
    app.state::<AppState>().set_funasr_ready(true);
    let response = call(&app, json!({"action":"status"}))
        .await
        .expect("stopped status");
    assert_desktop(&response, "confucius4-r2t2");
    assert_eq!(response["success"], true);
    assert_eq!(response["running"], false);
    assert_eq!(response["ready"], false);
    assert_runtime_unknown(&response);
    assert!(audit().is_empty());
    assert!(
        call(&app, json!({"action":"stats"})).await.is_err(),
        "no fake zero counters"
    );
}

async fn busy_case(kind: &str) {
    let app = app("confucius4-r2t2");
    let native = native(&app, "healthy").await;
    let state = app.state::<AppState>();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let owner = if kind == "owner" {
        Some(state.engine.native_asr_owner.lock().await)
    } else {
        None
    };
    if kind == "recording" {
        *state.recording.recording.lock() =
            Some(RecordingSlot::Starting(PendingRecordingSession {
                session_id: 1,
                subtitle_show_gen: 0,
                trigger: RecordingTrigger::DictationOriginal,
                stop_flag: stop_flag.clone(),
                stop_notify: Arc::new(tokio::sync::Notify::new()),
            }));
    } else if kind == "processing" {
        state.recording.session_counter.store(1, Ordering::Release);
        state
            .recording
            .transition_snapshot_if_current(
                1,
                RecordingPhase::Processing,
                RecordingMode::Dictation,
                None,
                None,
            )
            .unwrap();
    }
    let configured = call(&app, json!({"action":"engine_get"}))
        .await
        .expect("saved selection remains readable while busy");
    assert_desktop(&configured, "confucius4-r2t2");
    assert_eq!(configured["success"], true);
    let status = call(&app, json!({"action":"status"}))
        .await
        .expect("busy status is informative");
    assert_desktop(&status, "confucius4-r2t2");
    assert_eq!(status["success"], true);
    assert_eq!(status["busy"], true);
    assert_runtime_unknown(&status);
    for key in ["running", "ready", "model_loaded"] {
        assert!(
            status.get(key).is_some_and(Value::is_null),
            "unknown busy {key}: {status}"
        );
    }
    for request in [
        json!({"action":"stats"}),
        json!({"action":"transcribe","audio_path":"missing-must-not-be-opened.wav"}),
    ] {
        let response = call(&app, request)
            .await
            .expect("busy rejection is structured");
        assert_desktop(&response, "confucius4-r2t2");
        assert_eq!(response["success"], false);
        assert_eq!(response["code"], "busy");
        assert_eq!(response["busy"], true);
        assert!(response["error"].as_str().is_some_and(|s| !s.is_empty()));
        assert_runtime_unknown(&response);
    }
    assert!(audit().is_empty(), "busy query must not send any IPC");
    assert!(native.alive());
    assert!(!stop_flag.load(Ordering::Acquire));
    assert_eq!(std::fs::read_dir(root().join("temp")).unwrap().count(), 0);
    if kind == "recording" {
        assert!(state.recording.recording.lock().is_some());
    }
    drop(owner);
    state.recording.recording.lock().take();
    stop(&app, &native).await;
}

#[tokio::test]
async fn recording_rejects_mutating_queries_without_touching_backend() {
    if isolated("recording_rejects_mutating_queries_without_touching_backend") {
        busy_case("recording").await;
    }
}

#[tokio::test]
async fn processing_rejects_mutating_queries_without_touching_backend() {
    if isolated("processing_rejects_mutating_queries_without_touching_backend") {
        busy_case("processing").await;
    }
}

#[tokio::test]
async fn occupied_native_owner_reports_busy_without_touching_backend() {
    if isolated("occupied_native_owner_reports_busy_without_touching_backend") {
        busy_case("owner").await;
    }
}

#[tokio::test]
async fn stats_returns_actual_native_payload_without_losing_fields() {
    if !isolated("stats_returns_actual_native_payload_without_losing_fields") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "healthy").await;
    let response = call(&app, json!({"action":"stats"}))
        .await
        .expect("native stats");
    assert_desktop(&response, "confucius4-r2t2");
    assert_eq!(response["success"], true);
    assert_eq!(
        response["stats"],
        json!({"transcription_count":7,"total_audio_duration":12.25,
        "average_inference_ms":321.5,"engine":"qwen3-asr-0.6b","backend":"fixture","extra_real_field":"retained"})
    );
    assert_eq!(audit().len(), 1);
    assert_eq!(audit()[0]["action"], "stats");
    stop(&app, &native).await;
    let native = self::native(&app, "stats_zero").await;
    let zero = call(&app, json!({"action":"stats"}))
        .await
        .expect("actual zero counters are valid");
    assert_eq!(
        zero["stats"],
        json!({"transcription_count":0,"total_audio_duration":0.0})
    );
    stop(&app, &native).await;
}

#[tokio::test]
async fn missing_or_invalid_stats_cannot_be_reported_as_success() {
    if !isolated("missing_or_invalid_stats_cannot_be_reported_as_success") {
        return;
    }
    let app = app("confucius4-r2t2");
    for mode in [
        "missing_stats",
        "stats_array",
        "stats_missing_count",
        "stats_negative_count",
        "stats_fraction_count",
        "stats_bool_count",
        "stats_missing_duration",
        "stats_negative_duration",
        "stats_string_duration",
    ] {
        let native = native(&app, mode).await;
        let before = audit().len();
        let error = call(&app, json!({"action":"stats"})).await.expect_err(mode);
        assert!(!error.to_string().is_empty());
        let commands = audit();
        assert_eq!(
            commands.len(),
            before + 1,
            "invalid stats must come from an actual native reply"
        );
        assert_eq!(commands.last().unwrap()["action"], "stats");
        stop(&app, &native).await;
    }
}

#[tokio::test]
async fn backend_errors_remain_explicit_in_status_and_stats() {
    if !isolated("backend_errors_remain_explicit_in_status_and_stats") {
        return;
    }
    let app = app("confucius4-r2t2");
    for action in ["status", "stats"] {
        for mode in ["false", "eof"] {
            let native = native(&app, mode).await;
            let before = audit().len();
            assert!(
                call(&app, json!({"action":action})).await.is_err(),
                "{action}/{mode} must fail explicitly"
            );
            let commands = audit();
            assert_eq!(
                commands.len(),
                before + 1,
                "error must follow actual native IPC"
            );
            assert_eq!(commands.last().unwrap()["action"], action);
            if mode == "false" {
                assert!(native.alive(), "application failure must preserve IPC");
            }
            stop(&app, &native).await;
        }
    }
}

fn audio() -> (PathBuf, Vec<u8>) {
    let bytes = crate::services::model_connection::TEST_AUDIO.to_vec();
    let path = root().join("input.wav");
    std::fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

fn profile_options(app: &tauri::App<MockRuntime>) {
    app.state::<AppState>().update_profile_mut(|profile| {
        profile.r2t2.context = "saved context".into();
        profile.r2t2.language = Some("en".into());
        profile.hot_words = vec![HotWord {
            text: "saved hot word".into(),
            weight: 9,
            source: HotWordSource::User,
            use_count: 0,
            last_used: 0,
        }];
    });
}

#[tokio::test]
async fn transcribe_forwards_explicit_overrides_and_cleans_owned_copy() {
    if !isolated("transcribe_forwards_explicit_overrides_and_cleans_owned_copy") {
        return;
    }
    let app = app("confucius4-r2t2");
    profile_options(&app);
    let native = native(&app, "healthy").await;
    let (path, bytes) = audio();
    let config = std::fs::read(root().join("data/engine.json")).unwrap();
    let profile = serde_json::to_value(app.state::<AppState>().snapshot_profile()).unwrap();
    let response = call(
        &app,
        json!({"action":"transcribe","audio_path":path,
        "options":{"language":"de","context":"explicit context"},"hot_words":["新词"]}),
    )
    .await
    .expect("native file transcription");
    assert_desktop(&response, "confucius4-r2t2");
    assert_eq!(response["success"], true);
    assert_eq!(response["text"], "合成测试 transcript");
    assert_eq!(response["duration"], 0.75);
    assert_eq!(response["language"], "de");
    let commands = audit();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["action"], "transcribe");
    assert_eq!(
        commands[0]["options"],
        json!({"language":"de","context":"explicit context"})
    );
    assert_eq!(commands[0]["hot_words"], json!(["新词"]));
    assert_eq!(commands[0]["received_audio"], json!(bytes));
    let owned_path = Path::new(commands[0]["audio_path"].as_str().unwrap());
    assert_ne!(owned_path, path);
    assert!(!owned_path.exists());
    assert_eq!(
        std::fs::read(path).unwrap(),
        bytes,
        "caller audio is retained"
    );
    assert_no_query_writes(&app, &config, &profile);
    stop(&app, &native).await;
}

#[tokio::test]
async fn transcribe_uses_current_profile_defaults_and_preserves_explicit_empty_values() {
    if !isolated("transcribe_uses_current_profile_defaults_and_preserves_explicit_empty_values") {
        return;
    }
    let app = app("confucius4-r2t2");
    profile_options(&app);
    let native = native(&app, "healthy").await;
    let (path, _) = audio();
    call(&app, json!({"action":"transcribe","audio_path":path}))
        .await
        .expect("profile defaults");
    app.state::<AppState>().update_profile_mut(|profile| {
        profile.r2t2.context = "updated context".into();
        profile.r2t2.language = Some("it".into());
    });
    call(
        &app,
        json!({"action":"transcribe","audio_path":path,
        "options":{"language":"fr"}}),
    )
    .await
    .expect("language-only override");
    call(
        &app,
        json!({"action":"transcribe","audio_path":path,
        "options":{"context":"partial context"}}),
    )
    .await
    .expect("context-only override");
    call(
        &app,
        json!({"action":"transcribe","audio_path":path,
        "options":{"language":"","context":""},"hot_words":[]}),
    )
    .await
    .expect("explicit empty overrides");
    let commands = audit();
    assert_eq!(commands.len(), 4);
    assert_eq!(
        commands[0]["options"],
        json!({"language":"en","context":"saved context"})
    );
    assert_eq!(commands[0]["hot_words"], json!(["saved hot word"]));
    assert_eq!(
        commands[1]["options"],
        json!({"language":"fr","context":"updated context"})
    );
    assert_eq!(
        commands[2]["options"],
        json!({"language":"it","context":"partial context"})
    );
    assert_eq!(commands[3]["options"], json!({"language":"","context":""}));
    assert_eq!(commands[3]["hot_words"], json!([]));
    stop(&app, &native).await;
}

#[tokio::test]
async fn invalid_cloud_and_oversize_audio_fail_before_any_backend_command() {
    if !isolated("invalid_cloud_and_oversize_audio_fail_before_any_backend_command") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "healthy").await;
    let oversized = root().join("oversized.wav");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(MAX_AUDIO + 1)
        .unwrap();
    for path in [
        root().join("missing.wav"),
        root().join("control"),
        oversized,
    ] {
        assert!(call(&app, json!({"action":"transcribe","audio_path":path}))
            .await
            .is_err());
    }
    let (path, _) = audio();
    for engine in ["glm-asr", "alibaba-asr"] {
        std::fs::write(
            root().join("data/engine.json"),
            json!({"engine":engine}).to_string(),
        )
        .unwrap();
        assert!(
            call(&app, json!({"action":"transcribe","audio_path":path}))
                .await
                .is_err(),
            "local only"
        );
    }
    assert!(audit().is_empty());
    assert!(native.alive());
    assert_eq!(std::fs::read_dir(root().join("temp")).unwrap().count(), 0);
    stop(&app, &native).await;
}

#[tokio::test]
async fn transcribe_cleans_owned_temporary_on_application_and_ipc_failure() {
    if !isolated("transcribe_cleans_owned_temporary_on_application_and_ipc_failure") {
        return;
    }
    let app = app("confucius4-r2t2");
    let (path, bytes) = audio();
    for mode in ["false", "eof"] {
        let native = native(&app, mode).await;
        let before = audit().len();
        let response = call(&app, json!({"action":"transcribe","audio_path":path})).await;
        match response {
            Ok(value) => {
                assert_eq!(value["success"], false);
                assert!(!value["error"].as_str().unwrap().is_empty());
            }
            Err(error) => assert!(!error.to_string().is_empty()),
        }
        let commands = audit();
        assert_eq!(
            commands.len(),
            before + 1,
            "failure cleanup must follow actual native transcription"
        );
        assert!(!Path::new(commands.last().unwrap()["audio_path"].as_str().unwrap()).exists());
        assert_eq!(std::fs::read_dir(root().join("temp")).unwrap().count(), 0);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(app
            .state::<AppState>()
            .engine
            .native_asr_owner
            .try_lock()
            .is_ok());
        if mode == "false" {
            assert!(native.alive());
        }
        stop(&app, &native).await;
    }
}

#[tokio::test]
async fn pending_transcription_retains_lease_until_actual_native_reply() {
    if !isolated("pending_transcription_retains_lease_until_actual_native_reply") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "pending").await;
    let (path, bytes) = audio();
    let mut operation = Box::pin(query(
        app.handle(),
        json!({"action":"transcribe","audio_path":path}),
    ));
    tokio::select! {
        result = &mut operation => panic!("must reach native pending barrier, got {result:?}"),
        _ = entered() => {}
    }
    assert!(app
        .state::<AppState>()
        .engine
        .native_asr_owner
        .try_lock()
        .is_err());
    std::fs::write(&path, b"changed caller file after admission").unwrap();
    let commands = audit();
    assert_eq!(commands[0]["received_audio"], json!(bytes));
    let owned = Path::new(commands[0]["audio_path"].as_str().unwrap());
    assert_eq!(
        std::fs::read(owned).unwrap(),
        bytes,
        "backend consumes an owned stable copy"
    );
    for request in [
        json!({"action":"stats"}),
        json!({"action":"transcribe","audio_path":path}),
    ] {
        let response = call(&app, request).await.expect("competing request busy");
        assert_eq!(response["code"], "busy");
        assert_eq!(response["success"], false);
    }
    let status = call(&app, json!({"action":"status"}))
        .await
        .expect("pending busy status");
    assert_eq!(status["busy"], true);
    assert_runtime_unknown(&status);
    assert_eq!(audit().len(), 1);
    assert!(native.alive());
    std::fs::write(root().join("control/release"), b"reply now").unwrap();
    assert_eq!(
        tokio::time::timeout(WAIT, &mut operation)
            .await
            .unwrap()
            .unwrap()["success"],
        true
    );
    drop(operation);
    assert!(!owned.exists());
    assert!(app
        .state::<AppState>()
        .engine
        .native_asr_owner
        .try_lock()
        .is_ok());
    assert!(native.alive());
    stop(&app, &native).await;
}

#[tokio::test]
async fn owner_abort_cleans_owned_audio_and_reaps_pending_backend() {
    if !isolated("owner_abort_cleans_owned_audio_and_reaps_pending_backend") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "pending").await;
    let (path, bytes) = audio();
    let mut operation = Box::pin(query(
        app.handle(),
        json!({"action":"transcribe","audio_path":path}),
    ));
    tokio::select! {
        result = &mut operation => panic!("must reach native pending barrier, got {result:?}"),
        _ = entered() => {}
    }
    let commands = audit();
    let owned = Path::new(commands[0]["audio_path"].as_str().unwrap());
    assert!(owned.exists());
    // Dropping the actual query models app-owned cancellation, never client disconnect.
    drop(operation);
    native.assert_reaped();
    assert!(
        !owned.exists(),
        "RAII cleanup must survive future cancellation"
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert!(!app.state::<AppState>().is_funasr_ready());
    assert!(app
        .state::<AppState>()
        .engine
        .funasr_process
        .lock()
        .await
        .is_none());
    assert!(app
        .state::<AppState>()
        .engine
        .native_asr_owner
        .try_lock()
        .is_ok());
}

#[tokio::test]
async fn qwen_transcribe_forwards_explicit_language_context_and_hot_words() {
    if !isolated("qwen_transcribe_forwards_explicit_language_context_and_hot_words") {
        return;
    }
    let app = app("qwen3-asr-0.6b");
    let native = native(&app, "healthy").await;
    let (path, _) = audio();
    call(
        &app,
        json!({"action":"transcribe","audio_path":path,
        "options":{"language":"de","context":"Qwen context"},"hot_words":["Qwen term"]}),
    )
    .await
    .expect("explicit options apply to both local engines");
    let commands = audit();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0]["options"],
        json!({"language":"de","context":"Qwen context"})
    );
    assert_eq!(commands[0]["hot_words"], json!(["Qwen term"]));
    stop(&app, &native).await;
}

#[tokio::test]
async fn unsupported_actions_and_malformed_transcribe_fields_have_no_effects() {
    if !isolated("unsupported_actions_and_malformed_transcribe_fields_have_no_effects") {
        return;
    }
    let app = app("confucius4-r2t2");
    let native = native(&app, "healthy").await;
    let (path, _) = audio();
    for request in [
        json!({"action":"exit"}),
        json!({"action":"start_recording"}),
        json!([]),
        json!({"action":"transcribe","audio_path":path,"options":{"language":2}}),
        json!({"action":"transcribe","audio_path":path,"options":{"context":false}}),
        json!({"action":"transcribe","audio_path":path,"hot_words":[2]}),
    ] {
        assert!(call(&app, request).await.is_err());
    }
    assert!(audit().is_empty());
    assert!(native.alive());
    assert!(app.state::<AppState>().recording.recording.lock().is_none());
    assert_eq!(std::fs::read_dir(root().join("temp")).unwrap().count(), 0);
    stop(&app, &native).await;
}

// Validate both local selections before asserting the response contract, so
// each missing/null-text RED records both engines rather than stopping at one.
async fn transcript_field_results(
    mode: &str,
) -> Vec<(&'static str, Result<Value, crate::utils::AppError>)> {
    let mut results = Vec::new();
    for engine in ["qwen3-asr-0.6b", "confucius4-r2t2"] {
        let app = app(engine);
        let native = native(&app, mode).await;
        let (path, bytes) = audio();
        let before = audit().len();
        let config = std::fs::read(root().join("data/engine.json")).unwrap();
        let profile = serde_json::to_value(app.state::<AppState>().snapshot_profile()).unwrap();
        let response = call(&app, json!({"action":"transcribe","audio_path":path})).await;
        let commands = audit();
        assert_eq!(
            commands.len(),
            before + 1,
            "response must follow actual native transcription"
        );
        let command = commands.last().unwrap();
        assert_eq!(command["action"], "transcribe");
        assert_eq!(command["received_audio"], json!(bytes));
        assert!(
            !Path::new(command["audio_path"].as_str().unwrap()).exists(),
            "owned audio copy cleaned"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "caller audio retained"
        );
        assert!(
            app.state::<AppState>()
                .engine
                .native_asr_owner
                .try_lock()
                .is_ok(),
            "query permit released"
        );
        assert_no_query_writes(&app, &config, &profile);
        if mode != "text_wrongtype" {
            assert!(
                native.alive(),
                "valid JSON field validation does not kill healthy IPC"
            );
        }
        stop(&app, &native).await;
        results.push((engine, response));
    }
    results
}

#[tokio::test]
async fn transcribe_missing_text_field_is_an_error_for_both_local_engines() {
    if !isolated("transcribe_missing_text_field_is_an_error_for_both_local_engines") {
        return;
    }
    let results = transcript_field_results("text_missing").await;
    assert!(
        results.iter().all(|(_, response)| response.is_err()),
        "successful native reply without a transcript field must fail: {results:?}"
    );
}

#[tokio::test]
async fn transcribe_null_text_field_is_an_error_for_both_local_engines() {
    if !isolated("transcribe_null_text_field_is_an_error_for_both_local_engines") {
        return;
    }
    let results = transcript_field_results("text_null").await;
    assert!(
        results.iter().all(|(_, response)| response.is_err()),
        "successful native reply with null transcript must fail: {results:?}"
    );
}

#[tokio::test]
async fn transcribe_explicit_empty_text_is_valid_silence_for_both_local_engines() {
    if !isolated("transcribe_explicit_empty_text_is_valid_silence_for_both_local_engines") {
        return;
    }
    for (engine, response) in transcript_field_results("text_empty").await {
        let response = response.expect("explicit silence is a valid native transcript");
        assert_desktop(&response, engine);
        assert_eq!(response["success"], true);
        assert_eq!(
            response["text"].as_str(),
            Some(""),
            "silence must remain empty"
        );
    }
}

#[tokio::test]
async fn transcribe_wrong_type_text_remains_an_error_for_both_local_engines() {
    if !isolated("transcribe_wrong_type_text_remains_an_error_for_both_local_engines") {
        return;
    }
    let results = transcript_field_results("text_wrongtype").await;
    assert!(
        results.iter().all(|(_, response)| response.is_err()),
        "existing typed decoding must reject non-string transcript: {results:?}"
    );
}

#[tokio::test]
async fn status_during_native_start_before_child_acceptance_is_unknown_not_stopped() {
    if !isolated("status_during_native_start_before_child_acceptance_is_unknown_not_stopped") {
        return;
    }
    let app = app("confucius4-r2t2");
    let state = app.state::<AppState>();
    assert!(state.engine.try_begin_funasr_start(27));
    assert!(state.engine.funasr_process.lock().await.is_none());
    let response = call(&app, json!({"action":"status"}))
        .await
        .expect("starting status is inspectable");
    assert_desktop(&response, "confucius4-r2t2");
    assert_eq!(response["success"], true);
    assert_eq!(
        response["starting"], true,
        "native startup must be visible: {response}"
    );
    assert!(response.get("running").is_some_and(Value::is_null));
    assert_eq!(response["ready"], false);
    assert_runtime_unknown(&response);
    assert!(audit().is_empty(), "status must not start or probe a child");
    assert!(
        state.engine.owns_funasr_start(27),
        "query preserves startup owner"
    );
    state.engine.finish_funasr_start(27);
}

#[tokio::test(flavor = "current_thread")]
async fn desktop_set_engine_rejects_busy_and_emits_only_after_local_stop() {
    if !isolated("desktop_set_engine_rejects_busy_and_emits_only_after_local_stop") {
        return;
    }
    use std::future::Future;
    use tauri::Listener;

    // isolated() sets these before process startup, guarding paths' OnceLock.
    // Check the exact owned environment before AppState or Wry construction.
    let data = root().join("data");
    let temporary = root().join("temp");
    assert_eq!(
        std::env::var_os("LIGHT_WHISPER_DATA_DIR"),
        Some(data.clone().into_os_string())
    );
    for name in ["TEMP", "TMP"] {
        assert_eq!(
            std::env::var_os(name),
            Some(temporary.clone().into_os_string())
        );
    }
    assert_eq!(crate::utils::paths::get_data_dir(), &data);
    assert_eq!(std::env::temp_dir(), temporary);
    std::fs::write(
        data.join("engine.json"),
        br#"{"engine":"qwen3-asr-0.6b","preserve":"owned-test-field"}"#,
    )
    .unwrap();

    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().app.tray_icon = None;
    let app = tauri::Builder::<tauri::Wry>::new()
        .any_thread()
        .device_event_filter(tauri::DeviceEventFilter::Always)
        .manage(AppState::default())
        .build(context)
        .unwrap();
    assert!(app.config().app.windows.is_empty());
    assert!(app.webview_windows().is_empty());
    let state = app.state::<AppState>();

    #[derive(Debug, PartialEq)]
    struct Snapshot {
        config: Vec<u8>,
        profile: Value,
        data_files: Vec<std::ffi::OsString>,
        audit: Vec<Value>,
        pid: Option<u32>,
        generation: u64,
        ready: bool,
        inline: Option<bool>,
        starting: bool,
        recording: Option<u64>,
        recording_snapshot: Option<crate::state::RecordingSnapshot>,
        asr_available: bool,
        lifecycle_available: bool,
        download: Option<(u64, Option<String>, bool, bool)>,
    }

    async fn snapshot(app: &tauri::App<tauri::Wry>) -> Snapshot {
        let state = app.state::<AppState>();
        let mut data_files: Vec<_> = std::fs::read_dir(root().join("data"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        data_files.sort();
        let pid = state
            .engine
            .funasr_process
            .lock()
            .await
            .as_ref()
            .and_then(|p| p.child.id());
        let download = state
            .engine
            .download_task
            .lock()
            .await
            .as_ref()
            .map(|task| {
                (
                    task.id,
                    task.request_id.clone(),
                    task.cancel.is_some(),
                    *task.done.borrow(),
                )
            });
        let recording = state
            .recording
            .recording
            .lock()
            .as_ref()
            .map(RecordingSlot::session_id);
        let recording_snapshot = state.recording.snapshot();
        let asr_available = state.engine.native_asr_owner.try_lock().is_ok();
        let lifecycle_available = state.engine.funasr_lifecycle_op.try_lock().is_ok();
        Snapshot {
            config: std::fs::read(root().join("data/engine.json")).unwrap(),
            profile: serde_json::to_value(state.snapshot_profile()).unwrap(),
            data_files,
            audit: audit(),
            pid,
            generation: state.engine.funasr_generation.load(Ordering::SeqCst),
            ready: state.is_funasr_ready(),
            inline: state.inline_audio_transport(),
            starting: state.engine.is_funasr_starting(),
            recording,
            recording_snapshot,
            asr_available,
            lifecycle_available,
            download,
        }
    }

    #[derive(Debug)]
    struct EngineEvent {
        engine: String,
        child_reaped: bool,
        process_absent: bool,
        generation: u64,
        ready: bool,
        inline: Option<bool>,
        starting: bool,
        lifecycle_held: bool,
        asr_held: bool,
    }

    struct ListenerGuard {
        app: tauri::AppHandle,
        id: tauri::EventId,
    }
    impl Drop for ListenerGuard {
        fn drop(&mut self) {
            self.app.unlisten(self.id);
        }
    }

    fn observe(
        app: &tauri::App<tauri::Wry>,
        native: &Native,
        events: Arc<std::sync::Mutex<Vec<EngineEvent>>>,
    ) -> ListenerGuard {
        let handle = app.handle().clone();
        let child = native.0 as usize;
        let id = app.listen("engine-config-changed", move |event| {
            let state = handle.state::<AppState>();
            // SAFETY: Native owns this exact process handle. The later-declared
            // ListenerGuard unlistens before Native drops, including on panic.
            let child_reaped = unsafe { WaitForSingleObject(child as HANDLE, 0) == WAIT_OBJECT_0 };
            events.lock().unwrap().push(EngineEvent {
                engine: serde_json::from_str(event.payload()).unwrap(),
                child_reaped,
                process_absent: state
                    .engine
                    .funasr_process
                    .try_lock()
                    .is_ok_and(|p| p.is_none()),
                generation: state.engine.funasr_generation.load(Ordering::SeqCst),
                ready: state.is_funasr_ready(),
                inline: state.inline_audio_transport(),
                starting: state.engine.is_funasr_starting(),
                lifecycle_held: state.engine.funasr_lifecycle_op.try_lock().is_err(),
                asr_held: state.engine.native_asr_owner.try_lock().is_err(),
            });
        });
        ListenerGuard {
            app: app.handle().clone(),
            id,
        }
    }

    async fn reject(
        app: &tauri::App<tauri::Wry>,
        native: &Native,
        events: &Arc<std::sync::Mutex<Vec<EngineEvent>>>,
        engine: &str,
    ) {
        let before = snapshot(app).await;
        let event_count = events.lock().unwrap().len();
        let result = tokio::time::timeout(
            WAIT,
            crate::commands::funasr::set_engine(app.handle().clone(), app.state(), engine.into()),
        )
        .await
        .expect("rejection settles");
        assert!(
            result.is_err(),
            "desktop command must reject {engine}: {result:?}"
        );
        assert_eq!(
            snapshot(app).await,
            before,
            "rejection preserves state and owned files"
        );
        assert_eq!(
            events.lock().unwrap().len(),
            event_count,
            "rejection emits no engine change"
        );
        assert!(native.alive(), "rejection must not stop the actual backend");
    }

    for (index, engine) in ["confucius4-r2t2", "qwen3-asr-0.6b"]
        .into_iter()
        .enumerate()
    {
        let backend = native(&app, "healthy").await;
        state.set_inline_audio_transport(Some(true));
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        // Declared after backend: unlink the AppHandle cycle before its handle closes.
        let listener = observe(&app, &backend, events.clone());

        if index == 0 {
            reject(&app, &backend, &events, "unsupported-owned-test-engine").await;
            let stop_flag = Arc::new(AtomicBool::new(false));
            *state.recording.recording.lock() =
                Some(RecordingSlot::Starting(PendingRecordingSession {
                    session_id: 1,
                    subtitle_show_gen: 0,
                    trigger: RecordingTrigger::DictationOriginal,
                    stop_flag: stop_flag.clone(),
                    stop_notify: Arc::new(tokio::sync::Notify::new()),
                }));
            reject(&app, &backend, &events, engine).await;
            assert!(!stop_flag.load(Ordering::Acquire));
            state.recording.recording.lock().take();

            state.recording.session_counter.store(1, Ordering::Release);
            state
                .recording
                .transition_snapshot_if_current(
                    1,
                    RecordingPhase::Processing,
                    RecordingMode::Dictation,
                    None,
                    None,
                )
                .unwrap();
            reject(&app, &backend, &events, engine).await;
            assert!(state.recording.clear_snapshot_if_session(1));

            let owner = state.engine.native_asr_owner.lock().await;
            reject(&app, &backend, &events, engine).await;
            drop(owner);

            let (cancel, mut cancelled) = tokio::sync::oneshot::channel();
            let (done_sender, done) = tokio::sync::watch::channel(false);
            *state.engine.download_task.lock().await = Some(crate::state::DownloadTask {
                id: 1,
                request_id: Some("owned-test-download".into()),
                cancel: Some(cancel),
                done,
            });
            reject(&app, &backend, &events, engine).await;
            assert!(matches!(
                cancelled.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            state.engine.download_task.lock().await.take();
            drop(done_sender);
        }

        let lifecycle = state.engine.funasr_lifecycle_op.lock().await;
        let before = snapshot(&app).await;
        let generation = before.generation;
        let mut switch = Box::pin(crate::commands::funasr::set_engine(
            app.handle().clone(),
            app.state(),
            engine.into(),
        ));
        let pending = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(switch.as_mut().poll(cx).is_pending())
        })
        .await;
        assert!(
            pending,
            "valid command waits on the actual shared lifecycle mutex"
        );
        assert_eq!(snapshot(&app).await, before);
        assert!(events.lock().unwrap().is_empty());
        assert!(backend.alive());
        drop(lifecycle);

        let selected = tokio::time::timeout(WAIT, &mut switch)
            .await
            .expect("local switch settles")
            .unwrap();
        assert_eq!(selected, engine);
        backend.assert_reaped();
        let config: Value =
            serde_json::from_slice(&std::fs::read(data.join("engine.json")).unwrap()).unwrap();
        assert_eq!(config["engine"], engine);
        assert_eq!(config["preserve"], "owned-test-field");
        {
            let observed = events.lock().unwrap();
            assert_eq!(
                observed.len(),
                1,
                "exactly one native config event per completed switch"
            );
            let event = &observed[0];
            assert_eq!(event.engine, engine);
            assert!(
                event.child_reaped && event.process_absent,
                "event follows actual settled stop: {event:?}"
            );
            assert_eq!(event.generation, generation + 1);
            assert!(!event.ready && event.inline.is_none() && !event.starting);
            assert!(
                event.lifecycle_held && event.asr_held,
                "emission stays inside shared ownership: {event:?}"
            );
        }
        assert!(
            state.engine.funasr_process.lock().await.is_none(),
            "switch never auto-starts a backend"
        );
        assert!(state.engine.native_asr_owner.try_lock().is_ok());
        assert!(state.engine.funasr_lifecycle_op.try_lock().is_ok());
        assert!(!state.engine.is_funasr_starting());
        assert!(app.config().app.windows.is_empty() && app.webview_windows().is_empty());
        drop(listener);
    }
    assert_eq!(std::fs::read_dir(temporary).unwrap().count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn controller_engine_set_admission_rejections_preserve_owned_state() {
    if !isolated("controller_engine_set_admission_rejections_preserve_owned_state") {
        return;
    }
    use tauri::Listener;

    let data = root().join("data");
    let temporary = root().join("temp");
    assert_eq!(
        std::env::var_os("LIGHT_WHISPER_DATA_DIR"),
        Some(data.clone().into_os_string())
    );
    for name in ["TEMP", "TMP"] {
        assert_eq!(
            std::env::var_os(name),
            Some(temporary.clone().into_os_string())
        );
    }
    assert_eq!(crate::utils::paths::get_data_dir(), &data);
    assert_eq!(std::env::temp_dir(), temporary);
    std::fs::write(
        data.join("engine.json"),
        br#"{"engine":"qwen3-asr-0.6b","preserve":"owned-admission-field"}"#,
    )
    .unwrap();

    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().app.tray_icon = None;
    let app = tauri::Builder::<tauri::Wry>::new()
        .any_thread()
        .device_event_filter(tauri::DeviceEventFilter::Always)
        .manage(AppState::default())
        .build(context)
        .unwrap();
    assert!(app.config().app.windows.is_empty());
    assert!(app.webview_windows().is_empty());
    let state = app.state::<AppState>();
    std::fs::write(
        data.join("user_profile.json"),
        serde_json::to_vec(&state.snapshot_profile()).unwrap(),
    )
    .unwrap();

    // A real nonzero startup owner, as used before child acceptance in start_server.
    const START_OWNER: u64 = 27;
    #[derive(Debug, PartialEq)]
    struct Snapshot {
        files: Vec<(std::ffi::OsString, Vec<u8>)>,
        profile: Value,
        audit: Vec<Value>,
        pid: Option<u32>,
        generation: u64,
        ready: bool,
        inline: Option<bool>,
        starting: bool,
        owns_start: bool,
        starting_process: bool,
        recording: Option<u64>,
        recording_snapshot: Option<crate::state::RecordingSnapshot>,
        asr_available: bool,
        lifecycle_available: bool,
        download: Option<(u64, Option<String>, bool, bool)>,
    }

    async fn snapshot(app: &tauri::App<tauri::Wry>) -> Snapshot {
        let state = app.state::<AppState>();
        let mut files: Vec<_> = std::fs::read_dir(root().join("data"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let pid = state
            .engine
            .funasr_process
            .lock()
            .await
            .as_ref()
            .and_then(|process| process.child.id());
        let download = state
            .engine
            .download_task
            .lock()
            .await
            .as_ref()
            .map(|task| {
                (
                    task.id,
                    task.request_id.clone(),
                    task.cancel.is_some(),
                    *task.done.borrow(),
                )
            });
        let recording = state
            .recording
            .recording
            .lock()
            .as_ref()
            .map(RecordingSlot::session_id);
        let recording_snapshot = state.recording.snapshot();
        let asr_available = state.engine.native_asr_owner.try_lock().is_ok();
        let lifecycle_available = state.engine.funasr_lifecycle_op.try_lock().is_ok();
        let starting_process = state.engine.funasr_starting_process.lock().is_some();
        Snapshot {
            files,
            profile: serde_json::to_value(state.snapshot_profile()).unwrap(),
            audit: audit(),
            pid,
            generation: state.engine.funasr_generation.load(Ordering::SeqCst),
            ready: state.is_funasr_ready(),
            inline: state.inline_audio_transport(),
            starting: state.engine.is_funasr_starting(),
            owns_start: state.engine.owns_funasr_start(START_OWNER),
            starting_process,
            recording,
            recording_snapshot,
            asr_available,
            lifecycle_available,
            download,
        }
    }

    struct ListenerGuard {
        app: tauri::AppHandle,
        id: tauri::EventId,
    }
    impl Drop for ListenerGuard {
        fn drop(&mut self) {
            self.app.unlisten(self.id);
        }
    }

    async fn reject(
        app: &tauri::App<tauri::Wry>,
        native: Option<&Native>,
        events: &Arc<std::sync::Mutex<Vec<Value>>>,
        engine: &str,
        shutdown_initial: bool,
        code: &str,
    ) {
        let before = snapshot(app).await;
        assert!(before.asr_available && before.lifecycle_available);
        assert!(before.recording.is_none() && before.recording_snapshot.is_none());
        let event_count = events.lock().unwrap().len();
        let (shutdown_owner, shutdown) = tokio::sync::watch::channel(shutdown_initial);
        let response = tokio::time::timeout(
            WAIT,
            super::controller::dispatch(
                app.handle(),
                json!({"action":"engine_set", "engine":engine}),
                shutdown,
            ),
        )
        .await
        .expect("controller admission settles");
        assert_eq!(*shutdown_owner.borrow(), shutdown_initial);
        assert_eq!(
            response["code"].as_str(),
            Some(code),
            "controller must reject {engine} with {code}: {response}"
        );
        assert_eq!(response["success"], false);
        assert_eq!(response["surface"], "desktop");
        assert_eq!(
            snapshot(app).await,
            before,
            "rejection has no owned effects"
        );
        assert_eq!(events.lock().unwrap().len(), event_count);
        if let Some(native) = native {
            assert!(native.alive(), "rejection preserves the actual backend");
        }
    }

    struct StartingOwner<'a>(&'a crate::state::EngineState);
    impl Drop for StartingOwner<'_> {
        fn drop(&mut self) {
            self.0.finish_funasr_start(START_OWNER);
        }
    }
    let events = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let observed = events.clone();
    let listener = ListenerGuard {
        app: app.handle().clone(),
        id: app.listen("engine-config-changed", move |event| {
            observed
                .lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap());
        }),
    };

    // start_server marks this owner before find_engine/child acceptance. Keep the
    // accepted process absent and both admission mutexes free, as in that phase.
    assert!(state.engine.try_begin_funasr_start(START_OWNER));
    let starting = StartingOwner(&state.engine);
    assert!(state.engine.funasr_process.lock().await.is_none());
    assert!(state.engine.funasr_starting_process.lock().is_none());
    assert!(!state.is_funasr_ready() && state.inline_audio_transport().is_none());
    reject(&app, None, &events, "confucius4-r2t2", false, "busy").await;
    assert!(state.engine.owns_funasr_start(START_OWNER));
    drop(starting);
    assert!(!state.engine.is_funasr_starting());

    let backend = native(&app, "healthy").await;
    state.set_inline_audio_transport(Some(true));
    reject(
        &app,
        Some(&backend),
        &events,
        "confucius4-r2t2",
        true,
        "owner_shutdown",
    )
    .await;

    let (cancel, mut cancelled) = tokio::sync::oneshot::channel();
    let (done_owner, done) = tokio::sync::watch::channel(false);
    *state.engine.download_task.lock().await = Some(crate::state::DownloadTask {
        id: 1,
        request_id: Some("owned-admission-download".into()),
        cancel: Some(cancel),
        done,
    });
    reject(
        &app,
        Some(&backend),
        &events,
        "confucius4-r2t2",
        false,
        "busy",
    )
    .await;
    assert!(matches!(
        cancelled.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    assert!(!*done_owner.borrow());
    state.engine.download_task.lock().await.take();
    drop(done_owner);

    reject(
        &app,
        Some(&backend),
        &events,
        "unsupported-owned-admission-engine",
        false,
        "invalid_request",
    )
    .await;
    assert!(events.lock().unwrap().is_empty());
    assert!(audit().is_empty());
    crate::services::funasr_service::stop_server(state.inner())
        .await
        .unwrap();
    backend.assert_reaped();
    assert!(state.engine.funasr_process.lock().await.is_none());
    assert!(state.engine.native_asr_owner.try_lock().is_ok());
    assert!(state.engine.funasr_lifecycle_op.try_lock().is_ok());
    assert!(state.engine.download_task.lock().await.is_none());
    assert!(app.config().app.windows.is_empty() && app.webview_windows().is_empty());
    assert_eq!(std::fs::read_dir(temporary).unwrap().count(), 0);
    drop(listener);
}

// These cases drive the real controller. The competing actor uses the maintained
// AppState publication seam under the same mutexes as commands/audio.rs and
// start_server; it does not call microphone capture, foreground APIs or window UI.
fn lifecycle_admission_app() -> tauri::App<tauri::Wry> {
    let data = root().join("data");
    let temporary = root().join("temp");
    assert_eq!(crate::utils::paths::get_data_dir(), &data);
    assert_eq!(std::env::temp_dir(), temporary);
    std::fs::write(
        data.join("engine.json"),
        br#"{"engine":"qwen3-asr-0.6b","preserve":"owned-concurrent-admission-field"}"#,
    )
    .unwrap();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().app.tray_icon = None;
    let app = tauri::Builder::<tauri::Wry>::new()
        .any_thread()
        .device_event_filter(tauri::DeviceEventFilter::Always)
        .manage(AppState::default())
        .build(context)
        .unwrap();
    assert!(app.config().app.windows.is_empty() && app.webview_windows().is_empty());
    app
}

const CONCURRENT_START_OWNER: u64 = 41;

#[derive(Debug, PartialEq)]
struct LifecycleAdmissionSnapshot {
    files: Vec<(std::ffi::OsString, Vec<u8>)>,
    profile: Value,
    audit: Vec<Value>,
    pid: Option<u32>,
    generation: u64,
    ready: bool,
    inline: Option<bool>,
    starting: bool,
    owns_start: bool,
    starting_process: bool,
    recording: Option<u64>,
    recording_snapshot: Option<crate::state::RecordingSnapshot>,
}

async fn lifecycle_admission_snapshot(app: &tauri::App<tauri::Wry>) -> LifecycleAdmissionSnapshot {
    let state = app.state::<AppState>();
    let mut files: Vec<_> = std::fs::read_dir(root().join("data"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), std::fs::read(entry.path()).unwrap())
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let pid = state
        .engine
        .funasr_process
        .lock()
        .await
        .as_ref()
        .and_then(|process| process.child.id());
    let recording = state
        .recording
        .recording
        .lock()
        .as_ref()
        .map(RecordingSlot::session_id);
    let starting_process = state.engine.funasr_starting_process.lock().is_some();
    LifecycleAdmissionSnapshot {
        files,
        profile: serde_json::to_value(state.snapshot_profile()).unwrap(),
        audit: audit(),
        pid,
        generation: state.engine.funasr_generation.load(Ordering::SeqCst),
        ready: state.is_funasr_ready(),
        inline: state.inline_audio_transport(),
        starting: state.engine.is_funasr_starting(),
        owns_start: state.engine.owns_funasr_start(CONCURRENT_START_OWNER),
        starting_process,
        recording,
        recording_snapshot: state.recording.snapshot(),
    }
}

struct LifecycleAdmissionListener {
    app: tauri::AppHandle,
    id: tauri::EventId,
}

impl Drop for LifecycleAdmissionListener {
    fn drop(&mut self) {
        use tauri::Listener;
        self.app.unlisten(self.id);
    }
}

fn lifecycle_admission_events(
    app: &tauri::App<tauri::Wry>,
) -> (
    Arc<std::sync::Mutex<Vec<Value>>>,
    LifecycleAdmissionListener,
) {
    use tauri::Listener;
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = events.clone();
    let id = app.listen("engine-config-changed", move |event| {
        observed
            .lock()
            .unwrap()
            .push(serde_json::from_str(event.payload()).unwrap());
    });
    (
        events,
        LifecycleAdmissionListener {
            app: app.handle().clone(),
            id,
        },
    )
}

async fn lifecycle_admission_claim_during_wait(action: &str, recording_actor: bool) {
    use std::future::Future;
    use std::task::Poll;

    let app = lifecycle_admission_app();
    let state = app.state::<AppState>();
    let backend = if recording_actor {
        let backend = native(&app, "healthy").await;
        state.set_inline_audio_transport(Some(true));
        Some(backend)
    } else {
        None
    };
    let (events, _listener) = lifecycle_admission_events(&app);
    // audio.rs owns lifecycle while awaiting paste_dispatch. This test owns
    // those real gates on behalf of that actor until its claim is published.
    let paste = state.recording.paste_dispatch.lock().await;
    let lifecycle = state.engine.funasr_lifecycle_op.lock().await;
    let (shutdown_owner, shutdown) = tokio::sync::watch::channel(false);
    let mut request = Box::pin(super::controller::dispatch(
        app.handle(),
        json!({"action":action, "engine":"confucius4-r2t2"}),
        shutdown,
    ));
    let initial = std::future::poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
    assert!(
        initial.is_pending(),
        "dispatch must wait at the actual lifecycle gate"
    );
    assert!(state.recording.recording.lock().is_none());
    assert!(!state.engine.is_funasr_starting());
    drop(paste);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let session_id = if recording_actor {
        assert!(state.is_funasr_ready());
        let mut slot = state.recording.recording.lock();
        let session_id = state
            .recording
            .session_counter
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        *slot = Some(RecordingSlot::Starting(PendingRecordingSession {
            session_id,
            subtitle_show_gen: 0,
            trigger: RecordingTrigger::DictationOriginal,
            stop_flag: stop_flag.clone(),
            stop_notify: Arc::new(tokio::sync::Notify::new()),
        }));
        let published = slot.as_ref().unwrap().snapshot(0);
        state
            .recording
            .transition_snapshot_while_recording_locked(
                session_id,
                published.phase,
                published.mode,
                None,
                None,
            )
            .expect("actual recording claim owns the snapshot");
        Some(session_id)
    } else {
        // start_server publishes this owner under lifecycle before find_engine.
        assert!(state.engine.funasr_process.lock().await.is_none());
        assert!(!state.is_funasr_ready() && state.inline_audio_transport().is_none());
        assert!(state.engine.try_begin_funasr_start(CONCURRENT_START_OWNER));
        assert!(state.engine.owns_funasr_start(CONCURRENT_START_OWNER));
        None
    };
    let before = lifecycle_admission_snapshot(&app).await;
    // Containment only: a wrongly admitted baseline stops at this real process
    // mutex. One resumed poll can expose the config write/generation advance,
    // but cannot enter child stop, discovery, downloads or startup. Drop the
    // controller before releasing this guard, including on the failing baseline.
    let process = state.engine.funasr_process.lock().await;
    drop(lifecycle);
    let outcome = std::future::poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
    drop(request);
    drop(process);
    let after = lifecycle_admission_snapshot(&app).await;
    assert!(state.engine.native_asr_owner.try_lock().is_ok());
    assert!(state.engine.funasr_lifecycle_op.try_lock().is_ok());
    assert!(!*shutdown_owner.borrow());
    assert!(
        matches!(&outcome, Poll::Ready(value) if value["success"] == false
            && value["surface"] == "desktop" && value["code"] == "busy"),
        "{action} must reject a claim published while awaiting lifecycle before any effect; \
         recording_actor={recording_actor}, outcome={outcome:?}, before={before:?}, after={after:?}"
    );
    assert_eq!(after, before, "busy admission preserves all owned effects");
    assert!(
        events.lock().unwrap().is_empty(),
        "busy admission emits no config event"
    );
    assert!(!stop_flag.load(Ordering::Acquire));
    if let Some(session_id) = session_id {
        assert_eq!(after.recording, Some(session_id));
        assert_eq!(
            after.recording_snapshot.as_ref().unwrap().phase,
            RecordingPhase::Starting
        );
        assert!(backend.as_ref().unwrap().alive());
        state.recording.recording.lock().take();
        assert!(state.recording.clear_snapshot_if_session(session_id));
    } else {
        assert!(state.engine.owns_funasr_start(CONCURRENT_START_OWNER));
        state.engine.finish_funasr_start(CONCURRENT_START_OWNER);
    }
    if let Some(backend) = backend {
        crate::services::funasr_service::stop_server(state.inner())
            .await
            .unwrap();
        backend.assert_reaped();
    }
    assert!(state.engine.funasr_process.lock().await.is_none());
    assert!(state.engine.funasr_starting_process.lock().is_none());
    assert!(!state.engine.is_funasr_starting());
    assert!(state.recording.recording.lock().is_none());
    assert!(app.config().app.windows.is_empty() && app.webview_windows().is_empty());
    assert_eq!(std::fs::read_dir(root().join("temp")).unwrap().count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_engine_set_rechecks_recording_claim() {
    if isolated("controller_lifecycle_admission_engine_set_rechecks_recording_claim") {
        lifecycle_admission_claim_during_wait("engine_set", true).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_engine_restart_rechecks_recording_claim() {
    if isolated("controller_lifecycle_admission_engine_restart_rechecks_recording_claim") {
        lifecycle_admission_claim_during_wait("engine_restart", true).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_engine_set_rechecks_start_owner() {
    if isolated("controller_lifecycle_admission_engine_set_rechecks_start_owner") {
        lifecycle_admission_claim_during_wait("engine_set", false).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_engine_restart_rechecks_start_owner() {
    if isolated("controller_lifecycle_admission_engine_restart_rechecks_start_owner") {
        lifecycle_admission_claim_during_wait("engine_restart", false).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_engine_start_rechecks_start_owner() {
    if isolated("controller_lifecycle_admission_engine_start_rechecks_start_owner") {
        lifecycle_admission_claim_during_wait("engine_start", false).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_shutdown_releases_waiting_request_owners() {
    if !isolated("controller_lifecycle_admission_shutdown_releases_waiting_request_owners") {
        return;
    }
    use std::future::Future;
    use std::task::Poll;

    let app = lifecycle_admission_app();
    let state = app.state::<AppState>();
    let (events, _listener) = lifecycle_admission_events(&app);
    for action in ["engine_set", "engine_restart", "engine_start"] {
        let before = lifecycle_admission_snapshot(&app).await;
        let lifecycle = state.engine.funasr_lifecycle_op.lock().await;
        let (shutdown_owner, shutdown) = tokio::sync::watch::channel(false);
        let mut request = Box::pin(super::controller::dispatch(
            app.handle(),
            json!({"action":action, "engine":"confucius4-r2t2"}),
            shutdown,
        ));
        let initial = std::future::poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
        assert!(initial.is_pending());
        shutdown_owner.send(true).unwrap();
        let outcome = std::future::poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
        drop(request);
        assert!(
            matches!(&outcome, Poll::Ready(value) if value["success"] == false
            && value["code"] == "owner_shutdown"),
            "{action}: {outcome:?}"
        );
        assert!(state.engine.native_asr_owner.try_lock().is_ok());
        assert!(state.engine.funasr_lifecycle_op.try_lock().is_err());
        assert_eq!(lifecycle_admission_snapshot(&app).await, before);
        assert!(events.lock().unwrap().is_empty());
        drop(lifecycle);
        assert!(state.engine.funasr_lifecycle_op.try_lock().is_ok());
    }
}

async fn lifecycle_admission_saved_configuration_during_wait(action: &str) {
    use std::future::Future;
    use std::task::Poll;

    let app = lifecycle_admission_app();
    let state = app.state::<AppState>();
    let backend = native(&app, "healthy").await;
    state.set_inline_audio_transport(Some(true));
    let (events, _listener) = lifecycle_admission_events(&app);
    let lifecycle = state.engine.funasr_lifecycle_op.lock().await;
    let (shutdown_owner, shutdown) = tokio::sync::watch::channel(false);
    let mut request = Box::pin(super::controller::dispatch(
        app.handle(),
        json!({"action":action}),
        shutdown,
    ));
    let initial = std::future::poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
    assert!(
        initial.is_pending(),
        "local runtime request awaits lifecycle"
    );

    // A request pending on lifecycle at the Stage A checkpoint owns no ASR
    // lease. Follow the actual GUI writer's ownership protocol, without calling
    // the full GUI command, keyring, a provider or an API.
    let actor = crate::commands::funasr::lock_idle_asr(state.inner())
        .expect("waiting Stage A request leaves ASR available to the lifecycle owner");
    crate::utils::paths::write_engine_config("glm-asr").unwrap();
    state.engine.block_funasr_starting();
    crate::services::funasr_service::stop_server(state.inner())
        .await
        .unwrap();
    backend.assert_reaped();
    state.engine.unblock_funasr_starting();
    // No configured cloud key/provider is consulted. Publish the stopped,
    // not-ready cloud state, then release ASR before the lifecycle guard.
    state.set_funasr_ready(false);
    state.set_inline_audio_transport(None);
    drop(actor);
    let before = lifecycle_admission_snapshot(&app).await;
    // A legacy restart advances generation before waiting on this mutex. Keep
    // the wrongly admitted request contained before child stop/start discovery.
    let process = state.engine.funasr_process.lock().await;
    drop(lifecycle);
    let outcome = std::future::poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
    drop(request);
    drop(process);
    let after = lifecycle_admission_snapshot(&app).await;
    assert!(state.engine.native_asr_owner.try_lock().is_ok());
    assert!(state.engine.funasr_lifecycle_op.try_lock().is_ok());
    assert!(!*shutdown_owner.borrow());
    assert!(
        matches!(&outcome, Poll::Ready(value) if value["success"] == false
            && value["surface"] == "desktop" && value["code"] == "invalid_request"),
        "{action} must validate the saved cloud engine after lifecycle admission; \
         outcome={outcome:?}, before={before:?}, after={after:?}"
    );
    assert_eq!(
        after, before,
        "invalid saved engine preserves actor-owned state"
    );
    assert!(events.lock().unwrap().is_empty());
    assert!(
        !state.is_funasr_ready(),
        "rejection preserves cloud readiness"
    );
    assert!(state.engine.funasr_process.lock().await.is_none());
    assert!(state.engine.funasr_starting_process.lock().is_none());
    assert!(!state.engine.is_funasr_starting());
    assert_eq!(
        audit(),
        before.audit,
        "no CLI IPC after the actor's settled stop"
    );
    assert!(app.config().app.windows.is_empty() && app.webview_windows().is_empty());
    assert_eq!(std::fs::read_dir(root().join("temp")).unwrap().count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_engine_start_rechecks_saved_configuration() {
    if isolated("controller_lifecycle_admission_engine_start_rechecks_saved_configuration") {
        lifecycle_admission_saved_configuration_during_wait("engine_start").await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn controller_lifecycle_admission_engine_restart_rechecks_saved_configuration() {
    if isolated("controller_lifecycle_admission_engine_restart_rechecks_saved_configuration") {
        lifecycle_admission_saved_configuration_during_wait("engine_restart").await;
    }
}
