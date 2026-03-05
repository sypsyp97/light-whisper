use crate::commands::audio::{
    start_recording_inner, stop_recording_inner, RECORDING_ALREADY_ACTIVE_ERROR,
    RECORDING_NOT_READY_ERROR,
};
use crate::state::AppState;
use crate::utils::AppError;
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
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT,
    VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN,
    VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
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

fn handle_hotkey_start(app_handle: tauri::AppHandle, shortcut_label: String) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
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
                log::debug!("忽略热键 {} 的开始请求: {}", shortcut_label, message);
            }
            Err(err) => {
                let message = err.to_string();
                log::warn!("热键 {} 开始录音失败: {}", shortcut_label, message);
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
        gate.last_release_ms.store(now_unix_ms(), Ordering::Release);
        log::info!("{}", released_log);
        handle_hotkey_stop(app_handle.clone(), shortcut_label.to_string());
    }
}

#[cfg(target_os = "windows")]
struct ModifierOnlyHotkeyMonitor {
    stop_flag: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
fn modifier_monitor_slot() -> &'static Mutex<Option<ModifierOnlyHotkeyMonitor>> {
    static SLOT: OnceLock<Mutex<Option<ModifierOnlyHotkeyMonitor>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
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
        monitor.stop_flag.store(true, Ordering::Relaxed);
        let _ = monitor.handle.join();
    }
}

#[cfg(not(target_os = "windows"))]
fn stop_modifier_only_hotkey_monitor() {}

#[cfg(target_os = "windows")]
fn is_key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

#[cfg(target_os = "windows")]
fn is_modifier_down(modifier: bool, left_vk: i32, right_vk: i32) -> bool {
    !modifier || is_key_down(left_vk) || is_key_down(right_vk)
}

#[cfg(target_os = "windows")]
fn parse_main_key_to_vk(main_key: &str) -> Option<u16> {
    match main_key {
        "Escape" => Some(VK_ESCAPE as u16),
        "Enter" => Some(VK_RETURN as u16),
        "Tab" => Some(VK_TAB as u16),
        "Space" => Some(VK_SPACE as u16),
        "Backspace" => Some(VK_BACK as u16),
        "Delete" => Some(VK_DELETE as u16),
        "Insert" => Some(VK_INSERT as u16),
        "Home" => Some(VK_HOME as u16),
        "End" => Some(VK_END as u16),
        "PageUp" => Some(VK_PRIOR as u16),
        "PageDown" => Some(VK_NEXT as u16),
        "ArrowUp" => Some(VK_UP as u16),
        "ArrowDown" => Some(VK_DOWN as u16),
        "ArrowLeft" => Some(VK_LEFT as u16),
        "ArrowRight" => Some(VK_RIGHT as u16),
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
                Some(VK_F1 as u16 + index - 1)
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

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("ctrl-win-hotkey-monitor".to_string())
        .spawn(move || {
            let mut was_active = false;

            while !stop_flag_clone.load(Ordering::Relaxed) {
                let ctrl_down = is_key_down(VK_LCONTROL as i32) || is_key_down(VK_RCONTROL as i32);
                let win_down = is_key_down(VK_LWIN as i32) || is_key_down(VK_RWIN as i32);
                let is_active = ctrl_down && win_down;

                if is_active != was_active {
                    was_active = is_active;
                    if is_active {
                        dispatch_hotkey_press(&app_handle, "Ctrl+Win 按下，开始录音", "Ctrl+Win");
                    } else {
                        dispatch_hotkey_release(&app_handle, "Ctrl+Win 松开，停止录音", "Ctrl+Win");
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            if was_active {
                dispatch_hotkey_release(&app_handle, "Ctrl+Win 监听结束，补发松开事件", "Ctrl+Win");
            }
        })
        .map_err(|e| AppError::Other(format!("启动 Ctrl+Win 热键监听失败: {}", e)))?;

    let mut guard = match modifier_monitor_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(ModifierOnlyHotkeyMonitor { stop_flag, handle });
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

    let normalized = normalize_shortcut(&shortcut)?;
    stop_modifier_only_hotkey_monitor();
    reset_hotkey_event_gate();

    let _ = app_handle.global_shortcut().unregister_all();

    if let ShortcutRegistrationMode::CtrlSuperModifierOnly = normalized {
        start_ctrl_super_modifier_only_hotkey_monitor(app_handle.clone())?;
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
            AppError::Other(format!(
                "注册快捷键 {} 失败: {}。{}",
                normalized_shortcut, e, hint
            ))
        })?;

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

    log::info!("所有全局快捷键已注销");
    Ok("所有全局快捷键已注销".to_string())
}
