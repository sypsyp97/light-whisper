use tauri::Emitter;
use tauri_plugin_keyring::KeyringExt;

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
    let keyring = app_handle.keyring();
    if api_key.is_empty() {
        let _ = keyring.delete_password(llm_provider::KEYRING_SERVICE, ONLINE_ASR_KEYRING_USER);
    } else {
        keyring
            .set_password(llm_provider::KEYRING_SERVICE, ONLINE_ASR_KEYRING_USER, &api_key)
            .map_err(|e| AppError::Other(format!("保存在线 ASR API Key 失败: {}", e)))?;
    }
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
            }),
        );
    }

    Ok(())
}

#[tauri::command]
pub async fn get_online_asr_api_key(
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
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
