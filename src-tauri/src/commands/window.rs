use std::sync::atomic::Ordering;

use crate::state::{AppState, RecordingMode, RecordingPhase};
use crate::utils::AppError;
use tauri::{Emitter, Manager};

/// 使用 Windows API 强制将窗口置于最顶层
#[cfg(target_os = "windows")]
fn force_window_topmost(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    match window.hwnd() {
        Ok(hwnd) => {
            let raw = hwnd.0;
            let topmost = -1isize as *mut std::ffi::c_void;
            unsafe {
                SetWindowPos(
                    raw,
                    topmost,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        Err(err) => log::warn!("获取字幕窗口句柄失败: {}", err),
    }
}

const DEFAULT_SUBTITLE_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_SUBTITLE_WINDOW_HEIGHT: f64 = 720.0;

fn tauri_error(action: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Tauri(format!("{}: {}", action, err))
}

fn require_window(
    app_handle: &tauri::AppHandle,
    label: &str,
    missing_message: &str,
) -> Result<tauri::WebviewWindow, AppError> {
    app_handle
        .get_webview_window(label)
        .ok_or_else(|| AppError::Tauri(missing_message.to_string()))
}

/// 获取光标所在显示器（物理坐标比对）
#[cfg(target_os = "windows")]
fn find_cursor_monitor(app_handle: &tauri::AppHandle) -> Option<tauri::Monitor> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }

    app_handle
        .available_monitors()
        .ok()?
        .into_iter()
        .find(|monitor| {
            let pos = monitor.position();
            let size = monitor.size();
            point.x >= pos.x
                && point.x < pos.x + size.width as i32
                && point.y >= pos.y
                && point.y < pos.y + size.height as i32
        })
}

#[cfg(not(target_os = "windows"))]
fn find_cursor_monitor(_app_handle: &tauri::AppHandle) -> Option<tauri::Monitor> {
    None
}

fn resolve_subtitle_layout(app_handle: &tauri::AppHandle) -> (f64, f64, f64, f64) {
    let monitor = find_cursor_monitor(app_handle)
        .or_else(|| {
            app_handle
                .get_webview_window("main")
                .and_then(|window| window.current_monitor().ok().flatten())
        })
        .or_else(|| app_handle.primary_monitor().ok().flatten())
        .or_else(|| {
            app_handle
                .available_monitors()
                .ok()
                .and_then(|monitors| monitors.into_iter().next())
        });

    if let Some(monitor) = monitor {
        let screen_size = monitor.size();
        let screen_pos = monitor.position();
        let scale_factor = monitor.scale_factor();
        let logical_width = (screen_size.width as f64 / scale_factor).max(1.0);
        let logical_height = (screen_size.height as f64 / scale_factor).max(1.0);
        let x = screen_pos.x as f64 / scale_factor;
        let y = screen_pos.y as f64 / scale_factor;
        (logical_width, logical_height, x, y)
    } else {
        log::warn!("未获取到显示器信息，字幕窗口使用默认布局");
        (
            DEFAULT_SUBTITLE_WINDOW_WIDTH,
            DEFAULT_SUBTITLE_WINDOW_HEIGHT,
            0.0,
            0.0,
        )
    }
}

fn apply_subtitle_layout(
    app_handle: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), AppError> {
    let (logical_width, logical_height, x, y) = resolve_subtitle_layout(app_handle);
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            logical_width,
            logical_height,
        )))
        .map_err(|e| tauri_error("设置字幕窗口尺寸失败", e))?;
    window
        .set_position(tauri::Position::Logical(tauri::LogicalPosition::new(x, y)))
        .map_err(|e| tauri_error("设置字幕窗口位置失败", e))?;
    Ok(())
}

pub(crate) fn set_subtitle_window_interactive(
    app_handle: &tauri::AppHandle,
    interactive: bool,
) -> Result<(), AppError> {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        window
            .set_ignore_cursor_events(!interactive)
            .map_err(|e| tauri_error("设置字幕窗口交互状态失败", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_main_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    let window = require_window(&app_handle, "main", "主窗口不存在")?;
    window
        .hide()
        .map_err(|e| tauri_error("隐藏主窗口失败", e))?;
    Ok("主窗口已隐藏".to_string())
}

fn create_subtitle_window_unlocked(app_handle: &tauri::AppHandle) -> Result<String, AppError> {
    if app_handle.get_webview_window("subtitle").is_some() {
        return Ok("字幕窗口已存在".to_string());
    }

    let (logical_width, logical_height, x, y) = resolve_subtitle_layout(app_handle);

    let window = tauri::WebviewWindowBuilder::new(
        app_handle,
        "subtitle",
        tauri::WebviewUrl::App("/?window=subtitle".into()),
    )
    .title("字幕")
    .inner_size(logical_width, logical_height)
    .position(x, y)
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .resizable(false)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|e| tauri_error("创建字幕窗口失败", e))?;

    if let Err(err) = window.set_ignore_cursor_events(true) {
        log::warn!("设置字幕窗口鼠标穿透失败，继续运行: {}", err);
    }

    Ok("字幕窗口已创建".to_string())
}

pub async fn create_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    let state = app_handle.state::<AppState>();
    let _window_op = state.recording.subtitle_window_op.lock().await;
    create_subtitle_window_unlocked(&app_handle)
}

