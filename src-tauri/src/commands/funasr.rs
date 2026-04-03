use std::sync::atomic::Ordering;
use tauri::Emitter;

use crate::services::funasr_service;
use crate::services::llm_provider;
use crate::state::AppState;
use crate::utils::{paths, AppError};

#[tauri::command]
pub async fn start_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR 服务器启动成功".to_string())
}

#[tauri::command]
pub async fn transcribe_audio(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    audio_base64: String,
) -> Result<funasr_service::TranscriptionResult, AppError> {
    use base64::Engine;
    let audio_data = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| AppError::Asr(format!("Base64 解码失败: {}", e)))?;
    funasr_service::transcribe(state.inner(), audio_data, &app_handle).await
}

#[tauri::command]
pub async fn check_funasr_status(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<funasr_service::FunASRStatus, AppError> {
    funasr_service::check_status(state.inner(), &app_handle).await
}

#[tauri::command]
pub async fn check_model_files() -> Result<funasr_service::ModelCheckResult, AppError> {
    funasr_service::check_model_files().await
}

#[tauri::command]
pub async fn download_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    crate::services::download_service::run_download(&app_handle, state.inner()).await
}

#[tauri::command]
pub async fn cancel_model_download(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    let task = {
        let mut guard = state.download_task.lock().await;
        guard.take()
    };

    if let Some(task) = task {
        let _ = task.cancel.send(());
        Ok("已取消模型下载".to_string())
    } else {
        Ok("当前没有下载任务".to_string())
    }
}

#[tauri::command]
pub async fn restart_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        // 在线引擎无需重启 Python，仅刷新就绪状态
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        return Ok("在线引擎状态已刷新".to_string());
    }

    // 首次启动可能仍在解压 engine.zip 或加载模型，自动重启不应打断这一过程。
    if state.funasr_starting.load(Ordering::SeqCst) {
        log::info!("FunASR 正在启动中，跳过本次重启请求");
        return Ok("FunASR 正在启动中，跳过重启".to_string());
    }

    log::info!("正在重启 FunASR 服务器...");
    funasr_service::stop_server(state.inner()).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR 服务器已重启".to_string())
}

#[tauri::command]
pub async fn get_engine() -> Result<String, AppError> {
    Ok(paths::read_engine_config())
}

#[tauri::command]
pub async fn set_engine(
    state: tauri::State<'_, AppState>,
    engine: String,
) -> Result<String, AppError> {
    if engine != "sensevoice" && engine != "whisper" && engine != "glm-asr" {
        return Err(AppError::Other(format!(
            "不支持的引擎类型: {}，可选值: sensevoice, whisper, glm-asr",
            engine
        )));
    }

    paths::write_engine_config(&engine)
        .map_err(|e| AppError::Other(format!("写入引擎配置失败: {}", e)))?;
    funasr_service::stop_server(state.inner()).await?;

    // 强制重置启动标志，确保新引擎可以立即启动。
    state
        .funasr_starting
        .store(false, std::sync::atomic::Ordering::SeqCst);

    // 在线引擎：根据 API Key 设置就绪状态并通知前端
    if paths::is_online_engine(&engine) {
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
    }

    log::info!("引擎已切换为: {}", engine);
    Ok(engine)
}

const ONLINE_ASR_KEYRING_USER: &str = "glm-asr-api-key";

#[tauri::command]
pub async fn set_online_asr_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_key: String,
) -> Result<(), AppError> {
    llm_provider::save_or_delete_api_key(&app_handle, ONLINE_ASR_KEYRING_USER, &api_key);
    state.set_online_asr_api_key(&api_key);

    // 如果当前是在线引擎，更新就绪状态并通知前端
    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        let has_key = !api_key.is_empty();
        state.set_funasr_ready(has_key);
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": if has_key { "ready" } else { "need_api_key" },
                "message": if has_key { "GLM-ASR 在线服务就绪" } else { "请配置 GLM-ASR API Key" },
                "device": "cloud",
                "gpu_name": serde_json::Value::Null,
                "models_present": true,
                "missing_models": [],
            }),
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn get_online_asr_api_key(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    Ok(state.read_online_asr_api_key())
}

#[tauri::command]
pub async fn get_online_asr_endpoint() -> Result<serde_json::Value, AppError> {
    Ok(serde_json::json!({
        "region": paths::read_online_asr_region(),
        "url": paths::read_online_asr_endpoint(),
    }))
}

#[tauri::command]
pub async fn set_online_asr_endpoint(region: String) -> Result<serde_json::Value, AppError> {
    if region != "international" && region != "domestic" {
        return Err(AppError::Other(format!(
            "不支持的区域: {}，可选值: international, domestic",
            region
        )));
    }
    paths::write_online_asr_endpoint(&region)
        .map_err(|e| AppError::Other(format!("写入端点配置失败: {}", e)))?;
    Ok(serde_json::json!({
        "region": paths::read_online_asr_region(),
        "url": paths::read_online_asr_endpoint(),
    }))
}

#[tauri::command]
pub async fn get_models_dir() -> Result<serde_json::Value, AppError> {
    let effective = paths::strip_win_prefix(&paths::get_effective_models_dir());
    let is_custom = paths::read_models_dir().is_some();
    Ok(serde_json::json!({
        "path": effective,
        "is_custom": is_custom,
    }))
}

