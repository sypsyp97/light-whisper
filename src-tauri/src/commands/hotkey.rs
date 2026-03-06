use crate::commands::audio::{
    start_recording_inner, stop_recording_inner, RECORDING_ALREADY_ACTIVE_ERROR,
    RECORDING_NOT_READY_ERROR,
};
use crate::state::AppState;
use crate::utils::{AppError, MutexRecover};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    OnceLock,
};
#[cfg(target_os = "windows")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::thread::JoinHandle;
use tauri::{Emitter, Manager};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT,
    VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN,
    VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const HOTKEY_REPRESS_DEBOUNCE_MS: u64 = 180;

enum ShortcutRegistrationMode {
    Standard(RegisteredShortcut),
    CtrlSuperModifierOnly,
}

#[derive(Clone, Copy, Default)]
struct ShortcutModifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
}

#[derive(Clone)]
struct RegisteredShortcut {
    normalized: String,
    modifiers: ShortcutModifiers,
    main_key: String,
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct WindowsShortcutKeys {
    modifiers: ShortcutModifiers,
    main_vk: u16,
}

#[derive(Default)]
struct HotkeyEventGate {
    is_pressed: AtomicBool,
    last_release_ms: AtomicU64,
}

fn hotkey_event_gate() -> &'static HotkeyEventGate {
    static GATE: OnceLock<HotkeyEventGate> = OnceLock::new();
    GATE.get_or_init(HotkeyEventGate::default)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn reset_hotkey_event_gate() {
    let gate = hotkey_event_gate();
    gate.is_pressed.store(false, Ordering::Release);
    gate.last_release_ms.store(0, Ordering::Release);
}

fn emit_recording_error(app_handle: &tauri::AppHandle, message: &str) {
    let _ = app_handle.emit(
        "recording-error",
        serde_json::json!({
            "message": message,
        }),
    );
}

fn hotkey_warning_message(shortcut_label: &str, backend: &str) -> Option<String> {
    if backend == "lowLevelHook" && shortcut_label == "Ctrl+Win" {
        return Some("纯 Ctrl+Win 当前走低层键盘钩子监听。".to_string());
    }
    if shortcut_label.contains("Win") {
        return Some("部分 Win 组合键可能被系统或其他软件保留。".to_string());
    }
    None
}

fn update_hotkey_diagnostic<F>(app_handle: &tauri::AppHandle, update: F)
where
    F: FnOnce(&mut crate::state::HotkeyDiagnosticState),
{
    let state = app_handle.state::<AppState>();
    let (_, snapshot) = state.update_hotkey_diagnostic(|diagnostic| {
        update(diagnostic);
    });
    let _ = app_handle.emit("hotkey-diagnostic", snapshot);
}

fn handle_hotkey_start(app_handle: tauri::AppHandle, shortcut_label: String) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();

        // 通过 UI Automation 读取选中文本，决定是否进入编辑模式
        let selected = tokio::task::spawn_blocking(crate::commands::clipboard::grab_selected_text)
            .await
            .unwrap_or(None);
        *state.edit_context.lock_or_recover() = selected;

        match start_recording_inner(app_handle.clone(), state.inner()).await {
            Ok(session_id) => {
                log::info!(
                    "热键 {} 触发录音开始 (session {})",
                    shortcut_label,
                    session_id
                );
            }
            Err(AppError::Audio(message))
                if message == RECORDING_NOT_READY_ERROR
                    || message == RECORDING_ALREADY_ACTIVE_ERROR =>
            {
                state.edit_context.lock_or_recover().take();
                log::debug!("忽略热键 {} 的开始请求: {}", shortcut_label, message);
            }
            Err(err) => {
                state.edit_context.lock_or_recover().take();
                let message = err.to_string();
                log::warn!("热键 {} 开始录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic(&app_handle, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
                emit_recording_error(&app_handle, &message);
            }
        }
    });
}

fn handle_hotkey_stop(app_handle: tauri::AppHandle, shortcut_label: String) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        match stop_recording_inner(app_handle.clone(), state.inner()).await {
            Ok(Some(session_id)) => {
                log::info!(
                    "热键 {} 触发录音停止 (session {})",
                    shortcut_label,
                    session_id
                );
            }
            Ok(None) => {
                log::debug!("忽略热键 {} 的停止请求：当前没有活跃录音", shortcut_label);
            }
            Err(err) => {
                let message = err.to_string();
                log::warn!("热键 {} 停止录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic(&app_handle, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
                emit_recording_error(&app_handle, &message);
            }
        }
    });
}

