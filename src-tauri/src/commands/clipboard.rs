use crate::utils::AppError;

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

#[tauri::command]
pub async fn paste_text(
    app_handle: tauri::AppHandle,
    text: String,
    method: Option<String>,
) -> Result<String, AppError> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE,
        };

        fn make_key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        }

        fn send_inputs(inputs: &[INPUT]) -> Result<(), AppError> {
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
            Ok(())
        }

        let use_clipboard = method.as_deref() == Some("clipboard");

        if use_clipboard {
            use tauri_plugin_clipboard_manager::ClipboardExt;

            app_handle
                .clipboard()
                .write_text(&text)
                .map_err(|e| AppError::Other(format!("写入剪贴板失败: {}", e)))?;

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            const VK_CONTROL: u16 = 0x11;
            const VK_V: u16 = 0x56;

            let inputs = [
                make_key_input(VK_CONTROL, 0, 0),
                make_key_input(VK_V, 0, 0),
                make_key_input(VK_V, 0, KEYEVENTF_KEYUP),
                make_key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
            ];
            send_inputs(&inputs)?;
        } else {
            let mut inputs: Vec<INPUT> = Vec::new();

            for code_unit in text.encode_utf16() {
                inputs.push(make_key_input(0, code_unit, KEYEVENTF_UNICODE));
                inputs.push(make_key_input(
                    0,
                    code_unit,
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                ));
            }

            if !inputs.is_empty() {
                send_inputs(&inputs)?;
            }
        }
    }

    log::info!("已输入 {} 个字符", text.len());
    Ok("已输入".to_string())
}
