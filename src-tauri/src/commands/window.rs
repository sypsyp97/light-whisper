//! 窗口管理命令模块
//!
//! 管理主窗口和字幕叠加窗口的生命周期。

use crate::utils::AppError;
use tauri::Manager;

/// 隐藏主窗口
///
/// 把主窗口隐藏到系统托盘。
/// 用户可以通过托盘图标重新显示窗口。
#[tauri::command]
pub async fn hide_main_window(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    if let Some(window) = app_handle.get_webview_window("main") {
        window
            .hide()
            .map_err(|e| AppError::Tauri(format!("隐藏主窗口失败: {}", e)))?;

        Ok("主窗口已隐藏".to_string())
    } else {
        Err(AppError::Tauri("主窗口不存在".to_string()))
    }
}

/// 显示并聚焦主窗口
#[tauri::command]
pub async fn show_main_window(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    if let Some(window) = app_handle.get_webview_window("main") {
        window
            .show()
            .map_err(|e| AppError::Tauri(format!("显示主窗口失败: {}", e)))?;
        window
            .set_focus()
            .map_err(|e| AppError::Tauri(format!("聚焦主窗口失败: {}", e)))?;
        Ok("主窗口已显示".to_string())
    } else {
        Err(AppError::Tauri("主窗口不存在".to_string()))
    }
}

/// 创建字幕叠加窗口（隐藏状态）
///
/// 在屏幕底部居中位置创建一个透明的、鼠标穿透的字幕窗口。
/// 窗口创建后默认隐藏，通过 `show_subtitle_window` 显示。
#[tauri::command]
pub async fn create_subtitle_window(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    // 防重复创建：如果窗口已存在直接返回
    if app_handle.get_webview_window("subtitle").is_some() {
        return Ok("字幕窗口已存在".to_string());
    }

    // 获取主显示器尺寸以计算位置
    let monitor = app_handle
        .primary_monitor()
        .map_err(|e| AppError::Tauri(format!("获取显示器信息失败: {}", e)))?
        .ok_or_else(|| AppError::Tauri("未找到主显示器".to_string()))?;

    let screen_size = monitor.size();
    let scale_factor = monitor.scale_factor();

    // position() 和 inner_size() 使用逻辑像素，monitor.size() 返回物理像素
    let logical_width = screen_size.width as f64 / scale_factor;
    let logical_height = screen_size.height as f64 / scale_factor;

    // 窗口撑满屏幕宽度，让 CSS 内部居中，避免文字被截断
    let window_width = logical_width;
    let window_height: f64 = 64.0;
    let bottom_offset: f64 = 60.0;

    let x = 0.0;
    let y = logical_height - window_height - bottom_offset;

    let subtitle_url = tauri::WebviewUrl::App("/?window=subtitle".into());

    let window = tauri::WebviewWindowBuilder::new(
        &app_handle,
        "subtitle",
        subtitle_url,
    )
    .title("字幕")
    .inner_size(window_width, window_height)
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
    .map_err(|e| AppError::Tauri(format!("创建字幕窗口失败: {}", e)))?;

    // 设置鼠标穿透，让点击事件透过字幕窗口
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| AppError::Tauri(format!("设置鼠标穿透失败: {}", e)))?;

    Ok("字幕窗口已创建".to_string())
}

/// 显示字幕窗口
///
/// 如果窗口不存在则先创建再显示。
#[tauri::command]
pub async fn show_subtitle_window(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    // 如果窗口不存在，先创建
    if app_handle.get_webview_window("subtitle").is_none() {
        create_subtitle_window(app_handle.clone()).await?;
    }

    if let Some(window) = app_handle.get_webview_window("subtitle") {
        window
            .show()
            .map_err(|e| AppError::Tauri(format!("显示字幕窗口失败: {}", e)))?;
        Ok("字幕窗口已显示".to_string())
    } else {
        Err(AppError::Tauri("字幕窗口创建后仍不存在".to_string()))
    }
}

/// 隐藏字幕窗口（不销毁，下次可立即显示）
#[tauri::command]
pub async fn hide_subtitle_window(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        window
            .hide()
            .map_err(|e| AppError::Tauri(format!("隐藏字幕窗口失败: {}", e)))?;
        Ok("字幕窗口已隐藏".to_string())
    } else {
        Ok("字幕窗口不存在".to_string())
    }
}

/// 关闭并销毁字幕窗口
#[tauri::command]
pub async fn destroy_subtitle_window(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        window
            .destroy()
            .map_err(|e| AppError::Tauri(format!("销毁字幕窗口失败: {}", e)))?;
        Ok("字幕窗口已销毁".to_string())
    } else {
        Ok("字幕窗口不存在，无需销毁".to_string())
    }
}
