//! 剪贴板命令模块
//!
//! 提供文本复制和粘贴功能。
//! 复制功能使用 tauri-plugin-clipboard-manager 插件，
//! 粘贴功能通过模拟键盘快捷键实现。
//!
//! # 为什么需要模拟粘贴？
//! 在某些场景下，用户希望转写结果能直接"打"到当前活动的输入框中。
//! 实现方式是：先把文本写入剪贴板，然后模拟按下 Ctrl+V（或 Cmd+V）。

use crate::utils::AppError;

/// 复制文本到系统剪贴板
///
/// # 参数
/// - `text`：要复制的文本内容
///
/// # 实现方式
/// 通过 `tauri-plugin-clipboard-manager` 插件的 API 写入剪贴板。
///
/// # 前端调用示例
/// ```javascript
/// await invoke('copy_to_clipboard', { text: '要复制的内容' });
/// ```
#[tauri::command]
pub async fn copy_to_clipboard(
    app_handle: tauri::AppHandle,
    text: String,
) -> Result<String, AppError> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    // 使用 Tauri 剪贴板插件写入文本
    app_handle
        .clipboard()
        .write_text(&text)
        .map_err(|e| AppError::Other(format!("写入剪贴板失败: {}", e)))?;

    log::info!("已复制 {} 个字符到剪贴板", text.len());
    Ok("已复制到剪贴板".to_string())
}

/// 粘贴文本（写入剪贴板并模拟 Ctrl+V）
///
/// 这个功能的使用场景是：用户完成语音转写后，
/// 希望直接把结果输入到当前焦点所在的文本框中。
///
/// # 实现步骤
/// 1. 先将文本写入系统剪贴板
/// 2. 等待一小段时间确保剪贴板更新
/// 3. 使用平台特定的方式模拟 Ctrl+V 快捷键
///
/// # 平台差异
/// - Windows：使用 PowerShell 调用 SendKeys
/// - macOS：使用 osascript 模拟 Cmd+V
/// - Linux：使用 xdotool 模拟按键
///
/// # 注意事项
/// 模拟按键可能被某些安全软件拦截。
#[tauri::command]
pub async fn paste_text(
    app_handle: tauri::AppHandle,
    text: String,
) -> Result<String, AppError> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    // 第一步：写入剪贴板
    app_handle
        .clipboard()
        .write_text(&text)
        .map_err(|e| AppError::Other(format!("写入剪贴板失败: {}", e)))?;

    // 第二步：等待剪贴板更新（缩短等待时间以降低粘贴延迟）
    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    // 第三步：模拟粘贴快捷键
    //
    // `cfg!(target_os = "xxx")` 是编译时条件判断，
    // 只有对应平台的代码会被编译进最终程序。
    //
    // 但这里我们用运行时判断（因为代码量不大）：
    #[cfg(target_os = "windows")]
    {
        // Windows：直接调用 Win32 API 模拟 Ctrl+V（避免启动 PowerShell）
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            keybd_event, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
        };

        unsafe {
            keybd_event(VK_CONTROL as u8, 0, 0, 0);
            keybd_event(VK_V as u8, 0, 0, 0);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        unsafe {
            keybd_event(VK_V as u8, 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS：使用 AppleScript 模拟 Cmd+V
        let _ = tokio::process::Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to keystroke \"v\" using command down",
            ])
            .output()
            .await;
    }

    #[cfg(target_os = "linux")]
    {
        // Linux：使用 xdotool 模拟 Ctrl+V
        let _ = tokio::process::Command::new("xdotool")
            .args(["key", "ctrl+v"])
            .output()
            .await;
    }

    log::info!("已粘贴 {} 个字符", text.len());
    Ok("已粘贴".to_string())
}
