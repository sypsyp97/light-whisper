use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must live below the repository root")
        .to_path_buf()
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("contract source must be readable")
}

#[test]
fn sensevoice_is_not_a_selectable_or_runnable_engine() {
    let root = repo_root();
    let settings = read(root.join("src/pages/SettingsPage.tsx"));
    let engine_entry = read(root.join("src-tauri/resources/engine.py"));
    let rust_paths = read(root.join("src-tauri/src/utils/paths.rs"));

    assert!(!settings.contains("key: \"sensevoice\""));
    assert!(!engine_entry.contains("\"sensevoice\""));
    assert!(!rust_paths.contains("\"sensevoice\""));
    assert!(rust_paths.contains("\"qwen3-asr-0.6b\".to_string()"));
}

#[test]
fn sensevoice_runtime_is_not_packaged() {
    let root = repo_root();
    let build_script = read(root.join("scripts/build_engine.py"));
    let tauri_config = read(root.join("src-tauri/tauri.conf.json"));

    assert!(!root.join("src-tauri/resources/funasr_server.py").exists());
    assert!(!build_script.to_ascii_lowercase().contains("funasr"));
    assert!(!tauri_config.contains("funasr_server.py"));
}
