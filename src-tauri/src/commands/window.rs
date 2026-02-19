use std::sync::atomic::Ordering;

use crate::state::AppState;
use crate::utils::AppError;
use tauri::Manager;

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

#[tauri::command]
pub async fn hide_main_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    let window = require_window(&app_handle, "main", "主窗口不存在")?;
    window
        .hide()
        .map_err(|e| tauri_error("隐藏主窗口失败", e))?;
    Ok("主窗口已隐藏".to_string())
}

pub async fn create_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    if app_handle.get_webview_window("subtitle").is_some() {
        return Ok("字幕窗口已存在".to_string());
    }

    let monitor = app_handle
        .primary_monitor()
        .map_err(|e| tauri_error("获取显示器信息失败", e))?
        .ok_or_else(|| AppError::Tauri("未找到主显示器".to_string()))?;

    let screen_size = monitor.size();
    let scale_factor = monitor.scale_factor();
    let logical_width = screen_size.width as f64 / scale_factor;
    let logical_height = screen_size.height as f64 / scale_factor;
    let window_height = 64.0_f64;
    let y = logical_height - window_height - 60.0_f64;

    let window = tauri::WebviewWindowBuilder::new(
        &app_handle,
        "subtitle",
        tauri::WebviewUrl::App("/?window=subtitle".into()),
    )
    .title("字幕")
    .inner_size(logical_width, window_height)
    .position(0.0, y)
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

    window
        .set_ignore_cursor_events(true)
        .map_err(|e| tauri_error("设置鼠标穿透失败", e))?;

    Ok("字幕窗口已创建".to_string())
}

#[tauri::command]
pub async fn show_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    if app_handle.get_webview_window("subtitle").is_none() {
        create_subtitle_window(app_handle.clone()).await?;
    }

    let window = require_window(&app_handle, "subtitle", "字幕窗口创建后仍不存在")?;
    window
        .show()
        .map_err(|e| tauri_error("显示字幕窗口失败", e))?;

    // 确保窗口在最顶层（Windows 上 hide/show 后可能丢失置顶状态）
    let _ = window.set_always_on_top(true);

    // 递增"显示代"，使之前排队的 schedule_hide 全部作废
    let state = app_handle.state::<AppState>();
    state.subtitle_show_gen.fetch_add(1, Ordering::Relaxed);

    Ok("字幕窗口已显示".to_string())
}

#[tauri::command]
pub async fn hide_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    hide_subtitle_window_inner(&app_handle)
}

pub fn hide_subtitle_window_inner(app_handle: &tauri::AppHandle) -> Result<String, AppError> {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        window
            .hide()
            .map_err(|e| tauri_error("隐藏字幕窗口失败", e))?;
        Ok("字幕窗口已隐藏".to_string())
    } else {
        Ok("字幕窗口不存在".to_string())
    }
}