fn dispatch_hotkey_press(app_handle: &tauri::AppHandle, pressed_log: &str, shortcut_label: &str) {
    let gate = hotkey_event_gate();
    let now_ms = now_unix_ms();
    let last_release_ms = gate.last_release_ms.load(Ordering::Acquire);

    if now_ms.saturating_sub(last_release_ms) < HOTKEY_REPRESS_DEBOUNCE_MS {
        log::debug!(
            "忽略热键 {} 的按下抖动（距离上次松开 {}ms）",
            shortcut_label,
            now_ms.saturating_sub(last_release_ms)
        );
        return;
    }

    if gate
        .is_pressed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        log::info!("{}", pressed_log);
        update_hotkey_diagnostic(app_handle, |diagnostic| {
            diagnostic.is_pressed = true;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("pressed".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_pressed_at_ms = Some(now_ms);
        });
        handle_hotkey_start(app_handle.clone(), shortcut_label.to_string());
    }
}

fn dispatch_hotkey_release(
    app_handle: &tauri::AppHandle,
    released_log: &str,
    shortcut_label: &str,
) {
    let gate = hotkey_event_gate();
    if gate.is_pressed.swap(false, Ordering::AcqRel) {
        let now_ms = now_unix_ms();
        gate.last_release_ms.store(now_ms, Ordering::Release);
        log::info!("{}", released_log);
        update_hotkey_diagnostic(app_handle, |diagnostic| {
            diagnostic.is_pressed = false;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("released".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_released_at_ms = Some(now_ms);
        });
        handle_hotkey_stop(app_handle.clone(), shortcut_label.to_string());
    }
}

#[cfg(target_os = "windows")]
struct ModifierOnlyHotkeyMonitor {
    thread_id: u32,
    handle: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
fn modifier_monitor_slot() -> &'static Mutex<Option<ModifierOnlyHotkeyMonitor>> {
    static SLOT: OnceLock<Mutex<Option<ModifierOnlyHotkeyMonitor>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "windows")]
/// 合成按键标记，hook 看到此 dwExtraInfo 值时直接放行，避免重入
const SYNTHETIC_KEY_MARKER: usize = 0x4C575F53; // "LW_S"

#[cfg(target_os = "windows")]
struct CtrlSuperHookState {
    app_handle: tauri::AppHandle,
    ctrl_down: AtomicBool,
    win_down: AtomicBool,
    /// 标记 Ctrl+Win 组合是否曾经同时按下（激活过热键）。
    /// 在所有键松开之前保持 true，防止 Win key-up 泄漏给 OS。
    activated: AtomicBool,
    /// 标记 Win key-down 是否曾穿透到 OS（Win 先于 Ctrl 按下时会发生）。
    /// 热键结束后需要补发合成 Win key-up 来清理 OS 状态。
    win_leaked_to_os: AtomicBool,
}

#[cfg(target_os = "windows")]
fn ctrl_super_hook_state_slot() -> &'static Mutex<Option<Arc<CtrlSuperHookState>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<CtrlSuperHookState>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "windows")]
fn set_ctrl_super_hook_state(state: Option<Arc<CtrlSuperHookState>>) {
    let mut guard = match ctrl_super_hook_state_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = state;
}

#[cfg(target_os = "windows")]
fn ctrl_super_hook_state() -> Option<Arc<CtrlSuperHookState>> {
    let guard = match ctrl_super_hook_state_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone()
}

#[cfg(target_os = "windows")]
fn send_synthetic_win_key_up() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };

    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: SYNTHETIC_KEY_MARKER,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_RWIN,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: SYNTHETIC_KEY_MARKER,
                },
            },
        },
    ];

    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

