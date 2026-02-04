//! 窗口管理命令模块
//!
//! 目前仅保留对主窗口的隐藏操作。

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
