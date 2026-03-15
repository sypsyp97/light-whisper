use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

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
    let metadata = fs::metadata(path)
        .unwrap_or_else(|err| panic!("无法读取引擎归档 {}: {}", path.display(), err));
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{:016x}-{:016x}", metadata.len(), modified)
}

fn main() {
    emit_rerun_hints();

    let engine_archive = selected_engine_archive_path();
    println!(
        "cargo:rustc-env=LIGHT_WHISPER_ENGINE_ARCHIVE_FINGERPRINT={}",
        compute_file_fingerprint(&engine_archive)
    );

    let attributes = {
        #[cfg(target_os = "windows")]
        {
            tauri_build::Attributes::new().windows_attributes(
                tauri_build::WindowsAttributes::new()
                    .app_manifest(include_str!("windows-app-manifest.xml")),
            )
        }

        #[cfg(not(target_os = "windows"))]
        {
            tauri_build::Attributes::new()
        }
    };

    tauri_build::try_build(attributes).expect("failed to run tauri build")
}
