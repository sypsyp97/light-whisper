use light_whisper_native_asr::Backend;
use serde_json::{Value, json};
use std::{
    io::Read,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn capture(command: &mut Command, timeout_secs: u64) -> Option<String> {
    let mut child = command
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let limit = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if Instant::now() < limit => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(16384).read_to_string(&mut output);
    }
    Some(output)
}
pub fn describe(backend: Backend, qwen: bool) -> Value {
    let name = match backend {
        Backend::Cuda => "cuda",
        Backend::Vulkan => "vulkan",
        Backend::Cpu => "cpu",
    };
    let mut v = json!({"device": name});
    if !qwen {
        v["gpu_name"] = Value::Null
    }
    if backend == Backend::Cpu {
        return v;
    }
    if backend == Backend::Vulkan {
        // ICD registration differs per vendor; ask the loader for real devices.
        let output = capture(
            Command::new("vulkaninfo").arg("--summary"),
            if qwen { 3 } else { 1 },
        );
        if let Some(name) = output.and_then(|text| {
            text.lines()
                .map(|line| line.split_once('='))
                .find_map(|pair| match pair {
                    Some((head, value)) if head.trim() == "deviceName" => {
                        let name = value.trim();
                        (!name.is_empty()).then(|| name.to_owned())
                    }
                    _ => None,
                })
        }) {
            v["gpu_name"] = json!(name)
        }
        return v;
    }
    let fields = if qwen { "name,memory.total" } else { "name" };
    let output = capture(
        Command::new("nvidia-smi").args([
            "--id=0",
            &format!("--query-gpu={fields}"),
            "--format=csv,noheader,nounits",
        ]),
        if qwen { 3 } else { 1 },
    );
    if let Some(line) = output.and_then(|text| {
        text.lines()
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }) {
        if qwen {
            if let Some((name, mb)) = line.rsplit_once(',')
                && let Ok(mb) = mb.trim().parse::<f64>()
            {
                v["gpu_name"] = json!(name.trim());
                v["gpu_memory_total"] = json!((mb / 1024. * 10.).round() / 10.);
            }
        } else {
            v["gpu_name"] = json!(line)
        }
    }
    v
}
