#![cfg(windows)]
use light_whisper_native_asr::{Backend, Family, NativeAsr};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_abi_contract() {
    let parent = env::temp_dir();
    let root =
        Scratch(parent.join(
            format!("lw-asr-fixture-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
        ));
    fs::create_dir(&root.0).unwrap();
    let dll = root.0.join("audiocpp.dll");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audiocpp.rs");
    let build = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg(fixture)
        .args(["--edition=2021", "--crate-type=cdylib", "-o"])
        .arg(&dll)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let model = root.0.join("语音.gguf");
    fs::write(&model, b"test fixture").unwrap();
    let cases = [
        ("cpu", 0x0600),
        ("cuda", 0x0300),
        ("cuda", 0x0400),
        ("cuda", 0x0500),
        ("cuda", 0x0600),
        ("unsupported", 0x0200),
        ("unsupported", 0x10300),
        ("threads", 0x0600),
        ("one_thread", 0x0600),
        ("max_threads", 0x0600),
        ("qwen", 0x0600),
        ("conflict", 0x0600),
        ("fail_registry_create", 0x0600),
        ("fail_model_load", 0x0600),
        ("null_options", 0x0600),
        ("fail_options_set", 0x0600),
        ("fail_session_create", 0x0600),
        ("null_request", 0x0600),
        ("fail_request_set_text", 0x0600),
        ("fail_stream_start", 0x0600),
        ("fail_stream_push", 0x0600),
        ("fail_result_preview_text", 0x0600),
        ("fail_stream_finish", 0x0600),
        ("null_event", 0x0600),
        ("no_result_text", 0x0600),
        ("fail_request_set_option", 0x0600),
        ("fail_request_set_audio", 0x0600),
        ("fail_session_run", 0x0600),
        ("fail_result_text", 0x0600),
    ];
    let mut failures = Vec::new();
    for (name, abi) in cases {
        let child = Command::new(env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "exercise_one_native_case",
                "--nocapture",
            ])
            .env("LW_FIXTURE_CASE", name)
            .env("LW_FIXTURE_ABI", abi.to_string())
            .env("LW_FIXTURE_DLL", &dll)
            .env("LW_FIXTURE_MODEL", &model)
            .env("LW_FIXTURE_TRACE", root.0.join(format!("{name}-{abi}.log")))
            .output()
            .unwrap();
        if !child.status.success() {
            failures.push(format!(
                "{name}, ABI {abi:#x}: {}",
                String::from_utf8_lossy(&child.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn trace() -> Vec<String> {
    let path = env::var_os("LW_FIXTURE_TRACE").unwrap();
    match fs::read_to_string(path) {
        Ok(contents) => contents.lines().map(str::to_string).collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!("read fixture trace: {err}"),
    }
}

fn check_ownership(log: &[String]) {
    let handles = |prefix: &str| {
        let mut ids: Vec<_> = log
            .iter()
            .filter_map(|line| line.strip_prefix(prefix))
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(
        handles("alloc "),
        handles("free "),
        "each owned handle freed with its own destructor once; borrowed events' results never freed"
    );
    let lifetime: Vec<_> = log
        .iter()
        .filter_map(|line| line.strip_prefix("free "))
        .filter_map(|id| id.split_once(' ').map(|(kind, _)| kind))
        .filter(|kind| ["registry", "model", "session"].contains(kind))
        .collect();
    let order = ["session", "model", "registry"];
    let expected: Vec<_> = order
        .into_iter()
        .filter(|kind| lifetime.contains(kind))
        .collect();
    assert_eq!(lifetime, expected, "release owned parents last");
}

fn has(log: &[String], line: &str) -> bool {
    log.iter().any(|item| item == line)
}

fn run_r2t2(native: &mut NativeAsr, cuda: bool, abi: u32) {
    assert!(native.feed(&[1.0]).is_err());
    assert!(native.finish().is_err());
    native.start("术语", Some("zh")).unwrap();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(native.feed(&[1.0, bad]).is_err());
    }
    let chunk = if cuda { 2560 } else { 5120 };
    let batched = cuda && abi >= 0x0400;
    let first = native.feed(&vec![0.5; chunk * 2]).unwrap();
    let (text, preview, lang) = if batched {
        ("A", "A?", "zh")
    } else {
        ("AB", "AB?", "Chinese")
    };
    assert_eq!(
        (
            first.text.as_str(),
            first.preview.as_str(),
            first.language.as_deref()
        ),
        (text, preview, Some(lang))
    );
    let tail = native.feed(&[0.25; 13]).unwrap();
    assert_eq!(
        (
            tail.text.as_str(),
            tail.preview.as_str(),
            tail.language.as_deref()
        ),
        ("AB", "AB?", Some("Chinese"))
    );
    let finished = native.finish().unwrap();
    assert_eq!(
        (finished.text.as_str(), finished.language.as_deref()),
        ("原文", Some("Chinese"))
    );
    assert!(native.finish().is_err());

    let log = trace();
    let mut expected = Vec::new();
    if batched {
        expected.push(format!("push 0 {} 16000 1 false", 2 * chunk));
    } else {
        expected.push(format!("push 0 {chunk} 16000 1 false"));
        expected.push(format!("push {chunk} {chunk} 16000 1 false"));
    }
    expected.push(format!("push {} 13 16000 1 false", 2 * chunk));
    let coalesced = cuda && abi >= 0x0500;
    if !coalesced {
        for padded in (0..5120).step_by(chunk) {
            expected.push(format!(
                "push {} {chunk} 16000 1 true",
                2 * chunk + 13 + padded
            ));
        }
    }
    let pushes: Vec<_> = log
        .iter()
        .filter(|line| line.starts_with("push "))
        .cloned()
        .collect();
    assert_eq!(pushes, expected);
    for (name, present) in [
        ("coalesced_finish", coalesced),
        ("adaptive_cadence", cuda && abi >= 0x0600),
        ("rolling_window", true),
        ("rollback_punctuation", true),
    ] {
        assert_eq!(
            has(&log, &format!("option confucius4_r2t2.{name} true")),
            present
        );
    }
    assert!(has(
        &log,
        &format!("option confucius4_r2t2.chunk_size_ms {}", chunk / 16)
    ));
    assert!(
        log.iter()
            .any(|line| line.starts_with("model confucius4_r2t2 ") && line.ends_with("语音.gguf"))
    );
    assert!(has(
        &log,
        &format!(
            "session asr streaming {} 0 3",
            if cuda { "cuda" } else { "cpu" }
        )
    ));
    assert!(has(&log, "text 术语 zh"));

    native.start("", None).unwrap();
    native.feed(&[0.5]).unwrap();
    native.reset().unwrap();
    native.reset().unwrap();
    assert!(native.feed(&[0.5]).is_err());
    native.start("", None).unwrap();
    let fresh = native.feed(&[0.5]).unwrap();
    assert_eq!((fresh.text.as_str(), fresh.language), ("A", None));
    assert_eq!(
        trace()
            .iter()
            .filter(|line| line.as_str() == "reset")
            .count(),
        1
    );
    native.reset().unwrap();
    native.start("", None).unwrap();
    let pushes = trace()
        .iter()
        .filter(|line| line.starts_with("push "))
        .count();
    native.finish().unwrap();
    assert_eq!(
        trace()
            .iter()
            .filter(|line| line.starts_with("push "))
            .count(),
        pushes,
        "empty stream needs no silence"
    );
}

fn run_qwen(native: &mut NativeAsr) {
    for bad in [&[][..], &[f32::NAN], &[f32::NEG_INFINITY], &[f32::INFINITY]] {
        assert!(native.transcribe(bad, "", None).is_err());
    }
    assert!(!trace().iter().any(|line| line.starts_with("audio ")));
    let max = 115 * 16000;
    for n in [max, max + 1] {
        let mut pcm = vec![0.5; n];
        pcm[n - 1] = -0.25;
        let result = native.transcribe(&pcm, "专名", Some("zh")).unwrap();
        assert_eq!(
            (result.text.as_str(), result.language.as_deref()),
            ("原文", Some("Chinese"))
        );
        let log = trace();
        assert!(has(&log, &format!("audio {n} 16000 1 0.5 -0.25")));
        let last_mode = log
            .iter()
            .rev()
            .find(|line| line.starts_with("request-option audio_chunk_mode "))
            .unwrap();
        assert_eq!(
            last_mode,
            if n == max {
                "request-option audio_chunk_mode none"
            } else {
                "request-option audio_chunk_mode auto"
            }
        );
    }
    let log = trace();
    for line in [
        "request-option max_tokens 512",
        "request-option return_timestamps false",
        "session asr offline cpu 0 3",
        "text 专名 zh",
    ] {
        assert!(has(&log, line), "{line}");
    }
    assert!(
        log.iter()
            .any(|line| line.starts_with("model qwen3_asr ") && line.ends_with("语音.gguf"))
    );
}

#[test]
#[ignore = "subprocess entrypoint for native_abi_contract"]
fn exercise_one_native_case() {
    let Ok(case) = env::var("LW_FIXTURE_CASE") else {
        return;
    };
    let abi: u32 = env::var("LW_FIXTURE_ABI").unwrap().parse().unwrap();
    let cuda = case == "cuda";
    let qwen = [
        "qwen",
        "fail_request_set_option",
        "fail_request_set_audio",
        "fail_session_run",
        "fail_result_text",
    ]
    .contains(&case.as_str());
    let model: PathBuf = env::var_os("LW_FIXTURE_MODEL").unwrap().into();
    let dll: PathBuf = env::var_os("LW_FIXTURE_DLL").unwrap().into();
    let opened = NativeAsr::open(
        &dll,
        &model,
        if qwen { Family::Qwen } else { Family::R2t2 },
        if cuda { Backend::Cuda } else { Backend::Cpu },
        match case.as_str() {
            "threads" => 0,
            "one_thread" => 1,
            "max_threads" => i32::MAX,
            _ => 3,
        },
    );
    if case == "unsupported" || case == "threads" {
        assert!(opened.is_err());
        assert!(!trace().iter().any(|line| line.starts_with("alloc ")));
        return;
    }
    if [
        "fail_registry_create",
        "fail_model_load",
        "null_options",
        "fail_options_set",
        "fail_session_create",
    ]
    .contains(&case.as_str())
    {
        let error = opened
            .err()
            .expect("failed allocation or status must propagate");
        if case.starts_with("fail_") {
            assert!(error.contains("fixture detail"), "{error}");
        }
        let log = trace();
        assert!(log.iter().any(|line| line.starts_with("alloc registry ")));
        check_ownership(&log);
        return;
    }
    let mut native = opened.expect("valid fixture opened");
    assert_eq!(native.chunk_samples(), if cuda { 2560 } else { 5120 });
    match case.as_str() {
        "one_thread" => assert!(has(&trace(), "session asr streaming cpu 0 1")),
        "max_threads" => assert!(has(&trace(), "session asr streaming cpu 0 2147483647")),
        "cpu" | "cuda" => run_r2t2(&mut native, cuda, abi),
        "qwen" => run_qwen(&mut native),
        "null_request" | "fail_request_set_text" | "fail_stream_start" => {
            assert!(native.start("", None).is_err())
        }
        "fail_request_set_option"
        | "fail_request_set_audio"
        | "fail_session_run"
        | "fail_result_text" => assert!(native.transcribe(&[0.25], "", None).is_err()),
        "fail_stream_finish" => {
            native.start("", None).unwrap();
            assert!(native.finish().is_err());
        }
        "fail_stream_push" | "fail_result_preview_text" | "conflict" => {
            native.start("", None).unwrap();
            assert!(native.feed(&[0.25]).is_err());
        }
        "null_event" | "no_result_text" => {
            native.start("", Some("zh")).unwrap();
            let out = native.feed(&[0.25]).unwrap();
            assert_eq!(
                (out.text.as_str(), out.language.as_deref()),
                ("", Some("zh"))
            );
            assert_eq!(out.preview, if case == "null_event" { "" } else { "A?" });
        }
        _ => unreachable!(),
    }
    drop(native);
    check_ownership(&trace());
}
