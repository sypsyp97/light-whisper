// Deterministic protocol peer for CLI process tests. The CLI checks this marker
// in the actual executable before it will start a desktop broker.
use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

fn main() {
    let record = PathBuf::from(env::var_os("LW_TEST_RECORD").unwrap());
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(record)
        .unwrap();
    writeln!(log, "START LIGHT_WHISPER_AGENT_PROTOCOL_1").unwrap();
    for arg in env::args().skip(1) {
        writeln!(log, "ARG {arg}").unwrap();
    }
    for key in [
        "LIGHT_WHISPER_DATA_DIR",
        "HF_HUB_OFFLINE",
        "TRANSFORMERS_OFFLINE",
        "HF_HUB_CACHE",
        "LIGHT_WHISPER_NATIVE_THREADS",
        "OMP_NUM_THREADS",
        "MKL_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
    ] {
        writeln!(log, "ENV {key}={}", env::var(key).unwrap_or_default()).unwrap();
    }
    let mode = env::var("LW_TEST_MODE").unwrap_or_default();
    if mode == "startup-fail" {
        println!(r#"{{"success":false,"error":"startup refused"}}"#);
        return;
    }
    println!(r#"{{"success":true}}"#);
    for (i, request) in io::stdin().lock().lines().enumerate() {
        let request = request.unwrap();
        writeln!(log, "WIRE {request}").unwrap();
        if request.contains(r#""exit""#) {
            return;
        }
        let seq = i + 1;
        if mode == "timeout" {
            thread::sleep(Duration::from_secs(10));
            return;
        }
        if mode == "eof" {
            return;
        }
        println!("not protocol");
        println!(r#"{{"success":false,"fatal":true,"request_id":999,"error":"stale"}}"#);
        println!(r#"{{"success":true,"request_id":true}}"#);
        if i == 0 && mode == "refuse" {
            println!(r#"{{"request_id":{seq},"success":false,"error":"refused"}}"#);
        } else if mode == "fatal" {
            println!(
                r#"{{"request_id":{seq},"fatal":true,"success":true,"error":"connection lost"}}"#
            );
        } else if mode == "nonfinite" {
            println!(r#"{{"request_id":{seq},"success":true,"nested":[1e400]}}"#);
            println!(r#"{{"request_id":{seq},"success":true}}"#);
        } else {
            println!(r#"{{"request_id":{seq},"success":true,"request":{request}}}"#);
        }
    }
}
