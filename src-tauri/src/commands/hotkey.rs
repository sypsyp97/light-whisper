//! 全局快捷键命令模块
//!
//! 管理应用的全局快捷键。
//! 默认使用 F2 键作为录音的开始/停止快捷键。
//!
//! # 实现方式
//! 使用 `tauri-plugin-global-shortcut` 插件注册全局快捷键。
//! 全局快捷键即使应用不在前台也能响应。
//!
//! # 注意事项
//! - 全局快捷键可能与其他应用冲突
//! - 某些系统快捷键（如 Win+L）无法覆盖

use crate::utils::AppError;
use tauri::Emitter;

/// 注册 F2 全局快捷键
///
/// 按下 F2 键时，会向前端发送 `toggle-recording` 事件。
/// 前端监听这个事件来切换录音状态。
///
/// # Rust 知识点：闭包（Closure）
/// `|app, shortcut, event| { ... }` 是一个闭包，类似于 JavaScript 的箭头函数。
/// 闭包可以捕获外部变量（这里的 `app`）。
///
/// `move` 关键字表示闭包获取捕获变量的所有权（而不是引用）。
/// 在多线程场景下，`move` 闭包确保数据的安全传递。
///
/// # 前端监听示例
/// ```javascript
/// import { listen } from '@tauri-apps/api/event';
/// listen('toggle-recording', () => {
///     // 切换录音状态
///     toggleRecording();
/// });
/// ```
#[tauri::command]
pub async fn register_f2_hotkey(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // 解析快捷键字符串
    // "F2" 会被解析为 F2 功能键
    let shortcut = "F2";

    // 先尝试清理旧的注册（忽略错误）
    let _ = app_handle.global_shortcut().unregister(shortcut);

    // 注册全局快捷键
    //
    // `on_shortcut` 方法注册一个快捷键及其回调函数。
    // 当用户按下指定快捷键时，回调函数会被调用。
    app_handle
        .global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            match event.state {
                tauri_plugin_global_shortcut::ShortcutState::Pressed => {
                    log::info!("F2 按下，开始录音");
                    let _ = app.emit("hotkey-press", ());
                }
                tauri_plugin_global_shortcut::ShortcutState::Released => {
                    log::info!("F2 松开，停止录音");
                    let _ = app.emit("hotkey-release", ());
                }
            }
        })
        .map_err(|e| AppError::Other(format!("注册 F2 快捷键失败: {}", e)))?;

    log::info!("F2 全局快捷键已注册");
    Ok("F2 快捷键已注册".to_string())
}

/// 注销 F2 全局快捷键
///
/// 取消 F2 键的全局快捷键绑定。
/// 通常在应用退出前或用户更改快捷键时调用。
#[tauri::command]
pub async fn unregister_f2_hotkey(
    app_handle: tauri::AppHandle,
) -> Result<String, AppError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let shortcut = "F2";

    app_handle
        .global_shortcut()
        .unregister(shortcut)
        .map_err(|e| AppError::Other(format!("注销 F2 快捷键失败: {}", e)))?;

    log::info!("F2 全局快捷键已注销");
    Ok("F2 快捷键已注销".to_string())
}

/// 注册自定义快捷键
///
/// 允许用户自定义快捷键来触发录音。
///
/// # 参数
/// - `shortcut`：快捷键字符串（如 "F2"、"Ctrl+Shift+R"）
///
/// # 支持的快捷键格式
/// - 单键：`F1` ~ `F12`
/// - 组合键：`Ctrl+R`、`Alt+S`、`Ctrl+Shift+R`
/// - 修饰键：`Ctrl`、`Alt`、`Shift`
#[tauri::command]
pub async fn register_custom_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // 先尝试注销已有的快捷键（忽略错误）
    let _ = app_handle.global_shortcut().unregister_all();

    // 注册新的快捷键
    app_handle
        .global_shortcut()
        .on_shortcut(shortcut.as_str(), move |app, _shortcut, event| {
            match event.state {
                tauri_plugin_global_shortcut::ShortcutState::Pressed => {
                    log::info!("自定义快捷键按下，开始录音");
                    let _ = app.emit("hotkey-press", ());
                }
                tauri_plugin_global_shortcut::ShortcutState::Released => {
                    log::info!("自定义快捷键松开，停止录音");
                    let _ = app.emit("hotkey-release", ());
                }
            }
        })
        .map_err(|e| {
            AppError::Other(format!(
                "注册快捷键 {} 失败: {}。请检查快捷键格式是否正确。",
                shortcut, e
            ))
        })?;

    log::info!("自定义快捷键 {} 已注册", shortcut);
    Ok(format!("快捷键 {} 已注册", shortcut))
}