#[cfg(target_os = "windows")]
fn release_ctrl_super_if_needed(state: &CtrlSuperHookState, message: &str) {
    state.ctrl_down.store(false, Ordering::Release);
    state.win_down.store(false, Ordering::Release);
    dispatch_hotkey_release(&state.app_handle, message, "Ctrl+Win");
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn ctrl_super_low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };
    }

    let Some(state) = ctrl_super_hook_state() else {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };
    };

    let keyboard = unsafe { &*(l_param as *const KBDLLHOOKSTRUCT) };

    // 跳过自己发出的合成按键，防止重入
    if keyboard.dwExtraInfo == SYNTHETIC_KEY_MARKER {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };
    }

    let message = w_param as u32;
    let is_key_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
    if !is_key_down && !is_key_up {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };
    }

    let vk = keyboard.vkCode;
    let is_ctrl_key = vk == VK_LCONTROL as u32 || vk == VK_RCONTROL as u32;
    let is_win_key = vk == VK_LWIN as u32 || vk == VK_RWIN as u32;
    if !is_ctrl_key && !is_win_key {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };
    }

    let ctrl_before = state.ctrl_down.load(Ordering::Acquire);
    let win_before = state.win_down.load(Ordering::Acquire);
    let active_before = ctrl_before && win_before;

    let mut ctrl_after = ctrl_before;
    let mut win_after = win_before;

    if is_ctrl_key {
        ctrl_after = is_key_down;
        state.ctrl_down.store(ctrl_after, Ordering::Release);
    }
    if is_win_key {
        win_after = is_key_down;
        state.win_down.store(win_after, Ordering::Release);
    }

    let active_after = ctrl_after && win_after;
    let was_activated = state.activated.load(Ordering::Acquire);

    if active_after && !active_before {
        state.activated.store(true, Ordering::Release);
        dispatch_hotkey_press(&state.app_handle, "Ctrl+Win 按下，开始录音", "Ctrl+Win");
    } else if active_before && !active_after {
        dispatch_hotkey_release(&state.app_handle, "Ctrl+Win 松开，停止录音", "Ctrl+Win");
    }

    // 吞噬 Win 键事件，防止泄漏给 OS 触发系统快捷键。
    // was_activated 确保即使 Ctrl 先松开，后续的 Win key-up 也被吞噬。
    let should_swallow =
        is_win_key && (active_before || active_after || ctrl_before || ctrl_after || was_activated);

    // 追踪 Win key-down 是否穿透到了 OS（Win 先于 Ctrl 按下时会发生）
    if is_win_key && is_key_down && !should_swallow {
        state.win_leaked_to_os.store(true, Ordering::Release);
    }

    // 当两个键都松开后，清除激活标记，并补发合成 Win key-up 修复 OS 状态
    if !ctrl_after && !win_after {
        let need_cleanup = was_activated
            && state.win_leaked_to_os.swap(false, Ordering::AcqRel);
        state.activated.store(false, Ordering::Release);

        if need_cleanup {
            log::debug!("补发合成 Win key-up 修复 OS 修饰键状态");
            send_synthetic_win_key_up();
        }
    }

    if should_swallow {
        return 1;
    }

    unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) }
}

#[cfg(target_os = "windows")]
fn stop_modifier_only_hotkey_monitor() {
    let monitor = {
        let mut guard = match modifier_monitor_slot().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.take()
    };

    if let Some(monitor) = monitor {
        let _ = unsafe { PostThreadMessageW(monitor.thread_id, WM_QUIT, 0, 0) };
        let _ = monitor.handle.join();
    }
}

#[cfg(not(target_os = "windows"))]
fn stop_modifier_only_hotkey_monitor() {}

#[cfg(target_os = "windows")]
fn is_key_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) < 0 }
}

#[cfg(target_os = "windows")]
fn is_modifier_down(modifier: bool, left_vk: i32, right_vk: i32) -> bool {
    !modifier || is_key_down(left_vk) || is_key_down(right_vk)
}