#[tauri::command]
pub async fn pick_folder() -> Result<Option<String>, AppError> {
    let result = tokio::task::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .map_err(|e| AppError::Other(format!("文件夹选择失败: {}", e)))?;
    Ok(result.map(|p| paths::strip_win_prefix(&p)))
}

#[tauri::command]
pub async fn set_models_dir(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: Option<String>,
    migrate: bool,
) -> Result<String, AppError> {
    let old_dir = paths::get_effective_models_dir();
    let new_dir = match &path {
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p.trim()),
        _ => {
            // 恢复默认
            paths::write_models_dir(None)
                .map_err(|e| AppError::Other(format!("写入配置失败: {}", e)))?;
            return Ok("已恢复默认模型目录".to_string());
        }
    };

    // canonicalize 后比较，避免 Windows 大小写/尾部斜杠差异
    let canon_old = std::fs::canonicalize(&old_dir).unwrap_or_else(|_| old_dir.clone());
    // 目标目录可能还不存在，canonicalize 会失败，回退到原始路径比较
    let canon_new = std::fs::canonicalize(&new_dir).unwrap_or_else(|_| new_dir.clone());
    if canon_old == canon_new {
        return Ok("路径未变化".to_string());
    }

    // 确保目标目录存在（放 spawn_blocking 避免网络驱动器阻塞 async 线程）
    let new_dir_clone = new_dir.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&new_dir_clone))
        .await
        .map_err(|e| AppError::Other(format!("创建目录失败: {}", e)))?
        .map_err(|e| AppError::Other(format!("创建目录失败: {}", e)))?;

    if migrate && old_dir.is_dir() {
        // 设置 starting 标志，阻止前端轮询触发的自动重启
        state
            .funasr_starting
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let migration_result: Result<(), AppError> = async {
            funasr_service::stop_server(state.inner()).await?;

            let _ = app_handle.emit(
                "models-migrate-status",
                serde_json::json!({ "status": "migrating", "message": "正在迁移模型文件..." }),
            );

            let old = old_dir.clone();
            let dest = new_dir.clone();
            let handle = app_handle.clone();
            tokio::task::spawn_blocking(move || migrate_model_dirs(&old, &dest, &handle))
                .await
                .map_err(|e| AppError::Other(format!("迁移任务失败: {}", e)))?
                .map_err(|e| AppError::Other(format!("迁移失败: {}", e)))?;

            Ok(())
        }
        .await;

        // 无论成功失败都解除阻止，让后续 retryModel 能正常启动
        state
            .funasr_starting
            .store(false, std::sync::atomic::Ordering::SeqCst);

        migration_result?;
    }

    // 写入配置
    let dir_str = paths::strip_win_prefix(&new_dir);
    paths::write_models_dir(Some(&dir_str))
        .map_err(|e| AppError::Other(format!("写入配置失败: {}", e)))?;

    let _ = app_handle.emit(
        "models-migrate-status",
        serde_json::json!({ "status": "completed", "message": "模型目录已更新" }),
    );

    Ok("模型目录已更新".to_string())
}

/// 迁移 models--* 目录从 src 到 dst
///
/// 策略：先全量复制/rename，全部成功后再统一删除源目录。
/// 任何一步失败则中止，已完成的复制保留在目标，源文件不删除，保证数据不丢失。
fn migrate_model_dirs(
    src: &std::path::Path,
    dst: &std::path::Path,
    handle: &tauri::AppHandle,
) -> Result<(), String> {
    let entries: Vec<_> = std::fs::read_dir(src)
        .map_err(|e| format!("读取源目录失败: {}", e))?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("models--")
                && e.path().is_dir()
        })
        .collect();

    if entries.is_empty() {
        return Ok(());
    }

    let total = entries.len();
    // 记录需要跨盘复制（而非 rename）的目录，稍后统一删除源
    let mut copied_sources: Vec<std::path::PathBuf> = Vec::new();

    // 第一阶段：全量迁移（rename 或 copy）
    for (i, entry) in entries.iter().enumerate() {
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if dst_path.exists() {
            log::info!("迁移跳过（已存在）: {}", name.to_string_lossy());
        } else if std::fs::rename(&src_path, &dst_path).is_ok() {
            log::info!("迁移（rename）: {}", name.to_string_lossy());
        } else {
            // 跨盘：先复制，不立即删源
            copy_dir_recursive(&src_path, &dst_path)
                .map_err(|e| format!("复制 {} 失败: {}", name.to_string_lossy(), e))?;
            copied_sources.push(src_path);
            log::info!("迁移（copy）: {}", name.to_string_lossy());
        }

        let _ = handle.emit(
            "models-migrate-status",
            serde_json::json!({
                "status": "migrating",
                "message": format!("正在迁移 {}/{}...", i + 1, total),
                "progress": ((i + 1) as f64 / total as f64 * 100.0).round(),
            }),
        );
    }

    // 第二阶段：全部复制成功后，统一清理源目录
    for source in &copied_sources {
        if let Err(e) = std::fs::remove_dir_all(source) {
            log::warn!("清理源目录失败（不影响迁移结果）: {} — {}", source.display(), e);
        }
    }

    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
