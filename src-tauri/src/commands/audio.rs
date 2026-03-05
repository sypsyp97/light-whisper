use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::Emitter;

use crate::services::audio_service;
use crate::state::AppState;
use crate::utils::AppError;

pub(crate) const RECORDING_NOT_READY_ERROR: &str = "语音识别服务尚未就绪，请等待初始化完成";
pub(crate) const RECORDING_ALREADY_ACTIVE_ERROR: &str = "已有录音正在进行中";

pub(crate) async fn start_recording_inner(
    app_handle: tauri::AppHandle,
    state: &AppState,
) -> Result<u64, AppError> {
    if !state.is_funasr_ready() {
        return Err(AppError::Audio(RECORDING_NOT_READY_ERROR.into()));
    }

    {
        let guard = state
            .recording
            .lock()
            .map_err(|_| AppError::Audio("录音状态锁异常".into()))?;
        if guard.is_some() {
            return Err(AppError::Audio(RECORDING_ALREADY_ACTIVE_ERROR.into()));
        }
    }

    let session_id = state.session_counter.fetch_add(1, Ordering::Relaxed) + 1;
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_notify = Arc::new(tokio::sync::Notify::new());
    let samples: Arc<std::sync::Mutex<Vec<i16>>> =
        Arc::new(std::sync::Mutex::new(Vec::with_capacity(16000 * 30)));
    let interim_cache: Arc<std::sync::Mutex<Option<crate::state::InterimCache>>> =
        Arc::new(std::sync::Mutex::new(None));

    let (audio_thread, actual_sample_rate) =
        audio_service::spawn_audio_capture_thread(stop_flag.clone(), samples.clone())?;

    let interim_task = audio_service::spawn_interim_loop(
        app_handle.clone(),
        session_id,
        stop_flag.clone(),
        stop_notify.clone(),
        samples.clone(),
        actual_sample_rate,
        interim_cache.clone(),
    );

    {
        let mut guard = state
            .recording
            .lock()
            .map_err(|_| AppError::Audio("录音状态锁异常".into()))?;
        *guard = Some(crate::state::RecordingSession {
            session_id,
            stop_flag,
            stop_notify,
            samples,
            sample_rate: actual_sample_rate,
            audio_thread: Some(audio_thread),
            interim_task: Some(interim_task),
            interim_cache,
        });
    }

    if let Err(e) = crate::commands::window::show_subtitle_window(app_handle.clone()).await {
        log::warn!("显示字幕窗口失败（录音继续）: {}", e);
    }

    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": true,
            "isProcessing": false,
        }),
    );

    if state.sound_enabled.load(Ordering::Acquire) {
        crate::utils::sound::play_start_sound();
    }
    log::info!(
        "录音已开始 (session {}, {}Hz)",
        session_id,
        actual_sample_rate
    );
    Ok(session_id)
}

pub(crate) async fn stop_recording_inner(
    app_handle: tauri::AppHandle,
    state: &AppState,
) -> Result<Option<u64>, AppError> {
    let session = {
        let mut guard = state
            .recording
            .lock()
            .map_err(|_| AppError::Audio("录音状态锁异常".into()))?;
        guard.take()
    };

    let session = match session {
        Some(s) => s,
        None => {
            log::warn!("stop_recording 被调用但没有活跃的录音会话");
            return Ok(None);
        }
    };

    let session_id = session.session_id;
    session.stop_flag.store(true, Ordering::Relaxed);
    session.stop_notify.notify_waiters();
    if state.sound_enabled.load(Ordering::Acquire) {
        crate::utils::sound::play_stop_sound();
    }
    log::info!("正在停止录音 (session {})", session_id);
    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": false,
            "isProcessing": true,
        }),
    );

    tokio::spawn(async move {
        audio_service::finalize_recording(app_handle, session).await;
    });

    Ok(Some(session_id))
}

#[tauri::command]
pub async fn start_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u64, AppError> {
    start_recording_inner(app_handle, state.inner()).await
}

#[tauri::command]
pub async fn stop_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let _ = stop_recording_inner(app_handle, state.inner()).await?;
    Ok(())
}

#[tauri::command]
pub async fn test_microphone() -> Result<String, AppError> {
    tokio::task::spawn_blocking(audio_service::test_microphone_sync)
        .await
        .map_err(|e| AppError::Audio(format!("麦克风测试任务失败: {}", e)))?
}

#[tauri::command]
pub async fn set_sound_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), AppError> {
    state.sound_enabled.store(enabled, Ordering::Release);
    Ok(())
}

#[tauri::command]
pub async fn set_input_method(
    state: tauri::State<'_, AppState>,
    method: String,
) -> Result<(), AppError> {
    let mut guard = state
        .input_method
        .lock()
        .map_err(|_| AppError::Audio("输入方式锁异常".into()))?;
    *guard = method;
    Ok(())
}
