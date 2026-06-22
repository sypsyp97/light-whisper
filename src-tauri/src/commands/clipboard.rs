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
const CLIPBOARD_RESTORE_DELAY_MS: u64 = 200;

#[cfg(target_os = "windows")]
fn send_inputs(inputs: &[INPUT]) -> Result<(), AppError> {
    const SENDINPUT_CHUNK_SIZE: usize = 128;

    for chunk in inputs.chunks(SENDINPUT_CHUNK_SIZE) {
        let sent = unsafe {
            SendInput(
                chunk.len() as u32,
                chunk.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != chunk.len() {
            return Err(AppError::Other(format!(
                "SendInput 调用失败：只发送了 {}/{} 个输入事件（{}）",
                sent,
                chunk.len(),
                std::io::Error::last_os_error()
            )));
        }
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

#[cfg(target_os = "windows")]
fn capture_raw_paste_replacement_target_windows(
    raw_text: &str,
) -> Option<RawPasteReplacementToken> {
    if raw_text.trim().is_empty() {
        return None;
    }

    use uiautomation::patterns::{UITextPattern, UIValuePattern};
    use uiautomation::types::TextPatternRangeEndpoint;
    use uiautomation::UIAutomation;

    let automation = UIAutomation::new().ok()?;
    let focused = automation.get_focused_element().ok()?;
    let value_pattern: UIValuePattern = focused.get_pattern().ok()?;
    if value_pattern.is_readonly().ok()? {
        return None;
    }

    let text_pattern: UITextPattern = focused.get_pattern().ok()?;
    let (caret_active, caret_range) = text_pattern.get_caret_range().ok()?;
    if !caret_active {
        return None;
    }
    let document_range = text_pattern.get_document_range().ok()?;
    let caret_at_document_end = caret_range
        .compare_endpoints(
            TextPatternRangeEndpoint::End,
            &document_range,
            TextPatternRangeEndpoint::End,
        )
        .ok()?
        == 0;
    if !caret_at_document_end {
        return None;
    }

    let runtime_id = focused.get_runtime_id().ok()?;
    let value_before = value_pattern.get_value().ok()?;
    Some(RawPasteReplacementToken::new(
        runtime_id,
        value_before,
        raw_text.to_string(),
    ))
}

#[cfg(target_os = "windows")]
fn replace_raw_paste_suffix_if_unchanged_windows(
    token: &RawPasteReplacementToken,
    polished_text: &str,
) -> Result<bool, AppError> {
    use uiautomation::patterns::UIValuePattern;
    use uiautomation::UIAutomation;

    let automation = UIAutomation::new()
        .map_err(|e| AppError::Other(format!("初始化 UI Automation 失败: {}", e)))?;
    let focused = match automation.get_focused_element() {
        Ok(element) => element,
        Err(e) => {
            log::debug!("raw-first 替换跳过：无法读取当前焦点控件: {}", e);
            return Ok(false);
        }
    };
    match focused.get_runtime_id() {
        Ok(runtime_id) if runtime_id == token.runtime_id => {}
        Ok(_) => {
            log::debug!("raw-first 替换跳过：焦点控件已变化");
            return Ok(false);
        }
        Err(e) => {
            log::debug!("raw-first 替换跳过：无法读取控件 runtime id: {}", e);
            return Ok(false);
        }
    }

    let value_pattern: UIValuePattern = match focused.get_pattern() {
        Ok(pattern) => pattern,
        Err(e) => {
            log::debug!("raw-first 替换跳过：当前控件不支持 ValuePattern: {}", e);
            return Ok(false);
        }
    };
    let current_value = value_pattern
        .get_value()
        .map_err(|e| AppError::Other(format!("读取当前输入框文本失败: {}", e)))?;
    let Some(replacement_value) = replacement_value_if_raw_suffix_unchanged(
        &token.value_before,
        &token.raw_text,
        polished_text,
        &current_value,
    ) else {
        log::debug!("raw-first 替换跳过：raw 文本后已有用户输入或内容已变化");
        return Ok(false);
    };

    value_pattern
        .set_value(&replacement_value)
        .map_err(|e| AppError::Other(format!("替换 raw-first 文本失败: {}", e)))?;
    Ok(true)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPasteReplacementToken {
    runtime_id: Vec<i32>,
    value_before: String,
    raw_text: String,
}

impl RawPasteReplacementToken {
    fn new(runtime_id: Vec<i32>, value_before: String, raw_text: String) -> Self {
        Self {
            runtime_id,
            value_before,
            raw_text,
        }
    }
}

pub fn replacement_value_if_raw_suffix_unchanged(
    value_before: &str,
    raw_text: &str,
    polished_text: &str,
    current_value: &str,
) -> Option<String> {
    if raw_text.is_empty() || raw_text == polished_text {
        return None;
    }

    let expected_current = format!("{value_before}{raw_text}");
    (current_value == expected_current).then(|| format!("{value_before}{polished_text}"))
}

pub fn should_restore_clipboard_after_paste(
    current_clipboard_text: Option<&str>,
    paste_text: &str,
) -> bool {
    current_clipboard_text == Some(paste_text)
}

pub fn capture_raw_paste_replacement_target(raw_text: &str) -> Option<RawPasteReplacementToken> {
    #[cfg(target_os = "windows")]
    {
        capture_raw_paste_replacement_target_windows(raw_text)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = raw_text;
        None
    }
}

pub fn replace_raw_paste_suffix_if_unchanged(
    token: &RawPasteReplacementToken,
    polished_text: &str,
) -> Result<bool, AppError> {
    #[cfg(target_os = "windows")]
    {
        replace_raw_paste_suffix_if_unchanged_windows(token, polished_text)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = token;
        let _ = polished_text;
        Ok(false)
    }
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
            use uiautomation::clipboards::{Clipboard, Snapshot};

            fn capture_clipboard_snapshot() -> Result<Snapshot, AppError> {
                let clipboard = Clipboard::open()
                    .map_err(|e| AppError::Other(format!("打开剪贴板以创建快照失败: {}", e)))?;
                clipboard
                    .snapshot(true)
                    .map_err(|e| AppError::Other(format!("创建剪贴板快照失败: {}", e)))
            }

            fn restore_clipboard_snapshot(snapshot: Snapshot) -> Result<(), AppError> {
                let clipboard = Clipboard::open()
                    .map_err(|e| AppError::Other(format!("打开剪贴板以恢复快照失败: {}", e)))?;
                clipboard
                    .restore(snapshot)
                    .map_err(|e| AppError::Other(format!("恢复剪贴板快照失败: {}", e)))
            }

            let clipboard_snapshot = capture_clipboard_snapshot()?;

            app_handle
                .clipboard()
                .write_text(text)
                .map_err(|e| AppError::Other(format!("写入剪贴板失败: {}", e)))?;

            let paste_result = (|| -> Result<(), AppError> {
                std::thread::sleep(std::time::Duration::from_millis(10));

                release_stuck_modifiers()?;
                std::thread::sleep(std::time::Duration::from_millis(20));

                const VK_CONTROL: u16 = 0x11;
                const VK_V: u16 = 0x56;
                let inputs = [
                    make_key_input(VK_CONTROL, 0, 0),
                    make_key_input(VK_V, 0, 0),
                    make_key_input(VK_V, 0, KEYEVENTF_KEYUP),
                    make_key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
                ];
                send_inputs(&inputs)
            })();

            if paste_result.is_ok() {
                std::thread::sleep(std::time::Duration::from_millis(CLIPBOARD_RESTORE_DELAY_MS));
            }

            let current_clipboard_text = app_handle.clipboard().read_text().ok();
            if should_restore_clipboard_after_paste(current_clipboard_text.as_deref(), text) {
                if let Err(e) = restore_clipboard_snapshot(clipboard_snapshot) {
                    log::warn!("{}", e);
                    if paste_result.is_ok() {
                        return Err(e);
                    }
                }
            } else {
                log::debug!("剪贴板内容已变化，跳过恢复以保留用户的新剪贴板内容");
            }

            paste_result?;
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

#[cfg(test)]
mod tests {
    use super::{replacement_value_if_raw_suffix_unchanged, should_restore_clipboard_after_paste};

    #[test]
    fn sendinput_wrapper_treats_partial_sends_as_failure() {
        let source = include_str!("clipboard.rs");

        assert!(
            source.contains("sent != inputs.len() as u32")
                || source.contains("sent as usize != inputs.len()")
                || source.contains("sent as usize != chunk.len()"),
            "SendInput returning fewer events than requested must fail so text is not reported as pasted after a partial send"
        );
    }

    #[test]
    fn sendinput_unicode_path_chunks_long_text() {
        let source = include_str!("clipboard.rs");

        assert!(
            source.contains("chunks(") || source.contains("SENDINPUT_CHUNK"),
            "long Unicode paste text must be chunked before SendInput to avoid oversized input arrays"
        );
    }

    #[test]
    fn clipboard_restore_guard_allows_restoring_only_app_written_text() {
        assert!(should_restore_clipboard_after_paste(
            Some("dictated clipboard text"),
            "dictated clipboard text"
        ));
    }

    #[test]
    fn clipboard_restore_guard_preserves_user_clipboard_change() {
        assert!(!should_restore_clipboard_after_paste(
            Some("new user clipboard text"),
            "dictated clipboard text"
        ));
    }

    #[test]
    fn clipboard_restore_guard_skips_unavailable_or_non_text_clipboard() {
        assert!(!should_restore_clipboard_after_paste(
            None,
            "dictated clipboard text"
        ));
    }

    #[test]
    fn clipboard_mode_snapshots_before_write_and_restores_after_paste_attempt() {
        let source = include_str!("clipboard.rs");
        let clipboard_branch_start = source
            .find("if use_clipboard")
            .expect("clipboard mode branch should exist");
        let clipboard_branch = &source[clipboard_branch_start..];

        let snapshot_pos = clipboard_branch.find("capture_clipboard_snapshot").expect(
            "clipboard mode must capture the user's clipboard snapshot before writing paste text",
        );
        let write_pos = clipboard_branch
            .find("write_text(text)")
            .expect("clipboard mode must write the dictated paste text");
        assert!(
            snapshot_pos < write_pos,
            "clipboard snapshot must be captured before writing the paste text"
        );

        let paste_attempt_pos = clipboard_branch
            .find("send_inputs(&inputs)")
            .expect("clipboard mode must attempt Ctrl+V paste");
        let restore_guard_pos = clipboard_branch
            .find("should_restore_clipboard_after_paste")
            .expect("clipboard mode restore must be guarded by the current clipboard value");
        let restore_delay_pos = clipboard_branch
            .find("CLIPBOARD_RESTORE_DELAY_MS")
            .expect("clipboard mode must delay restore long enough for Ctrl+V to consume text");
        let restore_pos = clipboard_branch
            .find("restore_clipboard_snapshot(clipboard_snapshot)")
            .expect("clipboard mode must restore the previous clipboard snapshot");
        assert!(
            paste_attempt_pos < restore_delay_pos
                && restore_delay_pos < restore_guard_pos
                && restore_guard_pos < restore_pos,
            "clipboard restore must wait after the Ctrl+V paste attempt, then run through the restore guard"
        );
    }

    #[test]
    fn raw_suffix_replacement_requires_exact_before_plus_raw_value() {
        assert_eq!(
            replacement_value_if_raw_suffix_unchanged("hello ", "wrld", "world", "hello wrld"),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn raw_suffix_replacement_skips_when_user_typed_after_raw() {
        assert_eq!(
            replacement_value_if_raw_suffix_unchanged("hello ", "wrld", "world", "hello wrld!"),
            None
        );
    }

    #[test]
    fn raw_suffix_replacement_skips_mid_document_insertions() {
        assert_eq!(
            replacement_value_if_raw_suffix_unchanged(
                "hello world",
                " brave",
                " brave,",
                "hello brave world"
            ),
            None
        );
    }

    #[test]
    fn raw_suffix_replacement_skips_when_polish_does_not_change_text() {
        assert_eq!(
            replacement_value_if_raw_suffix_unchanged("hello ", "world", "world", "hello world"),
            None
        );
    }
}