#[cfg(target_os = "windows")]
fn parse_main_key_to_vk(main_key: &str) -> Option<u16> {
    match main_key {
        "Escape" => Some(VK_ESCAPE),
        "Enter" => Some(VK_RETURN),
        "Tab" => Some(VK_TAB),
        "Space" => Some(VK_SPACE),
        "Backspace" => Some(VK_BACK),
        "Delete" => Some(VK_DELETE),
        "Insert" => Some(VK_INSERT),
        "Home" => Some(VK_HOME),
        "End" => Some(VK_END),
        "PageUp" => Some(VK_PRIOR),
        "PageDown" => Some(VK_NEXT),
        "ArrowUp" => Some(VK_UP),
        "ArrowDown" => Some(VK_DOWN),
        "ArrowLeft" => Some(VK_LEFT),
        "ArrowRight" => Some(VK_RIGHT),
        _ if main_key.len() == 1 => {
            let byte = main_key.as_bytes()[0];
            if byte.is_ascii_uppercase() || byte.is_ascii_digit() {
                Some(byte as u16)
            } else {
                None
            }
        }
        _ if main_key.starts_with('F') => {
            let suffix = main_key.strip_prefix('F')?;
            let index = suffix.parse::<u16>().ok()?;
            if (1..=24).contains(&index) {
                Some(VK_F1 + index - 1)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn resolve_windows_shortcut(shortcut: &RegisteredShortcut) -> Option<WindowsShortcutKeys> {
    let main_vk = parse_main_key_to_vk(&shortcut.main_key)?;
    Some(WindowsShortcutKeys {
        modifiers: shortcut.modifiers,
        main_vk,
    })
}

#[cfg(target_os = "windows")]
fn is_shortcut_currently_down(shortcut: &WindowsShortcutKeys) -> bool {
    is_modifier_down(
        shortcut.modifiers.ctrl,
        VK_LCONTROL as i32,
        VK_RCONTROL as i32,
    ) && is_modifier_down(shortcut.modifiers.alt, VK_LMENU as i32, VK_RMENU as i32)
        && is_modifier_down(shortcut.modifiers.shift, VK_LSHIFT as i32, VK_RSHIFT as i32)
        && is_modifier_down(shortcut.modifiers.super_key, VK_LWIN as i32, VK_RWIN as i32)
        && is_key_down(shortcut.main_vk as i32)
}

#[cfg(target_os = "windows")]
fn should_accept_shortcut_state(
    state: tauri_plugin_global_shortcut::ShortcutState,
    shortcut_label: &str,
    keys: Option<&WindowsShortcutKeys>,
) -> bool {
    let Some(keys) = keys else {
        return true;
    };

    let is_down = is_shortcut_currently_down(keys);
    match state {
        tauri_plugin_global_shortcut::ShortcutState::Pressed if !is_down => {
            log::warn!(
                "忽略热键 {} 的幽灵按下事件：系统回调触发时按键实际上已松开",
                shortcut_label
            );
            false
        }
        tauri_plugin_global_shortcut::ShortcutState::Released if is_down => {
            log::warn!(
                "忽略热键 {} 的异常松开事件：系统回调触发时按键仍保持按下",
                shortcut_label
            );
            false
        }
        _ => true,
    }
}

#[cfg(not(target_os = "windows"))]
fn should_accept_shortcut_state(
    _state: tauri_plugin_global_shortcut::ShortcutState,
    _shortcut_label: &str,
    _keys: Option<&()>,
) -> bool {
    true
}

#[cfg(target_os = "windows")]
fn start_ctrl_super_modifier_only_hotkey_monitor(
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    stop_modifier_only_hotkey_monitor();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("ctrl-win-hotkey-monitor".to_string())
        .spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };
            let state = Arc::new(CtrlSuperHookState {
                app_handle: app_handle.clone(),
                ctrl_down: AtomicBool::new(false),
                win_down: AtomicBool::new(false),
                activated: AtomicBool::new(false),
                win_leaked_to_os: AtomicBool::new(false),
            });

            set_ctrl_super_hook_state(Some(state.clone()));

            let module = unsafe { GetModuleHandleW(std::ptr::null()) };
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(ctrl_super_low_level_keyboard_proc),
                    module,
                    0,
                )
            };

            if hook.is_null() {
                set_ctrl_super_hook_state(None);
                let _ = ready_tx.send(Err(format!(
                    "安装 Ctrl+Win 键盘钩子失败: {}",
                    std::io::Error::last_os_error()
                )));
                return;
            }

            let mut peek_msg: MSG = unsafe { std::mem::zeroed() };
            let _ = unsafe { PeekMessageW(&mut peek_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };

            if ready_tx.send(Ok(thread_id)).is_err() {
                unsafe {
                    UnhookWindowsHookEx(hook);
                }
                release_ctrl_super_if_needed(&state, "Ctrl+Win 监听结束，补发松开事件");
                set_ctrl_super_hook_state(None);
                return;
            }

            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
                if result <= 0 {
                    break;
                }
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            unsafe {
                UnhookWindowsHookEx(hook);
            }
            release_ctrl_super_if_needed(&state, "Ctrl+Win 监听结束，补发松开事件");
            set_ctrl_super_hook_state(None);
        })
        .map_err(|e| AppError::Other(format!("启动 Ctrl+Win 热键监听失败: {}", e)))?;

    let thread_id = ready_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| AppError::Other(format!("等待 Ctrl+Win 热键监听就绪超时: {}", e)))?
        .map_err(AppError::Other)?;

    let mut guard = match modifier_monitor_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(ModifierOnlyHotkeyMonitor { thread_id, handle });
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn start_ctrl_super_modifier_only_hotkey_monitor(
    _app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    Err(AppError::Other(
        "当前系统暂不支持将 Ctrl+Win 作为独立热键".to_string(),
    ))
}