pub(crate) fn reserve_subtitle_show_generation(app_handle: &tauri::AppHandle) -> u64 {
    app_handle
        .state::<AppState>()
        .recording
        .subtitle_show_gen
        .fetch_add(1, Ordering::AcqRel)
        + 1
}

fn session_show_is_current(app_handle: &tauri::AppHandle, session_id: u64, show_gen: u64) -> bool {
    let state = app_handle.state::<AppState>();
    let slot = state
        .recording
        .recording
        .lock()
        .as_ref()
        .map(|slot| (slot.session_id(), slot.subtitle_show_gen()));
    show_guard_matches(
        state.recording.subtitle_show_gen.load(Ordering::Acquire),
        slot,
        session_id,
        show_gen,
    )
}

fn show_guard_matches(
    current_gen: u64,
    slot: Option<(u64, u64)>,
    requested_session_id: u64,
    requested_gen: u64,
) -> bool {
    current_gen == requested_gen && slot == Some((requested_session_id, requested_gen))
}

fn hide_guard_matches(
    current_gen: u64,
    current_session_id: u64,
    recording_active: bool,
    requested_session_id: u64,
    requested_gen: u64,
) -> bool {
    current_gen == requested_gen && current_session_id == requested_session_id && !recording_active
}

fn stale_session_show_result(session_id: u64, show_gen: u64) -> String {
    log::debug!(
        "忽略过期字幕显示请求 (session {}, generation {})",
        session_id,
        show_gen
    );
    "字幕窗口显示请求已过期".to_string()
}

fn show_subtitle_window_unlocked(
    app_handle: &tauri::AppHandle,
    session: Option<(u64, u64)>,
) -> Result<String, AppError> {
    if let Some((session_id, show_gen)) = session {
        if !session_show_is_current(app_handle, session_id, show_gen) {
            return Ok(stale_session_show_result(session_id, show_gen));
        }
    }

    if app_handle.get_webview_window("subtitle").is_none() {
        create_subtitle_window_unlocked(app_handle)?;
    }

    if let Some((session_id, show_gen)) = session {
        if !session_show_is_current(app_handle, session_id, show_gen) {
            return Ok(stale_session_show_result(session_id, show_gen));
        }
    }

    let window = require_window(app_handle, "subtitle", "字幕窗口创建后仍不存在")?;
    if let Err(err) = apply_subtitle_layout(app_handle, &window) {
        log::warn!("刷新字幕窗口布局失败，继续尝试显示: {}", err);
    }

    if let Some((session_id, show_gen)) = session {
        // Keep the recording slot locked through the synchronous OS show call.
        // A quick cancel therefore either wins before this check (no show) or
        // waits until the window is visible and then schedules a guarded hide.
        let state = app_handle.state::<AppState>();
        if state.recording.subtitle_show_gen.load(Ordering::Acquire) != show_gen {
            return Ok(stale_session_show_result(session_id, show_gen));
        }
        let recording = state.recording.recording.lock();
        if !recording.as_ref().is_some_and(|slot| {
            slot.session_id() == session_id && slot.subtitle_show_gen() == show_gen
        }) {
            return Ok(stale_session_show_result(session_id, show_gen));
        }
        window
            .show()
            .map_err(|e| tauri_error("显示字幕窗口失败", e))?;
    } else {
        window
            .show()
            .map_err(|e| tauri_error("显示字幕窗口失败", e))?;
    }

    // 确保窗口在最顶层（Windows 上 hide/show 后可能丢失置顶状态）
    // 先用 Tauri API 置顶
    let _ = window.set_always_on_top(false);
    if let Err(err) = window.set_always_on_top(true) {
        log::warn!("设置字幕窗口置顶失败: {}", err);
    }
    // 再通过 Windows API 强制置顶，避免被其他窗口遮挡
    #[cfg(target_os = "windows")]
    force_window_topmost(&window);

    if let Err(err) = set_subtitle_window_interactive(app_handle, false) {
        log::warn!("重新设置字幕窗口鼠标穿透失败: {}", err);
    }

    Ok("字幕窗口已显示".to_string())
}

