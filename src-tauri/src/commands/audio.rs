use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::Emitter;

use crate::services::audio_service;
use crate::state::AppState;
use crate::utils::AppError;

#[tauri::command]
pub async fn start_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u64, AppError> {
    if !state.is_funasr_ready() {
        return Err(AppError::Other(
            "语音识别服务尚未就绪，请等待初始化完成".into(),
        ));
    }

    {
        let guard = state
            .recording
            .lock()
            .map_err(|_| AppError::Other("录音状态锁异常".into()))?;
        if guard.is_some() {
            return Err(AppError::Other("已有录音正在进行中".into()));
        }
    }

    let session_id = state.session_counter.fetch_add(1, Ordering::Relaxed) + 1;
    let stop_flag = Arc::new(AtomicBool::new(false));
    let samples: Arc<std::sync::Mutex<Vec<i16>>> =
        Arc::new(std::sync::Mutex::new(Vec::with_capacity(16000 * 30)));

    let (audio_thread, actual_sample_rate) =
        audio_service::spawn_audio_capture_thread(stop_flag.clone(), samples.clone())?;

    let interim_task = audio_service::spawn_interim_loop(
        app_handle.clone(),
        session_id,
        stop_flag.clone(),
        samples.clone(),
        actual_sample_rate,
    );

    {
        let mut guard = state
            .recording
            .lock()
            .map_err(|_| AppError::Other("录音状态锁异常".into()))?;
        *guard = Some(crate::state::RecordingSession {
            session_id,
            stop_flag,
            samples,
            sample_rate: actual_sample_rate,
            audio_thread: Some(audio_thread),
            interim_task: Some(interim_task),
        });
    }

    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": true,
            "isProcessing": false,
        }),
    );

    let app_for_subtitle = app_handle.clone();
    tokio::spawn(async move {
        let _ = crate::commands::window::show_subtitle_window(app_for_subtitle).await;
    });

    log::info!("录音已开始 (session {}, {}Hz)", session_id, actual_sample_rate);
    Ok(session_id)
}

#[tauri::command]
pub async fn stop_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let session = {
        let mut guard = state
            .recording
            .lock()
            .map_err(|_| AppError::Other("录音状态锁异常".into()))?;
        guard.take()
    };

    let session = match session {
        Some(s) => s,
        None => {
            log::warn!("stop_recording 被调用但没有活跃的录音会话");
            return Ok(());
        }
    };

    session.stop_flag.store(true, Ordering::Relaxed);
    log::info!("正在停止录音 (session {})", session.session_id);

    // 后台执行最终转写，不阻塞命令返回
    tokio::spawn(async move {
        audio_service::finalize_recording(app_handle, session).await;
    });

    Ok(())
}

#[tauri::command]
pub async fn test_microphone() -> Result<String, AppError> {
    tokio::task::spawn_blocking(audio_service::test_microphone_sync)
        .await
        .map_err(|e| AppError::Other(format!("麦克风测试任务失败: {}", e)))?
}

#[tauri::command]
pub async fn set_input_method(
    state: tauri::State<'_, AppState>,
    method: String,
) -> Result<(), AppError> {
    let mut guard = state
        .input_method
        .lock()
        .map_err(|_| AppError::Other("输入方式锁异常".into()))?;
    *guard = method;
    Ok(())
}
