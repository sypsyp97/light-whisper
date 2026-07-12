use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

const ENGINE_ARCHIVE_CANDIDATES: &[&str] = &["resources/engine.tar.xz", "resources/engine.zip"];

fn ensure_engine_archive_placeholder() -> PathBuf {
    let archive = PathBuf::from(ENGINE_ARCHIVE_CANDIDATES[0]);
    if !archive.exists() {
        let _ = fs::File::create(&archive);
    }
    archive
}

fn has_non_empty_archive(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.len() > 0)
        .unwrap_or(false)
}

fn selected_engine_archive_path() -> PathBuf {
    ENGINE_ARCHIVE_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|path| has_non_empty_archive(path))
        .unwrap_or_else(ensure_engine_archive_placeholder)
}

fn emit_rerun_hints() {
    for path in ENGINE_ARCHIVE_CANDIDATES {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
}

fn compute_file_fingerprint(path: &Path) -> String {
    let file = fs::File::open(path)
        .unwrap_or_else(|err| panic!("无法读取引擎归档 {}: {}", path.display(), err));
    let mut reader = BufReader::with_capacity(8 * 1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 8 * 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .unwrap_or_else(|err| panic!("无法计算引擎归档摘要 {}: {}", path.display(), err));
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn main() {
    emit_rerun_hints();

    let engine_archive = selected_engine_archive_path();
    println!(
        "cargo:rustc-env=LIGHT_WHISPER_ENGINE_ARCHIVE_FINGERPRINT={}",
        compute_file_fingerprint(&engine_archive)
    );

    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml")),
    );

    tauri_build::try_build(attributes).expect("failed to run tauri build")
}