fn normalize_shortcut(raw: &str) -> Result<ShortcutRegistrationMode, AppError> {
    let mut modifiers = ShortcutModifiers::default();
    let mut main_key: Option<String> = None;

    for token in raw.split('+').map(str::trim) {
        if token.is_empty() {
            return Err(AppError::Other(
                "快捷键格式无效：存在空白项，请重新设置".to_string(),
            ));
        }

        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.ctrl = true,
            "alt" | "option" | "altgraph" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            "super" | "meta" | "win" | "windows" | "cmd" | "command" | "os" => {
                modifiers.super_key = true
            }
            _ => {
                if main_key.is_some() {
                    return Err(AppError::Other(
                        "快捷键格式无效：只能包含一个主键（例如 Ctrl+Win+R）".to_string(),
                    ));
                }
                main_key = Some(token.to_string());
            }
        }
    }

    if main_key.is_none() {
        if modifiers.ctrl && modifiers.super_key && !modifiers.alt && !modifiers.shift {
            return Ok(ShortcutRegistrationMode::CtrlSuperModifierOnly);
        }
        return Err(AppError::Other(
            "纯修饰键热键目前仅支持 Ctrl+Win。其他组合请添加主键（例如 Ctrl+Shift+R）".to_string(),
        ));
    }

    let mut normalized = Vec::with_capacity(5);
    if modifiers.ctrl {
        normalized.push("Ctrl".to_string());
    }
    if modifiers.alt {
        normalized.push("Alt".to_string());
    }
    if modifiers.shift {
        normalized.push("Shift".to_string());
    }
    if modifiers.super_key {
        normalized.push("Super".to_string());
    }
    let main_key = main_key.unwrap_or_default();
    normalized.push(main_key.clone());
    Ok(ShortcutRegistrationMode::Standard(RegisteredShortcut {
        normalized: normalized.join("+"),
        modifiers,
        main_key,
    }))
}

fn emit_shortcut_state(
    app_handle: &tauri::AppHandle,
    shortcut_label: &str,
    state: tauri_plugin_global_shortcut::ShortcutState,
    #[cfg(target_os = "windows")] keys: Option<&WindowsShortcutKeys>,
) {
    #[cfg(target_os = "windows")]
    if !should_accept_shortcut_state(state, shortcut_label, keys) {
        return;
    }

    match state {
        tauri_plugin_global_shortcut::ShortcutState::Pressed => {
            dispatch_hotkey_press(app_handle, "自定义快捷键按下，开始录音", shortcut_label);
        }
        tauri_plugin_global_shortcut::ShortcutState::Released => {
            dispatch_hotkey_release(app_handle, "自定义快捷键松开，停止录音", shortcut_label);
        }
    }
}

