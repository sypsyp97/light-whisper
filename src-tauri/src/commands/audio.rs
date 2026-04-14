use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::Emitter;

use crate::services::audio_service;
use crate::state::{
    AppState, PendingRecordingSession, RecordingMode, RecordingSession, RecordingSlot,
    RecordingTrigger,
};
use crate::utils::AppError;

pub(crate) const RECORDING_NOT_READY_ERROR: &str = "语音识别服务尚未就绪，请等待初始化完成";
pub(crate) const RECORDING_ALREADY_ACTIVE_ERROR: &str = "已有录音正在进行中";
pub(crate) const RECORDING_START_CANCELLED_ERROR: &str = "录音启动已取消";

fn clear_pending_recording_if_current(state: &AppState, session_id: u64) {
    let mut guard = state.recording.lock();
    if matches!(guard.as_ref(), Some(RecordingSlot::Starting(s)) if s.session_id == session_id) {
        *guard = None;
    }
}

pub(crate) async fn start_recording_inner(
    app_handle: tauri::AppHandle,
    state: &AppState,
    trigger: RecordingTrigger,
    // 调用方（如 hotkey 路径）在 start 之前已 spawn 的选中文本抓取任务，
    // 会被存进最终创建的 RecordingSession，由 finalize/discard 路径负责回收。
    // 本函数如果在任何路径上提前失败，这个本地 Option 会自然 drop，JoinHandle 被 detach。
    mut edit_grab: Option<tokio::task::JoinHandle<Option<String>>>,
) -> Result<u64, AppError> {
    if !state.is_funasr_ready() {
        return Err(AppError::Audio(RECORDING_NOT_READY_ERROR.into()));
    }

    audio_service::stop_microphone_level_monitor(state);

    let (session_id, stop_flag, stop_notify) = {
        let mut guard = state.recording.lock();
        if guard.is_some() {
            return Err(AppError::Audio(RECORDING_ALREADY_ACTIVE_ERROR.into()));
        }
        let session_id = state.session_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_notify = Arc::new(tokio::sync::Notify::new());
        *guard = Some(RecordingSlot::Starting(PendingRecordingSession {
            session_id,
            trigger,
            stop_flag: stop_flag.clone(),
            stop_notify: stop_notify.clone(),
        }));
        (session_id, stop_flag, stop_notify)
    };

    let samples: Arc<parking_lot::Mutex<Vec<i16>>> =
        Arc::new(parking_lot::Mutex::new(Vec::with_capacity(16000 * 30)));
    let interim_cache: Arc<parking_lot::Mutex<Option<crate::state::InterimCache>>> =
        Arc::new(parking_lot::Mutex::new(None));

    let (audio_thread, actual_sample_rate) = match audio_service::spawn_audio_capture_thread(
        stop_flag.clone(),
        samples.clone(),
        state.selected_input_device_name(),
    ) {
        Ok(r) => r,
        Err(e) => {
            clear_pending_recording_if_current(state, session_id);
            return Err(e);
        }
    };

    if stop_flag.load(Ordering::Relaxed) {
        clear_pending_recording_if_current(state, session_id);
        audio_service::discard_recording(RecordingSession {
            session_id,
            trigger,
            stop_flag,
            stop_notify,
            samples,
            sample_rate: actual_sample_rate,
            audio_thread: Some(audio_thread),
            interim_task: None,
            interim_cache,
            edit_grab: edit_grab.take(),
        })
        .await;
        return Err(AppError::Audio(RECORDING_START_CANCELLED_ERROR.into()));
    }

    let interim_task = audio_service::spawn_interim_loop(
        app_handle.clone(),
        session_id,
        stop_flag.clone(),
        stop_notify.clone(),
        samples.clone(),
        actual_sample_rate,
        interim_cache.clone(),
    );

    audio_service::spawn_waveform_emitter(
        app_handle.clone(),
        session_id,
        stop_flag.clone(),
        samples.clone(),
        actual_sample_rate,
    );

    let mut session = Some(RecordingSession {
        session_id,
        trigger,
        stop_flag,
        stop_notify,
        samples,
        sample_rate: actual_sample_rate,
        audio_thread: Some(audio_thread),
        interim_task: Some(interim_task),
        interim_cache,
        edit_grab: edit_grab.take(),
    });

    let cancelled = {
        let mut guard = state.recording.lock();
        match guard.as_ref() {
            Some(RecordingSlot::Starting(p)) if p.session_id == session_id => {
                if let Some(s) = session.take() {
                    *guard = Some(RecordingSlot::Active(s));
                }
                None
            }
            _ => session.take(),
        }
    };

    if let Some(s) = cancelled {
        s.stop_flag.store(true, Ordering::Relaxed);
        s.stop_notify.notify_waiters();
        audio_service::discard_recording(s).await;
        return Err(AppError::Audio(RECORDING_START_CANCELLED_ERROR.into()));
    }

    // 先发 recording-state，后显示字幕窗口。show_subtitle_window 在 Windows 上
    // CreateWindow + 布局会花 50-100ms，这段窗口里前端如果还没收到本会话的
    // recording-state，上一 session 的延迟 transcription-result 会钻进来覆盖当前
    // 显示（useRecording 的 stale 过滤只能挡住已登记的 latestSessionIdRef）。
    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": true,
            "isProcessing": false,
            "mode": trigger.mode().as_str(),
        }),
    );

    {
        let app = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::commands::window::show_subtitle_window(app).await {
                log::warn!("显示字幕窗口失败（录音继续）: {}", e);
            }
        });
    }

    if state.sound_enabled.load(Ordering::Acquire) {
        match trigger.mode() {
            RecordingMode::Dictation => crate::utils::sound::play_start_sound(),
            RecordingMode::Assistant => crate::utils::sound::play_assistant_start_sound(),
        }
    }
    log::info!(
        "录音已开始 (session {}, {}Hz, mode={})",
        session_id,
        actual_sample_rate,
        trigger.mode().as_str()
    );
    Ok(session_id)
}

