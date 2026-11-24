use serde_json::{Value, json};
use std::{
    io::Read,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub fn describe(cuda: bool, qwen: bool) -> Value {
    let mut v = json!({"device": if cuda {"cuda"}else{"cpu"}});
    if !qwen {
        v["gpu_name"] = Value::Null
    }
    if !cuda {
        return v;
    }
    let fields = if qwen { "name,memory.total" } else { "name" };
    let Ok(mut child) = Command::new("nvidia-smi")
        .creation_flags(0x08000000)
        .args([
            "--id=0",
            &format!("--query-gpu={fields}"),
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return v;
    };
    let limit = Instant::now() + Duration::from_secs(if qwen { 3 } else { 1 });
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return v,
            Ok(None) if Instant::now() < limit => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return v;
            }
        }
    }
    let mut output = String::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(16384).read_to_string(&mut output);
    }
    if let Some(line) = output.lines().next().filter(|s| !s.trim().is_empty()) {
        if qwen {
            if let Some((name, mb)) = line.rsplit_once(',')
                && let Ok(mb) = mb.trim().parse::<f64>()
            {
                v["gpu_name"] = json!(name.trim());
                v["gpu_memory_total"] = json!((mb / 1024. * 10.).round() / 10.);
            }
        } else {
            v["gpu_name"] = json!(line.trim())
        }
    }
    v
}