#[tauri::command]
pub async fn register_custom_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let normalized = match normalize_shortcut(&shortcut) {
        Ok(normalized) => normalized,
        Err(err) => {
            let now_ms = now_unix_ms();
            update_hotkey_diagnostic(&app_handle, |diagnostic| {
                diagnostic.shortcut = shortcut.replace("Super", "Win");
                diagnostic.registered = false;
                diagnostic.backend = "none".to_string();
                diagnostic.is_pressed = false;
                diagnostic.last_error = Some(err.to_string());
                diagnostic.warning = None;
                diagnostic.last_event = Some("error".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
            });
            return Err(err);
        }
    };
    stop_modifier_only_hotkey_monitor();
    reset_hotkey_event_gate();

    let _ = app_handle.global_shortcut().unregister_all();

    if let ShortcutRegistrationMode::CtrlSuperModifierOnly = normalized {
        start_ctrl_super_modifier_only_hotkey_monitor(app_handle.clone()).map_err(|err| {
            let now_ms = now_unix_ms();
            update_hotkey_diagnostic(&app_handle, |diagnostic| {
                diagnostic.shortcut = "Ctrl+Win".to_string();
                diagnostic.registered = false;
                diagnostic.backend = "lowLevelHook".to_string();
                diagnostic.is_pressed = false;
                diagnostic.last_error = Some(err.to_string());
                diagnostic.warning = hotkey_warning_message("Ctrl+Win", "lowLevelHook");
                diagnostic.last_event = Some("error".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
            });
            err
        })?;
        let now_ms = now_unix_ms();
        update_hotkey_diagnostic(&app_handle, |diagnostic| {
            diagnostic.shortcut = "Ctrl+Win".to_string();
            diagnostic.registered = true;
            diagnostic.backend = "lowLevelHook".to_string();
            diagnostic.is_pressed = false;
            diagnostic.last_error = None;
            diagnostic.warning = hotkey_warning_message("Ctrl+Win", "lowLevelHook");
            diagnostic.last_event = Some("registered".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_registered_at_ms = Some(now_ms);
        });
        log::info!("自定义快捷键 Ctrl+Win 已注册（纯修饰键监听）");
        return Ok("快捷键 Ctrl+Win 已注册".to_string());
    }

    let ShortcutRegistrationMode::Standard(shortcut) = normalized else {
        return Err(AppError::Other("快捷键类型不支持".to_string()));
    };

    let normalized_shortcut = shortcut.normalized.clone();
    let shortcut_label = normalized_shortcut.replace("Super", "Win");
    #[cfg(target_os = "windows")]
    let windows_keys = resolve_windows_shortcut(&shortcut);

    app_handle
        .global_shortcut()
        .on_shortcut(normalized_shortcut.as_str(), {
            let shortcut_label = shortcut_label.clone();
            #[cfg(target_os = "windows")]
            let windows_keys = windows_keys.clone();

            move |app, _shortcut, event| {
                emit_shortcut_state(
                    app,
                    shortcut_label.as_str(),
                    event.state,
                    #[cfg(target_os = "windows")]
                    windows_keys.as_ref(),
                );
            }
        })
        .map_err(|e| {
            let mut hint = "请检查快捷键格式是否正确。".to_string();
            #[cfg(target_os = "windows")]
            if normalized_shortcut.to_ascii_lowercase().contains("super+") {
                hint.push_str("部分 Win 组合键被系统保留，建议尝试 Ctrl+Alt/Shift+字母。");
            }
            let error = AppError::Other(format!(
                "注册快捷键 {} 失败: {}。{}",
                normalized_shortcut, e, hint
            ));
            let now_ms = now_unix_ms();
            update_hotkey_diagnostic(&app_handle, |diagnostic| {
                diagnostic.shortcut = shortcut_label.clone();
                diagnostic.registered = false;
                diagnostic.backend = "globalShortcut".to_string();
                diagnostic.is_pressed = false;
                diagnostic.last_error = Some(error.to_string());
                diagnostic.warning = hotkey_warning_message(&shortcut_label, "globalShortcut");
                diagnostic.last_event = Some("error".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
            });
            error
        })?;

    let now_ms = now_unix_ms();
    update_hotkey_diagnostic(&app_handle, |diagnostic| {
        diagnostic.shortcut = shortcut_label.clone();
        diagnostic.registered = true;
        diagnostic.backend = "globalShortcut".to_string();
        diagnostic.is_pressed = false;
        diagnostic.last_error = None;
        diagnostic.warning = hotkey_warning_message(&shortcut_label, "globalShortcut");
        diagnostic.last_event = Some("registered".to_string());
        diagnostic.last_event_at_ms = Some(now_ms);
        diagnostic.last_registered_at_ms = Some(now_ms);
    });
    log::info!("自定义快捷键 {} 已注册", normalized_shortcut);
    Ok(format!("快捷键 {} 已注册", shortcut_label))
}

#[tauri::command]
pub async fn unregister_all_hotkeys(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    stop_modifier_only_hotkey_monitor();
    reset_hotkey_event_gate();

    app_handle
        .global_shortcut()
        .unregister_all()
        .map_err(|e| AppError::Other(format!("注销所有快捷键失败: {}", e)))?;

    let now_ms = now_unix_ms();
    update_hotkey_diagnostic(&app_handle, |diagnostic| {
        diagnostic.registered = false;
        diagnostic.is_pressed = false;
        diagnostic.last_error = None;
        diagnostic.last_event = Some("unregistered".to_string());
        diagnostic.last_event_at_ms = Some(now_ms);
    });

    log::info!("所有全局快捷键已注销");
    Ok("所有全局快捷键已注销".to_string())
}

#[tauri::command]
pub async fn get_hotkey_diagnostic(
    state: tauri::State<'_, AppState>,
) -> Result<crate::state::HotkeyDiagnosticState, AppError> {
    Ok(state.hotkey_diagnostic_snapshot())
}
