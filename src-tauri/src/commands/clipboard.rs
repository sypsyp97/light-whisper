use crate::utils::AppError;

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
};

#[cfg(target_os = "windows")]
#[link(name = "imm32")]
extern "system" {
    fn ImmGetDefaultIMEWnd(
        hwnd: windows_sys::Win32::Foundation::HWND,
    ) -> windows_sys::Win32::Foundation::HWND;
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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

/// 释放所有可能残留的修饰键，防止后续 SendInput 被 OS 解读为组合键
#[cfg(target_os = "windows")]
fn release_stuck_modifiers() -> Result<(), AppError> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    const MODIFIER_VKS: [u16; 8] = [
        0x5B, 0x5C, // VK_LWIN, VK_RWIN
        0xA4, 0xA5, // VK_LMENU, VK_RMENU
        0xA0, 0xA1, // VK_LSHIFT, VK_RSHIFT
        0xA2, 0xA3, // VK_LCONTROL, VK_RCONTROL
    ];

    let release: Vec<INPUT> = MODIFIER_VKS
        .iter()
        .filter(|&&vk| unsafe { GetAsyncKeyState(vk as i32) } < 0)
        .map(|&vk| make_key_input(vk, 0, KEYEVENTF_KEYUP))
        .collect();

    if !release.is_empty() {
        log::debug!("释放 {} 个残留修饰键", release.len());
        send_inputs(&release)?;
    }
    Ok(())
}

/// 通过 UIA TextPattern 读取前台焦点控件的选中文本。零副作用。
pub fn grab_selected_text() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        grab_selected_text_uia()
    }

    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn grab_selected_text_uia() -> Option<String> {
    use uiautomation::UIAutomation;

    let automation = UIAutomation::new().ok()?;
    let focused = automation.get_focused_element().ok()?;

    if let Some(text) = try_get_selection(&focused) {
        return Some(text);
    }

    let walker = automation.create_tree_walker().ok()?;
    let mut current = focused;
    for _ in 0..5 {
        current = match walker.get_parent(&current) {
            Ok(parent) => parent,
            Err(_) => break,
        };
        if let Some(text) = try_get_selection(&current) {
            return Some(text);
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn try_get_selection(element: &uiautomation::UIElement) -> Option<String> {
    use uiautomation::patterns::UITextPattern;

    let pattern: UITextPattern = element.get_pattern().ok()?;
    let ranges = pattern.get_selection().ok()?;

    let mut combined = String::new();
    for range in &ranges {
        if let Ok(text) = range.get_text(-1) {
            combined.push_str(&text);
        }
    }

    let trimmed = combined.trim();
    if trimmed.is_empty() {
        None
    } else {
        log::info!("UIA 检测到选中文本（{} 字符）", trimmed.len());
        Some(trimmed.to_string())
    }
}

#[tauri::command]
pub async fn copy_to_clipboard(
    app_handle: tauri::AppHandle,
    text: String,
) -> Result<String, AppError> {
    write_text_to_clipboard(&app_handle, &text)?;
    log::info!("已复制 {} 个字符到剪贴板", text.len());
    Ok("已复制到剪贴板".to_string())
}

pub fn write_text_to_clipboard(app_handle: &tauri::AppHandle, text: &str) -> Result<(), AppError> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    app_handle
        .clipboard()
        .write_text(text)
        .map_err(|e| AppError::Other(format!("写入剪贴板失败: {}", e)))
}

#[tauri::command]
pub async fn paste_text(
    app_handle: tauri::AppHandle,
    text: String,
    method: Option<String>,
) -> Result<String, AppError> {
    let method_str = method.as_deref().unwrap_or("sendInput");
    paste_text_impl(&app_handle, &text, method_str).await
}

pub async fn paste_text_impl(
    app_handle: &tauri::AppHandle,
    text: &str,
    method: &str,
) -> Result<String, AppError> {
    #[cfg(target_os = "windows")]
    {
        let use_clipboard = method == "clipboard";

        if use_clipboard {
            use tauri_plugin_clipboard_manager::ClipboardExt;

            app_handle
                .clipboard()
                .write_text(text)
                .map_err(|e| AppError::Other(format!("写入剪贴板失败: {}", e)))?;

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            release_stuck_modifiers()?;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;

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
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SendMessageW};

            const VK_RETURN: u16 = 0x0D;
            const VK_TAB: u16 = 0x09;
            const WM_IME_CONTROL: u32 = 0x0283;
            const IMC_GETOPENSTATUS: usize = 0x0005;
            const IMC_SETOPENSTATUS: usize = 0x0006;

            // ① 释放残留修饰键
            release_stuck_modifiers()?;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;

            // ② 临时关闭前台窗口的输入法，防止 Unicode 输入被 IME 拦截
            let hwnd = unsafe { GetForegroundWindow() };
            let ime_wnd_ptr = unsafe { ImmGetDefaultIMEWnd(hwnd) };
            // 将 *mut c_void 转为 usize 以跨越 await（HWND 本质是个数值句柄）
            let ime_wnd = ime_wnd_ptr as usize;
            let ime_was_open = if ime_wnd != 0 {
                let open =
                    unsafe { SendMessageW(ime_wnd as _, WM_IME_CONTROL, IMC_GETOPENSTATUS, 0) };
                if open != 0 {
                    unsafe {
                        SendMessageW(ime_wnd as _, WM_IME_CONTROL, IMC_SETOPENSTATUS, 0);
                    }
                    log::debug!("已临时关闭前台窗口输入法");
                    true
                } else {
                    false
                }
            } else {
                false
            };

            // ③ 构建并发送 Unicode 输入事件
            let mut inputs: Vec<INPUT> = Vec::new();
            for ch in text.chars() {
                match ch {
                    '\r' => {}
                    '\n' => {
                        inputs.push(make_key_input(VK_RETURN, 0, 0));
                        inputs.push(make_key_input(VK_RETURN, 0, KEYEVENTF_KEYUP));
                    }
                    '\t' => {
                        inputs.push(make_key_input(VK_TAB, 0, 0));
                        inputs.push(make_key_input(VK_TAB, 0, KEYEVENTF_KEYUP));
                    }
                    _ => {
                        for code_unit in ch.encode_utf16(&mut [0; 2]) {
                            inputs.push(make_key_input(0, *code_unit, KEYEVENTF_UNICODE));
                            inputs.push(make_key_input(
                                0,
                                *code_unit,
                                KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                            ));
                        }
                    }
                }
            }
            let send_result = if !inputs.is_empty() {
                send_inputs(&inputs)
            } else {
                Ok(())
            };

            // ④ 无论发送成功与否都必须恢复输入法，否则用户 IME 会卡在关闭状态
            if ime_was_open {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                unsafe {
                    SendMessageW(ime_wnd as _, WM_IME_CONTROL, IMC_SETOPENSTATUS, 1);
                }
                log::debug!("已恢复前台窗口输入法");
            }

            send_result?;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app_handle;
        let _ = text;
        let _ = method;
        return Err(AppError::Other(
            "当前平台暂不支持自动输入，仅 Windows 可用".to_string(),
        ));
    }

    log::info!("已输入 {} 个字符", text.len());
    Ok("已输入".to_string())
}