#[tauri::command]
pub async fn show_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    reserve_subtitle_show_generation(&app_handle);
    let state = app_handle.state::<AppState>();
    let _window_op = state.recording.subtitle_window_op.lock().await;
    show_subtitle_window_unlocked(&app_handle, None)
}

pub(crate) async fn show_subtitle_window_for_session(
    app_handle: tauri::AppHandle,
    session_id: u64,
    show_gen: u64,
) -> Result<String, AppError> {
    let state = app_handle.state::<AppState>();
    let _window_op = state.recording.subtitle_window_op.lock().await;
    show_subtitle_window_unlocked(&app_handle, Some((session_id, show_gen)))
}

pub(crate) async fn set_subtitle_window_interactive_for_session(
    app_handle: &tauri::AppHandle,
    session_id: u64,
    show_gen: u64,
    interactive: bool,
) -> Result<bool, AppError> {
    let state = app_handle.state::<AppState>();
    let _window_op = state.recording.subtitle_window_op.lock().await;
    let recording = state.recording.recording.lock();
    if state.recording.subtitle_show_gen.load(Ordering::Acquire) != show_gen
        || state.recording.session_counter.load(Ordering::Acquire) != session_id
        || recording.is_some()
    {
        return Ok(false);
    }
    set_subtitle_window_interactive(app_handle, interactive)?;
    Ok(true)
}

#[tauri::command]
pub async fn hide_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    let state = app_handle.state::<AppState>();
    let _window_op = state.recording.subtitle_window_op.lock().await;
    hide_subtitle_window_inner(&app_handle)
}

pub fn hide_subtitle_window_inner(app_handle: &tauri::AppHandle) -> Result<String, AppError> {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        let _ = set_subtitle_window_interactive(app_handle, false);
        window
            .hide()
            .map_err(|e| tauri_error("隐藏字幕窗口失败", e))?;
        Ok("字幕窗口已隐藏".to_string())
    } else {
        Ok("字幕窗口不存在".to_string())
    }
}

pub(crate) fn schedule_subtitle_hide(
    app_handle: &tauri::AppHandle,
    session_id: u64,
    show_gen: u64,
    mode: RecordingMode,
    delay_ms: u64,
) {
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

        let state = app.state::<AppState>();
        let _window_op = state.recording.subtitle_window_op.lock().await;
        let recording = state.recording.recording.lock();
        if !hide_guard_matches(
            state.recording.subtitle_show_gen.load(Ordering::Acquire),
            state.recording.session_counter.load(Ordering::Acquire),
            recording.is_some(),
            session_id,
            show_gen,
        ) {
            return;
        }

        let idle = state.recording.transition_snapshot_while_recording_locked(
            session_id,
            RecordingPhase::Idle,
            mode,
            None,
            None,
        );
        let hide_result = hide_subtitle_window_inner(&app);
        if hide_result.is_ok() {
            state
                .recording
                .clear_snapshot_while_recording_locked(session_id);
        }
        drop(recording);

        if let Some(idle) = idle {
            let _ = app.emit(
                "recording-state",
                serde_json::json!({
                    "sessionId": idle.session_id,
                    "revision": idle.revision,
                    "phase": idle.phase,
                    "isStarting": false,
                    "isRecording": false,
                    "isProcessing": false,
                    "mode": idle.mode,
                }),
            );
        }
        if let Err(err) = hide_result {
            log::warn!("隐藏字幕窗口失败: {}", err);
        }
    });
}

#[cfg(test)]
mod recording_window_guard_tests {
    use super::{hide_guard_matches, show_guard_matches};

    #[test]
    fn stale_show_cannot_overtake_a_new_session() {
        assert!(!show_guard_matches(2, Some((2, 2)), 1, 1));
        assert!(show_guard_matches(2, Some((2, 2)), 2, 2));
    }

    #[test]
    fn old_hide_cannot_hide_a_new_or_active_session() {
        assert!(!hide_guard_matches(2, 2, true, 1, 1));
        assert!(!hide_guard_matches(2, 2, false, 1, 1));
        assert!(!hide_guard_matches(2, 2, true, 2, 2));
        assert!(hide_guard_matches(2, 2, false, 2, 2));
    }
}
