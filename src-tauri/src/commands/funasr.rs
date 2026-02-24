use crate::services::funasr_service;
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
        .map_err(|e| AppError::FunASR(format!("Base64 解码失败: {}", e)))?;
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
    if engine != "sensevoice" && engine != "whisper" {
        return Err(AppError::FunASR(format!(
            "不支持的引擎类型: {}，可选值: sensevoice, whisper",
            engine
        )));
    }

    paths::write_engine_config(&engine)
        .map_err(|e| AppError::FunASR(format!("写入引擎配置失败: {}", e)))?;
    funasr_service::stop_server(state.inner()).await?;

    // 强制重置启动标志，确保新引擎可以立即启动。
    // 如果旧的 start_server 仍在运行（持有 StartingFlagGuard），
    // 它会在失败后把标志设回 false，不影响新引擎的启动。
    state
        .funasr_starting
        .store(false, std::sync::atomic::Ordering::SeqCst);

    log::info!("引擎已切换为: {}", engine);
    Ok(engine)
}
