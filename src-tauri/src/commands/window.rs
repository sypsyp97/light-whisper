use std::sync::atomic::Ordering;

use crate::state::AppState;
use crate::utils::AppError;
use tauri::Manager;

const SUBTITLE_WINDOW_HEIGHT: f64 = 64.0;
const SUBTITLE_WINDOW_BOTTOM_MARGIN: f64 = 60.0;
const DEFAULT_SUBTITLE_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_SUBTITLE_WINDOW_X: f64 = 0.0;
const DEFAULT_SUBTITLE_WINDOW_Y: f64 = 596.0;

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

fn resolve_subtitle_layout(app_handle: &tauri::AppHandle) -> (f64, f64, f64, f64) {
    let monitor = app_handle
        .get_webview_window("main")
        .and_then(|window| window.current_monitor().ok().flatten())
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
        let logical_height =
            (screen_size.height as f64 / scale_factor).max(SUBTITLE_WINDOW_HEIGHT);
        let x = screen_pos.x as f64 / scale_factor;
        let y_origin = screen_pos.y as f64 / scale_factor;
        let y = y_origin
            + (logical_height - SUBTITLE_WINDOW_HEIGHT - SUBTITLE_WINDOW_BOTTOM_MARGIN).max(0.0);
        (logical_width, SUBTITLE_WINDOW_HEIGHT, x, y)
    } else {
        log::warn!("未获取到显示器信息，字幕窗口使用默认布局");
        (
            DEFAULT_SUBTITLE_WINDOW_WIDTH,
            SUBTITLE_WINDOW_HEIGHT,
            DEFAULT_SUBTITLE_WINDOW_X,
            DEFAULT_SUBTITLE_WINDOW_Y,
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

    let (logical_width, logical_height, x, y) = resolve_subtitle_layout(&app_handle);

    let window = tauri::WebviewWindowBuilder::new(
        &app_handle,
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

#[tauri::command]
pub async fn show_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    if app_handle.get_webview_window("subtitle").is_none() {
        create_subtitle_window(app_handle.clone()).await?;
    }

    let window = require_window(&app_handle, "subtitle", "字幕窗口创建后仍不存在")?;
    if let Err(err) = apply_subtitle_layout(&app_handle, &window) {
        log::warn!("刷新字幕窗口布局失败，继续尝试显示: {}", err);
    }

    window
        .show()
        .map_err(|e| tauri_error("显示字幕窗口失败", e))?;

    // 确保窗口在最顶层（Windows 上 hide/show 后可能丢失置顶状态）
    if let Err(err) = window.set_always_on_top(true) {
        log::warn!("设置字幕窗口置顶失败: {}", err);
    }

    if let Err(err) = window.set_ignore_cursor_events(true) {
        log::warn!("重新设置字幕窗口鼠标穿透失败: {}", err);
    }

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
