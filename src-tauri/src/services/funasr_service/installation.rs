use crate::state::AppState;
use crate::utils::{paths, AppError};
use tauri::Emitter;

use super::{now_unix_ms, EngineProgressGate, ENGINE_ARCHIVE_FINGERPRINT};

pub(super) fn engine_install_fingerprint_matches(installed: &str, expected: &str) -> bool {
    let installed = installed.trim();
    installed == expected
        || (expected == ENGINE_ARCHIVE_FINGERPRINT
            && installed
                .rsplit_once('+')
                .is_some_and(|(_, archive_fingerprint)| archive_fingerprint == expected))
}

pub async fn find_engine(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    expected_generation: u64,
) -> Result<String, AppError> {
    let progress_gate = EngineProgressGate {
        generation: state.engine.funasr_generation.clone(),
        status_commit: state.engine.funasr_status_commit.clone(),
        expected_generation,
    };
    let _install_guard = state.engine.engine_install_op.lock().await;
    if !progress_gate.is_current() {
        return Err(AppError::Asr("引擎查找已被取消".to_string()));
    }
    let expected_fingerprint = ENGINE_ARCHIVE_FINGERPRINT;

    if let Some(engine_path) = paths::get_engine_exe_path(app_handle) {
        let version_file = paths::get_engine_dir().join(".version");
        let installed_version = std::fs::read_to_string(&version_file).unwrap_or_default();

        if engine_install_fingerprint_matches(&installed_version, expected_fingerprint) {
            let path_str = paths::strip_win_prefix(&engine_path);
            log::info!("找到引擎: {} ({})", path_str, expected_fingerprint);
            return Ok(path_str);
        }

        log::info!(
            "引擎指纹不匹配 (已安装: {:?}, 当前: {}), 需要重新解压",
            installed_version.trim(),
            expected_fingerprint
        );
    }

    if let Some(archive_path) = paths::get_engine_archive_path(app_handle) {
        log::info!("找到引擎压缩包，准备解压: {}", archive_path.display());
        let engine_exe = extract_engine_archive(&archive_path, app_handle, &progress_gate).await?;
        let path_str = paths::strip_win_prefix(&engine_exe);
        log::info!("引擎解压完成: {}", path_str);
        return Ok(path_str);
    }

    if let Some(engine_path) = paths::get_resource_engine_exe_path(app_handle) {
        let path_str = paths::strip_win_prefix(&engine_path);
        log::info!("找到资源目录引擎: {}", path_str);
        return Ok(path_str);
    }

    Err(AppError::Asr(
        "缺少本地语音引擎，请重新安装轻语 Whisper。".into(),
    ))
}

async fn extract_engine_archive(
    archive_path: &std::path::Path,
    app_handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<std::path::PathBuf, AppError> {
    progress_gate.commit_if_current(|| {
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "首次启动，正在解压引擎文件..."
            }),
        );
    });

    let engine_dir = paths::get_engine_dir();
    let archive = archive_path.to_path_buf();
    let handle = app_handle.clone();
    let progress_gate = progress_gate.clone();

    tokio::task::spawn_blocking(move || {
        let parent = engine_dir
            .parent()
            .ok_or_else(|| AppError::Asr("引擎目录缺少父目录".to_string()))?;
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Asr(format!("创建引擎父目录失败: {}", e)))?;

        let stamp = now_unix_ms();
        let staging_dir = parent.join(format!("engine.staging.{}", stamp));
        let backup_dir = parent.join(format!("engine.backup.{}", stamp));

        if staging_dir.exists() {
            let _ = std::fs::remove_dir_all(&staging_dir);
        }
        std::fs::create_dir_all(&staging_dir)
            .map_err(|e| AppError::Asr(format!("创建引擎目录失败: {}", e)))?;

        let extract_result = (|| -> Result<usize, AppError> {
            let archive_name = archive
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();

            let total = if archive_name.ends_with(".tar.xz") {
                extract_tar_xz_archive(&archive, &staging_dir, &handle, &progress_gate)?
            } else {
                extract_zip_archive(&archive, &staging_dir, &handle, &progress_gate)?
            };

            if total == 0 {
                return Err(AppError::Asr("引擎归档为空".to_string()));
            }
            let staged_exe = staging_dir.join("engine.exe");
            if !staged_exe.is_file() {
                return Err(AppError::Asr("引擎归档缺少 engine.exe".to_string()));
            }

            let fingerprint = ENGINE_ARCHIVE_FINGERPRINT;
            paths::atomic_write(
                staging_dir.join(".version").as_path(),
                fingerprint.as_bytes(),
            )
            .map_err(|e| AppError::Asr(format!("写入引擎版本标记失败: {}", e)))?;

            Ok(total)
        })();

        let total = match extract_result {
            Ok(total) => total,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return Err(err);
            }
        };

        replace_engine_dir(&engine_dir, &staging_dir, &backup_dir)?;
        let _ = std::fs::remove_dir_all(&backup_dir);

        log::info!("引擎解压完成: {} 个条目", total);
        Ok(engine_dir.join("engine.exe"))
    })
    .await
    .map_err(|e| AppError::Asr(format!("解压任务异常: {}", e)))?
}

