use crate::utils::AppError;

#[cfg(target_os = "macos")]
use core_foundation::{
    base::{CFRelease, CFTypeRef, TCFType},
    string::CFString,
};
#[cfg(target_os = "macos")]
use core_foundation_sys::{
    base::{Boolean, CFGetTypeID, CFRange},
    string::{CFStringGetTypeID, CFStringRef},
};
#[cfg(target_os = "macos")]
use std::{ffi::c_void, ptr};

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

#[cfg(target_os = "macos")]
type AXUIElementRef = *const c_void;
#[cfg(target_os = "macos")]
type AXValueRef = *const c_void;
#[cfg(target_os = "macos")]
type AXError = i32;
#[cfg(target_os = "macos")]
type AXValueType = u32;

#[cfg(target_os = "macos")]
const K_AX_ERROR_SUCCESS: AXError = 0;
#[cfg(target_os = "macos")]
const K_AX_VALUE_CF_RANGE_TYPE: AXValueType = 4;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXValueCreate(the_type: AXValueType, value_ptr: *const c_void) -> AXValueRef;
    fn AXValueGetValue(
        value: AXValueRef,
        the_type: AXValueType,
        value_ptr: *mut c_void,
    ) -> Boolean;
}

#[cfg(target_os = "macos")]
fn ax_focused_ui_element_attribute() -> CFString {
    CFString::from_static_string("AXFocusedUIElement")
}

#[cfg(target_os = "macos")]
fn ax_selected_text_attribute() -> CFString {
    CFString::from_static_string("AXSelectedText")
}

#[cfg(target_os = "macos")]
fn ax_selected_text_range_attribute() -> CFString {
    CFString::from_static_string("AXSelectedTextRange")
}

#[cfg(target_os = "macos")]
fn ax_string_for_range_parameterized_attribute() -> CFString {
    CFString::from_static_string("AXStringForRange")
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

/// 通过 Windows UI Automation 读取前台焦点控件中的选中文本。
/// 不碰剪贴板，不发送按键，零副作用。
pub fn grab_selected_text() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        grab_selected_text_uia()
    }

    #[cfg(target_os = "macos")]
    {
        grab_selected_text_macos()
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn grab_selected_text_uia() -> Option<String> {
    use uiautomation::UIAutomation;

    let automation = UIAutomation::new().ok()?;
    let focused = automation.get_focused_element().ok()?;

    // 尝试从焦点元素及其祖先中找到 TextPattern 并读取选中文本
    if let Some(text) = try_get_selection(&focused) {
        return Some(text);
    }

    // 向上遍历父元素
    let walker = match automation.create_tree_walker() {
        Ok(w) => w,
        Err(_) => return None,
    };
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
        log::info!("UI Automation 检测到选中文本（{} 字符）", trimmed.len());
        Some(trimmed.to_string())
    }
}

