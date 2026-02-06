//! 剪贴板与文本输入命令模块
//!
//! 提供文本复制和直接输入功能。
//! - 复制功能：使用 tauri-plugin-clipboard-manager 插件写入剪贴板
//! - 输入功能：通过平台原生 API 直接模拟键盘输入 Unicode 字符，不占用剪贴板

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

/// 输入文本到当前活动窗口（不覆盖剪贴板）
///
/// 通过模拟键盘输入将文本直接打到当前焦点所在的文本框中，
/// 不会覆盖用户的剪贴板内容。
///
/// # 平台实现
/// - Windows：使用 Win32 SendInput API 发送 Unicode 字符
/// - macOS：使用 osascript keystroke 模拟按键输入
/// - Linux：使用 xdotool type 模拟键盘输入
///
/// # 注意事项
/// 模拟输入可能被某些安全软件拦截。
#[tauri::command]
pub async fn paste_text(text: String) -> Result<String, AppError> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
            KEYEVENTF_UNICODE, KEYEVENTF_KEYUP,
        };

        let mut inputs: Vec<INPUT> = Vec::new();

        for code_unit in text.encode_utf16() {
            // Key down
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: code_unit,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            // Key up
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: code_unit,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        if !inputs.is_empty() {
            // SAFETY: SendInput is a well-documented Win32 API for synthesizing input.
            // We pass a correctly-sized array of INPUT structs with valid KEYBDINPUT data.
            let sent = unsafe {
                SendInput(
                    inputs.len() as u32,
                    inputs.as_ptr(),
                    std::mem::size_of::<INPUT>() as i32,
                )
            };
            if sent == 0 {
                return Err(AppError::Other("SendInput 调用失败".to_string()));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS：使用 AppleScript keystroke 直接输入文本（不经过剪贴板）
        let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "tell application \"System Events\" to keystroke \"{}\"",
            escaped
        );
        let _ = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await;
    }

    #[cfg(target_os = "linux")]
    {
        // Linux：使用 xdotool type 直接输入文本（不经过剪贴板）
        let _ = tokio::process::Command::new("xdotool")
            .args(["type", "--clearmodifiers", "--delay", "0", &text])
            .output()
            .await;
    }

    log::info!("已输入 {} 个字符", text.len());
    Ok("已输入".to_string())
}