pub(super) fn replace_engine_dir(
    engine_dir: &std::path::Path,
    staging_dir: &std::path::Path,
    backup_dir: &std::path::Path,
) -> Result<(), AppError> {
    if backup_dir.exists() {
        let _ = std::fs::remove_dir_all(backup_dir);
    }

    let had_previous = engine_dir.exists();
    if had_previous {
        std::fs::rename(engine_dir, backup_dir)
            .map_err(|e| AppError::Asr(format!("备份旧引擎失败: {}", e)))?;
    }

    match std::fs::rename(staging_dir, engine_dir) {
        Ok(()) => Ok(()),
        Err(err) => {
            if had_previous {
                if let Err(restore_err) = std::fs::rename(backup_dir, engine_dir) {
                    return Err(AppError::Asr(format!(
                        "替换引擎目录失败: {}; 恢复旧引擎也失败: {}（备份保留在 {}）",
                        err,
                        restore_err,
                        backup_dir.display()
                    )));
                }
            }
            Err(AppError::Asr(format!("替换引擎目录失败: {}", err)))
        }
    }
}

fn extract_zip_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<usize, AppError> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("打开引擎压缩包失败: {}", e)))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Asr(format!("读取引擎压缩包失败: {}", e)))?;

    let total = zip.len();
    log::info!("开始解压 ZIP 引擎归档: {} 个文件", total);

    for i in 0..total {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Asr(format!("读取压缩条目失败: {}", e)))?;

        let entry_path = engine_dir.join(
            entry
                .enclosed_name()
                .ok_or_else(|| AppError::Asr("压缩包含不安全路径".to_string()))?,
        );

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)
                .map_err(|e| AppError::Asr(format!("创建目录失败: {}", e)))?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| AppError::Asr(format!("创建父目录失败: {}", e)))?;
            }
            let mut outfile = std::fs::File::create(&entry_path)
                .map_err(|e| AppError::Asr(format!("创建文件失败: {}", e)))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| AppError::Asr(format!("写入文件失败: {}", e)))?;
        }

        report_extract_progress(handle, progress_gate, i + 1, Some(total), false);
    }

    Ok(total)
}

fn extract_tar_xz_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<usize, AppError> {
    log::info!("开始解压 TAR.XZ 引擎归档");

    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("打开引擎压缩包失败: {}", e)))?;
    let decoder = xz2::read::XzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut extracted = 0usize;

    for entry_result in tar
        .entries()
        .map_err(|e| AppError::Asr(format!("读取引擎压缩包失败: {}", e)))?
    {
        let mut entry =
            entry_result.map_err(|e| AppError::Asr(format!("读取压缩条目失败: {}", e)))?;
        entry
            .unpack_in(engine_dir)
            .map_err(|e| AppError::Asr(format!("写入文件失败: {}", e)))?;
        extracted += 1;

        report_extract_progress(handle, progress_gate, extracted, None, false);
    }

    if extracted > 0 && !extracted.is_multiple_of(200) {
        report_extract_progress(handle, progress_gate, extracted, None, true);
    }

    Ok(extracted)
}

fn report_extract_progress(
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
    current: usize,
    total: Option<usize>,
    force: bool,
) {
    let should_emit = force || current.is_multiple_of(200) || total.is_some_and(|t| current == t);

    if !should_emit {
        return;
    }
    progress_gate.commit_if_current(|| {
        if let Some(total) = total {
            if total == 0 {
                return;
            }
            let pct = current * 100 / total;
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "loading",
                    "message": format!("正在解压引擎文件... {}%", pct)
                }),
            );
        } else {
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "loading",
                    "message": format!("正在解压引擎文件... 已处理 {} 项", current)
                }),
            );
        }
    });
}