#[cfg(target_os = "macos")]
fn grab_selected_text_macos() -> Option<String> {
    unsafe {
        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            return None;
        }

        let focused_attr = ax_focused_ui_element_attribute();
        let selected_text_attr = ax_selected_text_attribute();
        let selected_text_range_attr = ax_selected_text_range_attribute();
        let string_for_range_attr = ax_string_for_range_parameterized_attribute();

        let focused = match copy_ax_attribute(
            system_wide,
            focused_attr.as_concrete_TypeRef(),
        ) {
            Some(value) => value,
            None => {
                CFRelease(system_wide as CFTypeRef);
                return None;
            }
        };
        let selected = copy_ax_string_attribute(
            focused as AXUIElementRef,
            selected_text_attr.as_concrete_TypeRef(),
        )
        .or_else(|| {
                copy_ax_selected_text_from_range(
                    focused as AXUIElementRef,
                    selected_text_range_attr.as_concrete_TypeRef(),
                    string_for_range_attr.as_concrete_TypeRef(),
                )
            });

        CFRelease(focused);
        CFRelease(system_wide as CFTypeRef);

        let trimmed = selected?.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            log::info!("macOS Accessibility 检测到选中文本（{} 字符）", trimmed.len());
            Some(trimmed)
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn copy_ax_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<CFTypeRef> {
    let mut value: CFTypeRef = ptr::null_mut();
    let status = AXUIElementCopyAttributeValue(element, attribute, &mut value);
    if status == K_AX_ERROR_SUCCESS && !value.is_null() {
        Some(value)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
unsafe fn copy_ax_string_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
) -> Option<String> {
    let value = copy_ax_attribute(element, attribute)?;
    cf_string_from_owned(value)
}

#[cfg(target_os = "macos")]
unsafe fn copy_ax_selected_text_from_range(
    element: AXUIElementRef,
    range_attribute: CFStringRef,
    string_for_range_attribute: CFStringRef,
) -> Option<String> {
    let range_value = copy_ax_attribute(element, range_attribute)?;
    let mut range = CFRange {
        location: 0,
        length: 0,
    };
    let ok = AXValueGetValue(
        range_value as AXValueRef,
        K_AX_VALUE_CF_RANGE_TYPE,
        &mut range as *mut _ as *mut c_void,
    ) != 0;
    CFRelease(range_value);

    if !ok || range.length <= 0 {
        return None;
    }

    let range_param =
        AXValueCreate(K_AX_VALUE_CF_RANGE_TYPE, &range as *const _ as *const c_void);
    if range_param.is_null() {
        return None;
    }

    let mut value: CFTypeRef = ptr::null_mut();
    let status = AXUIElementCopyParameterizedAttributeValue(
        element,
        string_for_range_attribute,
        range_param as CFTypeRef,
        &mut value,
    );
    CFRelease(range_param as CFTypeRef);

    if status != K_AX_ERROR_SUCCESS || value.is_null() {
        return None;
    }

    cf_string_from_owned(value)
}

#[cfg(target_os = "macos")]
unsafe fn cf_string_from_owned(value: CFTypeRef) -> Option<String> {
    if CFGetTypeID(value) != CFStringGetTypeID() {
        CFRelease(value);
        return None;
    }

    let text = CFString::wrap_under_create_rule(value as CFStringRef).to_string();
    Some(text)
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
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

            app_handle
                .clipboard()
                .write_text(text)
                .map_err(|e| AppError::Other(format!("写入剪贴板失败: {}", e)))?;

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            const VK_CONTROL: u16 = 0x11;
            const VK_V: u16 = 0x56;
            const VK_LWIN: u16 = 0x5B;
            const VK_RWIN: u16 = 0x5C;
            const VK_LMENU: u16 = 0xA4;
            const VK_RMENU: u16 = 0xA5;
            const VK_LSHIFT: u16 = 0xA0;
            const VK_RSHIFT: u16 = 0xA1;
            const VK_LCONTROL: u16 = 0xA2;
            const VK_RCONTROL: u16 = 0xA3;

            // 先释放所有可能残留的修饰键，防止 SendInput 的 Ctrl+V
            // 被 OS 解读为 Win+Ctrl+V 等组合
            let modifier_vks = [
                VK_LWIN,
                VK_RWIN,
                VK_LMENU,
                VK_RMENU,
                VK_LSHIFT,
                VK_RSHIFT,
                VK_LCONTROL,
                VK_RCONTROL,
            ];
            let mut release_inputs = Vec::new();
            for &vk in &modifier_vks {
                if unsafe { GetAsyncKeyState(vk as i32) } < 0 {
                    release_inputs.push(make_key_input(vk, 0, KEYEVENTF_KEYUP));
                }
            }
            if !release_inputs.is_empty() {
                log::debug!("粘贴前释放 {} 个残留修饰键", release_inputs.len());
                send_inputs(&release_inputs)?;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }

            let inputs = [
                make_key_input(VK_CONTROL, 0, 0),
                make_key_input(VK_V, 0, 0),
                make_key_input(VK_V, 0, KEYEVENTF_KEYUP),
                make_key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
            ];
            send_inputs(&inputs)?;
        } else {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SendMessageW};

            const VK_RETURN: u16 = 0x0D;
            const VK_TAB: u16 = 0x09;
            const VK_LWIN: u16 = 0x5B;
            const VK_RWIN: u16 = 0x5C;
            const VK_LMENU: u16 = 0xA4;
            const VK_RMENU: u16 = 0xA5;
            const VK_LSHIFT: u16 = 0xA0;
            const VK_RSHIFT: u16 = 0xA1;
            const VK_LCONTROL: u16 = 0xA2;
            const VK_RCONTROL: u16 = 0xA3;
            const WM_IME_CONTROL: u32 = 0x0283;
            const IMC_GETOPENSTATUS: usize = 0x0005;
            const IMC_SETOPENSTATUS: usize = 0x0006;

            // ① 释放残留修饰键，防止目标应用将输入解读为快捷键
            let modifier_vks = [
                VK_LWIN,
                VK_RWIN,
                VK_LMENU,
                VK_RMENU,
                VK_LSHIFT,
                VK_RSHIFT,
                VK_LCONTROL,
                VK_RCONTROL,
            ];
            let mut release_inputs = Vec::new();
            for &vk in &modifier_vks {
                if unsafe { GetAsyncKeyState(vk as i32) } < 0 {
                    release_inputs.push(make_key_input(vk, 0, KEYEVENTF_KEYUP));
                }
            }
            if !release_inputs.is_empty() {
                log::debug!("sendInput 前释放 {} 个残留修饰键", release_inputs.len());
                send_inputs(&release_inputs)?;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }

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

    #[cfg(target_os = "macos")]
    {
        let _ = method;

        crate::services::permissions_service::ensure_accessibility_permission_for_input().await?;
        crate::services::permissions_service::ensure_automation_permission_for_input().await?;
        write_text_to_clipboard(app_handle, text)?;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        let output = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \"v\" using command down")
            .output()
            .await
            .map_err(|e| AppError::Other(format!("启动 osascript 失败: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "系统没有返回更多错误信息".to_string()
            } else {
                stderr
            };
            return Err(AppError::Other(format!(
                "macOS 自动输入失败。请确认“辅助功能”和“自动化 > System Events”都已允许，并在修改权限后彻底退出再重开 app。系统返回: {}",
                detail
            )));
        }
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = app_handle;
        let _ = text;
        let _ = method;
        return Err(AppError::Other("当前平台暂不支持自动输入".to_string()));
    }

    log::info!("已输入 {} 个字符", text.len());
    Ok("已输入".to_string())
}