pub(crate) async fn stop_recording_inner(
    app_handle: tauri::AppHandle,
    state: &AppState,
) -> Result<Option<u64>, AppError> {
    let recording = state.recording.lock().take();

    let recording = match recording {
        Some(s) => s,
        None => {
            log::warn!("stop_recording 被调用但没有活跃的录音会话");
            return Ok(None);
        }
    };

    let session = match recording {
        RecordingSlot::Starting(p) => {
            p.stop_flag.store(true, Ordering::Relaxed);
            p.stop_notify.notify_waiters();
            log::info!("录音启动阶段已取消 (session {})", p.session_id);
            let _ = app_handle.emit(
                "recording-state",
                serde_json::json!({
                    "sessionId": p.session_id,
                    "isRecording": false,
                    "isProcessing": false,
                    "mode": p.trigger.mode().as_str(),
                }),
            );
            return Ok(Some(p.session_id));
        }
        RecordingSlot::Active(s) => s,
    };

    let session_id = session.session_id;
    session.stop_flag.store(true, Ordering::Relaxed);
    session.stop_notify.notify_waiters();
    if state.sound_enabled.load(Ordering::Acquire) {
        match session.trigger.mode() {
            RecordingMode::Dictation => crate::utils::sound::play_stop_sound(),
            RecordingMode::Assistant => crate::utils::sound::play_assistant_stop_sound(),
        }
    }
    log::info!("正在停止录音 (session {})", session_id);
    let _ = app_handle.emit(
        "recording-state",
        serde_json::json!({
            "sessionId": session_id,
            "isRecording": false,
            "isProcessing": true,
            "mode": session.trigger.mode().as_str(),
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
    start_recording_inner(
        app_handle,
        state.inner(),
        RecordingTrigger::DictationOriginal,
        None,
    )
    .await
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
pub async fn test_microphone(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    let name = state.selected_input_device_name();
    tokio::task::spawn_blocking(move || audio_service::test_microphone_sync(name))
        .await
        .map_err(|e| AppError::Audio(format!("麦克风测试任务失败: {}", e)))?
}

#[tauri::command]
pub async fn list_input_devices(
    state: tauri::State<'_, AppState>,
) -> Result<audio_service::InputDeviceListPayload, AppError> {
    let name = state.selected_input_device_name();
    tokio::task::spawn_blocking(move || audio_service::list_input_devices_sync(name))
        .await
        .map_err(|e| AppError::Audio(format!("设备枚举任务失败: {}", e)))?
}

#[tauri::command]
pub async fn set_input_device(
    state: tauri::State<'_, AppState>,
    name: Option<String>,
) -> Result<(), AppError> {
    state.set_selected_input_device_name(name);
    Ok(())
}

#[tauri::command]
pub async fn start_microphone_level_monitor(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    audio_service::start_microphone_level_monitor(app_handle, state.inner())
}

#[tauri::command]
pub async fn stop_microphone_level_monitor(
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    audio_service::stop_microphone_level_monitor(state.inner());
    Ok(())
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
    *state.input_method.lock() = method;
    Ok(())
}
