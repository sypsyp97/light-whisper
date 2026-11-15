//! Synthetic IPC fixture. No audio capture, model loading, network or inference.
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
fn watch_parent() {
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        exe: [u16; 260],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    // Open the exact live spawning host before publishing fixture readiness.
    // The kernel handle cannot accidentally follow a subsequently reused PID.
    let handle = unsafe {
        let snapshot = CreateToolhelp32Snapshot(2, 0);
        assert_ne!(snapshot as isize, -1);
        let mut entry: ProcessEntry = std::mem::zeroed();
        entry.size = std::mem::size_of::<ProcessEntry>() as u32;
        let mut found = Process32FirstW(snapshot, &mut entry);
        let mut parent = 0;
        while found != 0 {
            if entry.pid == std::process::id() {
                parent = entry.parent;
                break;
            }
            found = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        assert_ne!(parent, 0, "fixture parent must exist");
        let handle = OpenProcess(0x0010_0000, 0, parent);
        assert!(!handle.is_null(), "open live host handle before readiness");
        handle as usize
    };
    std::thread::spawn(move || unsafe {
        let result = WaitForSingleObject(handle as Handle, u32::MAX);
        CloseHandle(handle as Handle);
        std::process::exit(if result == 0 { 0 } else { 93 });
    });
}

fn record(root: &Path, value: Value) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("audit.jsonl"))
        .unwrap();
    writeln!(file, "{value}").unwrap();
    file.flush().unwrap();
}

fn gate(root: &Path, kind: &str) {
    if root.join(format!("hold-{kind}")).is_file() {
        fs::write(root.join(format!("entered-{kind}")), b"entered").unwrap();
        // File barriers establish ordering; this poll only bounds CPU use.
        while !root.join(format!("release-{kind}")).is_file() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn main() {
    #[cfg(windows)]
    watch_parent();
    let root =
        std::path::PathBuf::from(std::env::var_os("LIGHT_WHISPER_HOST_FIXTURE_ROOT").unwrap());
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 3, "production bundled engine invocation");
    assert_eq!(&args[..2], ["serve", "--engine"]);
    let engine = &args[2];
    assert!(matches!(
        engine.as_str(),
        "qwen3-asr-0.6b" | "confucius4-r2t2"
    ));
    record(
        &root,
        json!({"event":"spawn", "pid":std::process::id(), "args":args,
        "data_dir":std::env::var("LIGHT_WHISPER_DATA_DIR").unwrap(),
        "engine":engine, "model_cache":std::env::var("HF_HUB_CACHE").unwrap()}),
    );
    gate(&root, "startup");
    println!(
        "{}",
        json!({"success":true,"initialized":true,"model_loaded":true,
        "engine":engine,"device":"cpu","message":"synthetic IPC fixture; no inference"})
    );
    std::io::stdout().flush().unwrap();
    let mut count = 0;
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        record(
            &root,
            json!({"event":"command","pid":std::process::id(),"request":request}),
        );
        let mut response = match request["action"].as_str().unwrap() {
            "status" => json!({"success":true,"initialized":true,"model_loaded":true,
                "engine":engine,"device":"cpu"}),
            "stats" => json!({"success":true,"stats":{"transcription_count":count,
                "total_audio_duration":count as f64 * 0.5,"engine":engine,"backend":"synthetic-ipc"}}),
            "transcribe" => {
                let path = request["audio_path"].as_str().unwrap();
                let bytes = fs::read(path).unwrap();
                record(
                    &root,
                    json!({"event":"audio","pid":std::process::id(),
                    "path":path,"bytes":bytes,"request":request}),
                );
                gate(&root, "transcribe");
                count += 1;
                json!({"success":true,"text":"Synthetic 原文 transcription","duration":0.5,"language":"en"})
            }
            "set_audio_processing" | "set_gpu_idle" => json!({"success":true}),
            "exit" => {
                println!(
                    "{}",
                    json!({"success":true,"request_id":request["request_id"]})
                );
                std::io::stdout().flush().unwrap();
                return;
            }
            action => panic!("unexpected native command: {action}"),
        };
        response["request_id"] = request["request_id"].clone();
        println!("{response}");
        std::io::stdout().flush().unwrap();
    }
}
