use std::fs;
use std::path::{Path, PathBuf};

pub const ENGINE_ARCHIVE_CANDIDATES: &[&str] = &["resources/engine.tar.xz", "resources/engine.zip"];

fn is_non_empty_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

pub fn select_engine_archive(
    manifest_dir: &Path,
    allow_placeholder: bool,
) -> Result<PathBuf, String> {
    let candidates: Vec<PathBuf> = ENGINE_ARCHIVE_CANDIDATES
        .iter()
        .map(|relative| manifest_dir.join(relative))
        .collect();

    if allow_placeholder {
        if let Some(archive) = candidates
            .iter()
            .find(|candidate| is_non_empty_file(candidate))
        {
            return Ok(archive.clone());
        }

        let placeholder = &candidates[0];
        if !placeholder.exists() {
            fs::File::create(placeholder).map_err(|error| {
                format!(
                    "无法创建开发用引擎占位文件 {}: {error}",
                    placeholder.display()
                )
            })?;
        }
        return Ok(placeholder.clone());
    }

    let release_archive = &candidates[0];
    if is_non_empty_file(release_archive) {
        return Ok(release_archive.clone());
    }

    Err(format!(
        "正式构建需要非空的 {}。请先运行 `uv run --locked python scripts/build_engine.py`，或复用已经验证过的归档。",
        release_archive.display()
    ))
}
