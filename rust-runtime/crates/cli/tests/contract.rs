use serde_json::{Value, json};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = env::temp_dir().join(format!(
                "lw-native-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!("{err}"),
            }
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("clean isolated test files");
    }
}

fn peer(path: &Path) {
    static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
    install_peer(path, "peer.rs", &IMAGE);
}

fn install_peer(path: &Path, name: &str, cache: &OnceLock<Vec<u8>>) {
    let image = cache.get_or_init(|| {
        let scratch = Scratch::new();
        let binary = scratch.0.join(format!("peer{}", env::consts::EXE_SUFFIX));
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let output = Command::new("rustc")
            .arg(source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::read(binary).unwrap()
    });
    fs::write(path, image).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

struct Case {
    root: Scratch,
    cli: PathBuf,
    peer: PathBuf,
    config: PathBuf,
    log: PathBuf,
}
impl Case {
    fn new() -> Self {
        let root = Scratch::new();
        let config = root.0.join("settings");
        let bin = root.0.join("bin");
        fs::create_dir(&config).unwrap();
        fs::create_dir(&bin).unwrap();
        // Use the shipped alias layout so a release or installed desktop cannot be selected.
        let cli = bin.join(format!("light-whisper{}", env::consts::EXE_SUFFIX));
        fs::copy(env!("CARGO_BIN_EXE_light-whisper-cli"), &cli).unwrap();
        let peer_path = root.0.join("light-whisper.exe");
        peer(&peer_path);
        let log = root.0.join("peer.log");
        Self {
            root,
            cli,
            peer: peer_path,
            config,
            log,
        }
    }
    fn run(&self, args: &[&str], input: &str, mode: &str, expected: i32) -> Vec<Value> {
        self.run_raw(args, input, mode, expected)
            .lines()
            .map(|line| serde_json::from_str(line).expect("stdout must be JSONL"))
            .collect()
    }
    fn run_raw(&self, args: &[&str], input: &str, mode: &str, expected: i32) -> String {
        let (code, stdout, stderr) = self.output(args, input, mode);
        assert_eq!(
            code,
            Some(expected),
            "{args:?}, stdout={stdout}, stderr={stderr}"
        );
        stdout
    }
    fn output(&self, args: &[&str], input: &str, mode: &str) -> (Option<i32>, String, String) {
        let mut child = Command::new(&self.cli)
            .args(args)
            .current_dir(&self.root.0)
            .env("LIGHT_WHISPER_DATA_DIR", &self.config)
            .env("LOCALAPPDATA", self.root.0.join("local"))
            .env("APPDATA", self.root.0.join("roaming"))
            .env("HF_HOME", self.root.0.join("hf"))
            .env("HF_HUB_OFFLINE", "from-parent")
            .env("LW_TEST_RECORD", &self.log)
            .env("LW_TEST_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let bytes = input.as_bytes().to_vec();
        let writer = thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
        let read = |mut pipe: Box<dyn Read + Send>| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                pipe.read_to_end(&mut bytes).unwrap();
                bytes
            })
        };
        let out = read(Box::new(child.stdout.take().unwrap()));
        let err = read(Box::new(child.stderr.take().unwrap()));
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(6) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("CLI exceeded outer watchdog for {args:?}");
            }
            thread::sleep(Duration::from_millis(5));
        };
        writer.join().unwrap();
        let stdout = String::from_utf8(out.join().unwrap()).unwrap();
        let stderr = String::from_utf8_lossy(&err.join().unwrap()).into_owned();
        (status.code(), stdout, stderr)
    }
    fn record(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
    fn wire(&self) -> Vec<Value> {
        self.record()
            .lines()
            .filter_map(|line| line.strip_prefix("WIRE "))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

#[test]
fn doctor_discovers_parent_desktop_and_native_engine_without_launch() {
    let case = Case::new();
    let report = case.run(&["doctor"], "", "", 0);
    assert_eq!(report.len(), 1, "doctor must emit one report");
    let report = &report[0];
    assert_eq!(report["success"], true);
    assert_eq!(report["target"], "app");
    assert_eq!(report["engine_source"], "default");
    assert_eq!(report["engine"], "qwen3-asr-0.6b");
    assert_eq!(report["runtime_present"], true);
    assert_eq!(report["app_protocol_supported"], true);
    assert_eq!(report["model_load_checked"], false);
    assert_eq!(
        fs::canonicalize(report["runtime"].as_str().unwrap()).unwrap(),
        fs::canonicalize(&case.peer).unwrap()
    );
    let engine = case.run(&["--target", "engine", "doctor"], "", "", 0);
    assert_eq!(engine[0]["target"], "engine");
    assert_eq!(engine[0]["runtime_present"], false);
    assert!(
        !engine[0]["runtime"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("python")
    );
    assert!(case.record().is_empty());
}

#[test]
fn routes_each_one_shot_desktop_command_to_broker() {
    for (args, action, fields) in [
        (vec!["status"], "status", json!({})),
        (vec!["stats"], "stats", json!({})),
        (vec!["models"], "models", json!({})),
        (vec!["devices"], "devices", json!({})),
        (vec!["engine", "get"], "engine_get", json!({})),
        (vec!["engine", "start"], "engine_start", json!({})),
        (vec!["engine", "restart"], "engine_restart", json!({})),
        (
            vec!["engine", "set", "confucius4-r2t2"],
            "engine_set",
            json!({"engine":"confucius4-r2t2"}),
        ),
    ] {
        let case = Case::new();
        let replies = case.run(&args, "", "", 0);
        let mut request = fields;
        request["action"] = json!(action);
        request["request_id"] = json!(1);
        assert_eq!(
            replies.len(),
            1,
            "one-shot command {action} must emit a reply"
        );
        assert_eq!(replies[0]["request"], request);
        assert_eq!(
            case.wire(),
            [request, json!({"action":"exit","request_id":2})]
        );
        let record = case.record();
        let args: Vec<_> = record
            .lines()
            .filter_map(|s| s.strip_prefix("ARG "))
            .collect();
        assert_eq!(args, ["--agent-client"]);
        assert!(
            record
                .lines()
                .any(|s| s == "ENV HF_HUB_OFFLINE=from-parent")
        );
    }
}

fn failure(reply: &Value) {
    assert_eq!(reply["success"], false, "{reply}");
    assert!(
        reply["error"].as_str().is_some_and(|s| !s.is_empty()),
        "{reply}"
    );
}

#[test]
fn independent_engine_honors_saved_selection_offline_threads_and_directory_ownership() {
    for explicit in [true, false] {
        let case = Case::new();
        let model_dir = case.root.0.join("模型 cache");
        let saved = json!({"engine":"qwen3-asr-1.7b","models_dir":model_dir});
        fs::write(case.config.join("engine.json"), saved.to_string()).unwrap();
        let kept = case.root.0.join("retained data");
        let mut args = vec!["--engine-exe", case.peer.to_str().unwrap()];
        if explicit {
            args.extend([
                "--engine",
                "qwen3-asr-0.6b",
                "--threads",
                "3",
                "--data-dir",
                kept.to_str().unwrap(),
            ]);
        }
        let doctor = case.run(&[args.as_slice(), &["doctor"]].concat(), "", "", 0);
        assert_eq!(doctor.len(), 1);
        assert_eq!(doctor[0]["target"], "engine");
        assert_eq!(doctor[0]["models_dir_source"], "desktop_config");
        assert_eq!(
            doctor[0]["engine_source"],
            if explicit {
                "argument"
            } else {
                "desktop_config"
            }
        );
        assert!(case.record().is_empty());
        args.push("stats");
        let reply = case.run(&args, "", "", 0);
        assert_eq!(reply.len(), 1);
        assert_eq!(
            reply[0]["request"],
            json!({"action":"stats","request_id":1})
        );
        let log = case.record();
        let arguments: Vec<_> = log.lines().filter_map(|s| s.strip_prefix("ARG ")).collect();
        assert_eq!(
            arguments,
            [
                "serve",
                "--engine",
                if explicit {
                    "qwen3-asr-0.6b"
                } else {
                    "confucius4-r2t2"
                }
            ]
        );
        let setting = |key| {
            log.lines()
                .find_map(|s| s.strip_prefix(&format!("ENV {key}=")))
                .unwrap()
        };
        for name in ["HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE"] {
            assert_eq!(setting(name), "1");
        }
        for name in [
            "LIGHT_WHISPER_NATIVE_THREADS",
            "OMP_NUM_THREADS",
            "MKL_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
        ] {
            assert_eq!(setting(name), if explicit { "3" } else { "2" });
        }
        assert_eq!(
            Path::new(setting("HF_HUB_CACHE")).file_name(),
            model_dir.file_name()
        );
        let data = Path::new(setting("LIGHT_WHISPER_DATA_DIR"));
        if explicit {
            assert_eq!(
                fs::canonicalize(data).unwrap(),
                fs::canonicalize(&kept).unwrap()
            );
        } else {
            assert_ne!(data, case.config);
            assert!(
                !data.exists(),
                "ephemeral engine directory survived its owner"
            );
        }
    }
}

#[test]
fn invalid_arguments_config_and_target_conflicts_fail_before_launch() {
    let case = Case::new();
    let peer = case.peer.to_str().unwrap();
    for args in [
        vec!["--target", "app", "--engine-exe", peer, "status"],
        vec!["--app-exe", peer, "--target", "engine", "status"],
        vec!["--engine-exe", peer, "models"],
        vec!["--engine", "glm-asr", "status"],
        vec!["--threads", "0", "status"],
        vec!["--threads", "2147483648", "status"],
        vec!["--timeout", "NaN", "status"],
        vec!["--timeout", "0", "status"],
        vec!["--app-exe", "missing.exe", "status"],
        vec!["engine", "set", "glm-asr"],
        vec!["transcribe", "missing.wav"],
    ] {
        let reply = case.run(&args, "", "", 2);
        assert_eq!(reply.len(), 1, "expected JSON failure for {args:?}");
        failure(&reply[0]);
    }
    for config in ["[]", "{", r#"{"engine":"unsupported"}"#] {
        fs::write(case.config.join("engine.json"), config).unwrap();
        let reply = case.run(&["doctor"], "", "", 2);
        failure(&reply[0]);
    }
    fs::write(case.config.join("engine.json"), r#"{"engine":"glm-asr"}"#).unwrap();
    let report = case.run(&["--target", "engine", "doctor"], "", "", 0);
    assert_eq!(report[0]["local_engine_supported"], false);
    let reply = case.run(&["--engine-exe", peer, "status"], "", "", 2);
    failure(&reply[0]);
    assert!(case.record().is_empty());
}

#[test]
fn transcribe_preserves_unicode_options_and_repeated_words() {
    let case = Case::new();
    let audio = case.root.0.join("合成 speech 🎧.wav");
    fs::write(&audio, b"test peer never decodes audio").unwrap();
    for words in [false, true] {
        let mut args = vec![
            "transcribe",
            audio.to_str().unwrap(),
            "--language",
            "zh",
            "--context",
            "词条 🎧",
        ];
        if words {
            args.extend(["--hot-word", "术语", "--hot-word", "语音"]);
        }
        let reply = case.run(&args, "", "", 0);
        assert_eq!(reply.len(), 1);
        let req = &reply[0]["request"];
        assert_eq!(req["action"], "transcribe");
        assert_eq!(
            fs::canonicalize(req["audio_path"].as_str().unwrap()).unwrap(),
            fs::canonicalize(&audio).unwrap()
        );
        assert_eq!(req["options"], json!({"language":"zh","context":"词条 🎧"}));
        assert_eq!(
            req.get("hot_words").cloned(),
            if words {
                Some(json!(["术语", "语音"]))
            } else {
                None
            }
        );
    }
}

#[test]
fn session_recovers_bad_records_and_reuses_one_correlated_child() {
    let case = Case::new();
    let audio = case.root.0.join("音频.wav");
    fs::write(&audio, b"fixture").unwrap();
    let valid = json!({"action":"transcribe","request_id":-9,"audio":audio,"context":"文字 🎶","hot_words":["A","词"]});
    let input = format!(
        "\nnot-json\n{}\n{}\n{}\n{}\n{}\n{}\n",
        json!({"action":"status","request_id":true}),
        json!({"action":"engine_set","request_id":777,"engine":"glm-asr"}),
        json!({"action":"status","request_id":-9}),
        valid,
        json!({"action":"exit","request_id":71}),
        json!({"action":"stats","request_id":999})
    );
    let replies = case.run(&["session"], &input, "refuse", 0);
    assert_eq!(replies.len(), 6);
    for reply in &replies[..4] {
        failure(reply);
    }
    let ids: Vec<_> = replies.iter().map(|r| r["request_id"].clone()).collect();
    assert_eq!(
        ids,
        [
            Value::Null,
            Value::Null,
            json!(777),
            json!(-9),
            json!(-9),
            json!(71)
        ]
    );
    assert_eq!(replies[4]["success"], true);
    assert_eq!(replies[4]["request"]["options"]["context"], "文字 🎶");
    assert_eq!(replies[4]["request"]["hot_words"], json!(["A", "词"]));
    assert_eq!(replies[5], json!({"success":true,"request_id":71}));
    let wire = case.wire();
    assert_eq!(wire.len(), 3);
    for (i, action) in ["status", "transcribe", "exit"].iter().enumerate() {
        assert_eq!(wire[i]["action"], *action);
        assert_eq!(wire[i]["request_id"], i + 1);
    }
    assert_eq!(
        case.record()
            .lines()
            .filter(|s| s.starts_with("START "))
            .count(),
        1
    );
}

#[test]
fn invalid_or_empty_sessions_never_acquire_an_engine() {
    let case = Case::new();
    let args = ["--engine-exe", case.peer.to_str().unwrap(), "session"];
    let replies = case.run(&args, "\n \n", "", 0);
    assert!(replies.is_empty());
    let replies = case.run(
        &args,
        "{\"request_id\":42,\"action\":\"models\"}\n{\"request_id\":3,\"action\":\"exit\"}\n",
        "",
        0,
    );
    assert_eq!(replies.len(), 2);
    failure(&replies[0]);
    assert_eq!(replies[0]["request_id"], 42);
    assert_eq!(replies[1], json!({"success":true,"request_id":3}));
    assert!(case.record().is_empty());
}

#[test]
fn terminal_responses_and_deadlines_end_session_without_retry_or_tail_dispatch() {
    for mode in ["startup-fail", "fatal", "eof", "nonfinite", "timeout"] {
        let case = Case::new();
        let input =
            "{\"request_id\":-4,\"action\":\"status\"}\n{\"request_id\":15,\"action\":\"stats\"}\n";
        let replies = case.run(&["--timeout", "0.5", "session"], input, mode, 1);
        assert_eq!(replies.len(), 1, "terminal reply count for {mode}");
        assert_eq!(replies[0]["request_id"], -4);
        failure(&replies[0]);
        assert!(
            !case.wire().iter().any(|r| r["action"] == "stats"),
            "{mode} dispatched queued tail"
        );
        assert_eq!(
            case.record()
                .lines()
                .filter(|s| s.starts_with("START "))
                .count(),
            1
        );
    }
}

#[test]
fn desktop_rejects_invalid_unicode_and_oversized_records_without_consuming_wire_ids() {
    let case = Case::new();
    let audio = case.root.0.join("audio.wav");
    fs::write(&audio, b"fixture").unwrap();
    let bad_unicode = format!(
        r#"{{"request_id":32,"action":"transcribe","audio":{},"context":"\ud800"}}"#,
        json!(audio)
    );
    let too_big =
        json!({"request_id":33,"action":"transcribe","audio":audio,"context":"a".repeat(65536)});
    let input = format!(
        "{}\n{}\n{}\n{}\n",
        json!({"request_id":11,"action":"status"}),
        bad_unicode,
        too_big,
        json!({"request_id":34,"action":"stats"})
    );
    let replies = case.run(&["session"], &input, "", 0);
    assert_eq!(replies.len(), 4);
    for (i, id) in [11, 32, 33, 34].iter().enumerate() {
        assert_eq!(
            replies[i]["request_id"], *id,
            "record {i} lost its caller ID"
        );
        assert_eq!(replies[i]["success"], i == 0 || i == 3);
    }
    assert_eq!(
        case.wire(),
        [
            json!({"request_id":1,"action":"status"}),
            json!({"request_id":2,"action":"stats"}),
            json!({"request_id":3,"action":"exit"})
        ]
    );
}

#[test]
fn session_round_trips_opaque_integer_ids_outside_machine_range() {
    let case = Case::new();
    let huge = "-1208925819614629174706177"; // -(2^80 + 1)
    let input = format!(r#"{{"request_id":{huge},"action":"status"}}"#);
    let reply = case.run_raw(&["session"], &input, "", 0);
    assert!(
        reply.contains(huge),
        "caller ID rounded, rejected, or changed: {reply}"
    );
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["success"], true);
    assert_eq!(reply["request"]["request_id"], 1);
}

#[test]
fn explicit_missing_and_unmarked_desktop_are_never_replaced_with_valid_default() {
    let case = Case::new();
    let path = case.root.0.join("other.exe");
    for _ in 0..2 {
        let path_str = path.to_str().unwrap();
        let doctor = case.run(&["--app-exe", path_str, "doctor"], "", "", 0);
        assert_eq!(doctor.len(), 1);
        assert_eq!(doctor[0]["runtime_present"], path.exists());
        assert_eq!(doctor[0]["app_protocol_supported"], false);
        let errors = case.run(&["--app-exe", path_str, "status"], "", "", 2);
        failure(&errors[0]);
        fs::write(&path, b"test fixture lacking a protocol marker").unwrap();
    }
    assert!(case.record().is_empty());
}

#[test]
fn protocol_boundary_accepts_exact_physical_desktop_limit() {
    let case = Case::new();
    let audio = case.root.0.join("edge.wav");
    fs::write(&audio, b"fixture").unwrap();
    let record = |context: String| {
        json!({"action":"transcribe", "request_id":21, "audio":audio, "context":context})
            .to_string()
    };
    let calibration = case.run(&["session"], &record(String::new()), "", 0);
    assert_eq!(calibration[0]["success"], true);
    let log = case.record();
    let wire = log.lines().find_map(|s| s.strip_prefix("WIRE ")).unwrap();
    let terminator = if cfg!(windows) { 2 } else { 1 };
    // Same path, request sequence 1 and escaping on both invocations. Each
    // added ASCII letter contributes one byte to the physical broker frame.
    let available = 65536 - wire.len() - terminator;
    for extra in [0, 1] {
        let replies = case.run(&["session"], &record("a".repeat(available + extra)), "", 0);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["request_id"], 21);
        assert_eq!(replies[0]["success"], extra == 0, "65536+{extra} bytes");
    }
    assert_eq!(
        case.wire()
            .iter()
            .filter(|r| r["action"] == "transcribe")
            .count(),
        2
    );
}

#[test]
fn protocol_boundary_nonfinite_engine_literals_after_escaped_text_are_terminal() {
    let case = Case::new();
    let executable = case
        .root
        .0
        .join(format!("lexical-reply{}", env::consts::EXE_SUFFIX));
    static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
    install_peer(&executable, "lexical_reply.rs", &IMAGE);
    let args = [
        "--engine-exe",
        executable.to_str().unwrap(),
        "--timeout",
        "1",
        "session",
    ];
    let input =
        "{\"action\":\"status\",\"request_id\":42}\n{\"action\":\"stats\",\"request_id\":43}\n";
    for (token, quoted) in [
        ("NaN", r#"\""#),
        ("Infinity", r#"a\" b\\c NaN Infinity -Infinity"#),
        ("-Infinity", r#"a\" b\\c NaN Infinity -Infinity"#),
    ] {
        let frame =
            format!(r#"{{"request_id":1,"success":true,"quoted":"{quoted}","payload":[{token}]}}"#);
        let replies = case.run(&args, input, &frame, 1);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["request_id"], 42);
        failure(&replies[0]);
        assert!(
            replies[0]["error"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("nonfinite"),
            "{token}: {}",
            replies[0]
        );
    }
    // A repeated member keeps its last value (the wire decoder's existing
    // behavior). No nonfinite number survives here; literal text stays exact.
    let frame = r#"{"request_id":1,"success":true,"overridden":NaN,"overridden":7,"text":"literal NaN after \"quote\""}"#;
    let reply = case.run(
        &args,
        "{\"action\":\"status\",\"request_id\":42}\n",
        frame,
        0,
    );
    assert_eq!(reply.len(), 1);
    assert_eq!(reply[0]["overridden"], 7);
    assert_eq!(reply[0]["text"], "literal NaN after \"quote\"");
}

#[cfg(windows)]
struct EngineInstall {
    case: Case,
    archive: PathBuf,
    signed: PathBuf,
    digest: String,
}
#[cfg(windows)]
impl EngineInstall {
    fn new(archive_mode: &str) -> Self {
        let mut case = Case::new();
        let installed = case.root.0.join("light-whisper-cli.exe");
        fs::copy(&case.cli, &installed).unwrap();
        case.cli = installed;
        static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
        let signed = case.root.0.join("test-sidecar.exe");
        install_peer(&signed, "lexical_reply.rs", &IMAGE);
        let archive = case.root.0.join("resources/engine.tar.xz");
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build_test_archive.py");
        let output = Command::new("python")
            .arg(fixture)
            .arg(&signed)
            .arg(&archive)
            .arg(archive_mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let digest = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        Self {
            case,
            archive,
            signed,
            digest,
        }
    }
    fn cached(&self) -> PathBuf {
        self.case.config.join("cli-engines").join(&self.digest)
    }
    fn host(&self) -> PathBuf {
        self.case.config.join("engine")
    }
    fn set_host(&self, binary: &Path, version: &str) {
        fs::create_dir_all(self.host()).unwrap();
        fs::copy(binary, self.host().join("engine.exe")).unwrap();
        fs::write(self.host().join(".version"), version).unwrap();
    }
    fn status(&self, expected: i32) -> Vec<Value> {
        self.case.run(
            &["--target", "engine", "status"],
            "",
            r#"{"request_id":1,"success":true,"source":"archive-peer"}"#,
            expected,
        )
    }
    fn status_failure(&self) -> Vec<Value> {
        let args = ["--target", "engine", "status"];
        let (code, stdout, stderr) = self.case.output(&args, "", "");
        assert!(
            matches!(code, Some(1 | 2)),
            "invalid automatic runtime succeeded: code={code:?}, stdout={stdout}, stderr={stderr}"
        );
        let replies: Vec<_> = stdout
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(replies.len(), 1);
        replies
    }
    fn doctor_only(&self) {
        let replies = self.case.run(&["--target", "engine", "doctor"], "", "", 0);
        assert_eq!(replies[0]["success"], true);
        assert_eq!(replies[0]["model_load_checked"], false);
        assert!(!self.cached().exists(), "doctor extracted a runtime");
        assert!(
            self.case.record().is_empty(),
            "doctor started the old runtime"
        );
    }
}

#[cfg(windows)]
#[test]
fn engine_bootstrap_runs_bundled_native_cli_on_clean_install_and_legacy_upgrade() {
    for legacy in [true, false] {
        let install = EngineInstall::new("valid");
        if legacy {
            install.set_host(&install.case.peer, "1.8.1");
        }
        install.doctor_only();
        let expected_bytes = fs::read(&install.signed).unwrap();
        for _ in 0..2 {
            let replies = install.status(0);
            assert_eq!(replies.len(), 1);
            assert_eq!(
                replies[0]["source"], "archive-peer",
                "unverified engine executed"
            );
            assert!(
                install.case.record().is_empty(),
                "legacy host engine was launched"
            );
            assert_eq!(
                fs::read(install.cached().join("engine.exe")).unwrap(),
                expected_bytes
            );
            assert_eq!(
                fs::read(install.cached().join("_internal/test-asset.txt")).unwrap(),
                b"bundled test asset\n"
            );
        }
        if legacy {
            assert_eq!(
                fs::read_to_string(install.host().join(".version")).unwrap(),
                "1.8.1"
            );
            assert_eq!(
                fs::read(install.host().join("engine.exe")).unwrap(),
                fs::read(&install.case.peer).unwrap()
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn engine_bootstrap_refuses_corrupt_package_without_launching_or_modifying_legacy_cache() {
    for mode in ["truncated", "bad-image"] {
        let install = EngineInstall::new(mode);
        install.set_host(&install.case.peer, "1.8.1");
        let before = fs::read(install.host().join("engine.exe")).unwrap();
        let result = install.status_failure();
        failure(&result[0]);
        assert!(install.case.record().is_empty());
        assert_eq!(fs::read(install.host().join("engine.exe")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(install.host().join(".version")).unwrap(),
            "1.8.1"
        );
        assert!(
            !install.cached().join("engine.exe").exists(),
            "corrupt package published as reusable"
        );
    }
}

#[cfg(windows)]
#[test]
fn engine_bootstrap_without_archive_requires_verified_native_host_cache() {
    let install = EngineInstall::new("valid");
    fs::remove_file(&install.archive).unwrap();
    let marker = format!("sha256:{}", install.digest);
    for (binary, version) in [
        (&install.case.peer, marker.as_str()),
        (&install.signed, "1.8.1"),
    ] {
        install.set_host(binary, version);
        let result = install.status_failure();
        failure(&result[0]);
        assert!(install.case.record().is_empty());
    }
    install.set_host(&install.signed, &marker);
    let replies = install.status(0);
    assert_eq!(replies[0]["source"], "archive-peer");
    assert!(!install.cached().exists());
}

#[cfg(windows)]
#[test]
fn engine_bootstrap_rejects_a_link_to_another_bundled_asset() {
    let install = EngineInstall::new("linked-asset");
    install.set_host(&install.case.peer, "1.8.1");
    let replies = install.status_failure();
    failure(&replies[0]);
    assert!(
        install.case.record().is_empty(),
        "rejected package launched the old host"
    );
    assert!(
        !install.cached().join("engine.exe").exists(),
        "rejected package published a cache"
    );
    assert_eq!(
        fs::read(install.host().join("engine.exe")).unwrap(),
        fs::read(&install.case.peer).unwrap()
    );
    assert_eq!(
        fs::read_to_string(install.host().join(".version")).unwrap(),
        "1.8.1"
    );
}
