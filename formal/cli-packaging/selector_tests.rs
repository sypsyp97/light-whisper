//! Standalone std-only tests of the actual Cargo resource selector.
#[path = "../../src-tauri/build_support.rs"]
mod build_support;

use build_support::select_cli_executable;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "light-whisper-cli-selector-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("resources")).unwrap();
        Self(root)
    }

    fn cli(&self) -> PathBuf {
        self.0.join("resources/light-whisper-cli.exe")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn debug_missing_creates_only_empty_cli_placeholder() {
    let scratch = Scratch::new();
    let selected = select_cli_executable(&scratch.0, true).unwrap();
    assert_eq!(selected, scratch.cli());
    assert_eq!(fs::metadata(selected).unwrap().len(), 0);
    assert_eq!(
        fs::read_dir(scratch.0.join("resources")).unwrap().count(),
        1
    );
}

#[test]
fn debug_preserves_existing_empty_placeholder() {
    let scratch = Scratch::new();
    fs::write(scratch.cli(), []).unwrap();
    assert_eq!(
        select_cli_executable(&scratch.0, true).unwrap(),
        scratch.cli()
    );
    assert_eq!(fs::read(scratch.cli()).unwrap(), b"");
}

#[test]
fn both_modes_preserve_and_select_existing_nonempty_cli() {
    let scratch = Scratch::new();
    let bytes = b"existing standalone executable bytes";
    fs::write(scratch.cli(), bytes).unwrap();
    for debug in [false, true] {
        assert_eq!(
            select_cli_executable(&scratch.0, debug).unwrap(),
            scratch.cli()
        );
        assert_eq!(fs::read(scratch.cli()).unwrap(), bytes);
    }
}

#[test]
fn release_rejects_missing_and_empty_without_creating_or_overwriting() {
    let scratch = Scratch::new();
    let error = select_cli_executable(&scratch.0, false).unwrap_err();
    assert!(error.contains("light-whisper-cli.exe"), "{error}");
    assert!(!scratch.cli().exists());
    fs::write(scratch.cli(), []).unwrap();
    let error = select_cli_executable(&scratch.0, false).unwrap_err();
    assert!(error.contains("light-whisper-cli.exe"), "{error}");
    assert_eq!(fs::metadata(scratch.cli()).unwrap().len(), 0);
}

#[test]
fn directory_is_never_an_executable_or_overwritten() {
    let scratch = Scratch::new();
    fs::create_dir(scratch.cli()).unwrap();
    let marker = scratch.cli().join("keep.txt");
    fs::write(&marker, b"unrelated user state").unwrap();
    for debug in [false, true] {
        let error = select_cli_executable(&scratch.0, debug).unwrap_err();
        assert!(error.contains("light-whisper-cli.exe"), "{error}");
        assert_eq!(fs::read(&marker).unwrap(), b"unrelated user state");
    }
}

#[test]
fn development_create_io_error_is_returned_with_context() {
    let scratch = Scratch::new();
    fs::remove_dir(scratch.0.join("resources")).unwrap();
    fs::write(scratch.0.join("resources"), b"blocking file").unwrap();
    let error = select_cli_executable(&scratch.0, true).unwrap_err();
    assert!(error.contains("light-whisper-cli.exe"), "{error}");
    assert!(error.len() > "light-whisper-cli.exe".len(), "{error}");
    assert_eq!(
        fs::read(scratch.0.join("resources")).unwrap(),
        b"blocking file"
    );
}

#[test]
fn cli_selection_does_not_modify_engine_archive() {
    let scratch = Scratch::new();
    let engine = scratch.0.join("resources/engine.tar.xz");
    fs::write(&engine, b"existing engine archive").unwrap();
    select_cli_executable(&scratch.0, true).unwrap();
    assert_eq!(fs::read(engine).unwrap(), b"existing engine archive");
}

#[test]
fn both_modes_return_metadata_error_and_preserve_existing_resources() {
    let scratch = Scratch::new();
    let cli_bytes = b"existing standalone executable bytes";
    fs::write(scratch.cli(), cli_bytes).unwrap();
    let marker = scratch.0.join("keep.txt");
    fs::write(&marker, b"owned unrelated state").unwrap();
    let manifest_dir = scratch.0.join("invalid\0manifest");
    let candidate = manifest_dir.join("resources/light-whisper-cli.exe");
    assert!(candidate.starts_with(&scratch.0));
    let metadata_error = fs::metadata(&candidate).unwrap_err();
    assert_eq!(metadata_error.kind(), std::io::ErrorKind::InvalidInput);
    let expected = format!("CLI resource {}: {metadata_error}", candidate.display());

    for allow_placeholder in [false, true] {
        assert_eq!(
            select_cli_executable(&manifest_dir, allow_placeholder),
            Err(expected.clone()),
            "allow_placeholder={allow_placeholder}"
        );
        assert_eq!(fs::read(scratch.cli()).unwrap(), cli_bytes);
        assert_eq!(fs::read(&marker).unwrap(), b"owned unrelated state");
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 2);
        assert_eq!(
            fs::read_dir(scratch.0.join("resources")).unwrap().count(),
            1
        );
    }
}
