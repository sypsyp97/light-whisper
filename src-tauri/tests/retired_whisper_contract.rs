use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri should have a repository parent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

#[test]
fn whisper_engine_is_retired_from_runtime_and_ui() {
    let settings = read("src/pages/SettingsPage.tsx");
    let engine_cli = read("src-tauri/resources/engine.py");
    let rust_paths = read("src-tauri/src/utils/paths.rs");
    let rust_commands = read("src-tauri/src/commands/funasr.rs");
    let rust_runtime = read("src-tauri/src/services/funasr_service.rs");
    let tauri_config = read("src-tauri/tauri.conf.json");

    assert!(!settings.contains("key: \"whisper\""));
    assert!(!engine_cli.contains("choices=[\"whisper\""));
    assert!(!rust_paths.contains("\"whisper\""));
    assert!(!rust_paths.contains("get_whisper_server_path"));
    assert!(!rust_commands.contains("\"whisper\","));
    assert!(!rust_runtime.contains("WHISPER_REPO_ID"));
    assert!(!tauri_config.contains("resources/whisper_server.py"));
    assert!(!repo_root()
        .join("src-tauri/resources/whisper_server.py")
        .exists());
}

#[test]
fn bundled_qwen_runtime_uses_firered_vad_without_whisper_dependencies() {
    let project = read("pyproject.toml");
    let build = read("scripts/build_engine.py");
    let qwen_server = read("src-tauri/resources/qwen3_asr_server.py");

    assert!(project.contains("kaldi-native-fbank"));
    assert!(project.contains("onnxruntime"));
    assert!(!project.contains("faster-whisper"));
    assert!(!project.contains("librosa"));
    assert!(build.contains("fireredvad_vad.onnx"));
    assert!(!build.contains("whisper_server.py"));
    assert!(qwen_server.contains("FireRedVad"));
    assert!(!qwen_server.contains("faster_whisper"));
}
