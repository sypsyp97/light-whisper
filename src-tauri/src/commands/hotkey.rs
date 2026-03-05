use crate::utils::AppError;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    OnceLock,
};
#[cfg(target_os = "windows")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::thread::JoinHandle;
use tauri::Emitter;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LCONTROL, VK_LWIN, VK_RCONTROL, VK_RWIN,
};

enum ShortcutRegistrationMode {
    Standard(String),
    CtrlSuperModifierOnly,
}

const HOTKEY_REPRESS_DEBOUNCE_MS: u64 = 180;

#[derive(Default)]
struct ShortcutModifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
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

fn emit_hotkey_press<R: tauri::Runtime>(app: &tauri::AppHandle<R>, pressed_log: &str) {
    let gate = hotkey_event_gate();
    let now_ms = now_unix_ms();
    let last_release_ms = gate.last_release_ms.load(Ordering::Acquire);

    if now_ms.saturating_sub(last_release_ms) < HOTKEY_REPRESS_DEBOUNCE_MS {
        log::debug!(
            "忽略热键按下抖动（距离上次松开 {}ms）",
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
        let _ = app.emit("hotkey-press", ());
    }
}

fn emit_hotkey_release<R: tauri::Runtime>(app: &tauri::AppHandle<R>, released_log: &str) {
    let gate = hotkey_event_gate();
    if gate.is_pressed.swap(false, Ordering::AcqRel) {
        gate.last_release_ms.store(now_unix_ms(), Ordering::Release);
        log::info!("{}", released_log);
        let _ = app.emit("hotkey-release", ());
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
                        emit_hotkey_press(&app_handle, "Ctrl+Win 按下，开始录音");
                    } else {
                        emit_hotkey_release(&app_handle, "Ctrl+Win 松开，停止录音");
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            if was_active {
                emit_hotkey_release(&app_handle, "Ctrl+Win 监听结束，补发松开事件");
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
    normalized.push(main_key.unwrap_or_default());
    Ok(ShortcutRegistrationMode::Standard(normalized.join("+")))
}

fn emit_shortcut_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: tauri_plugin_global_shortcut::ShortcutState,
    pressed_log: &str,
    released_log: &str,
) {
    match state {
        tauri_plugin_global_shortcut::ShortcutState::Pressed => {
            emit_hotkey_press(app, pressed_log);
        }
        tauri_plugin_global_shortcut::ShortcutState::Released => {
            emit_hotkey_release(app, released_log);
        }
    }
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

    let normalized = normalize_shortcut(&shortcut)?;
    stop_modifier_only_hotkey_monitor();
    reset_hotkey_event_gate();

    // 先尝试注销已有的快捷键（忽略错误）
    let _ = app_handle.global_shortcut().unregister_all();

    if let ShortcutRegistrationMode::CtrlSuperModifierOnly = normalized {
        start_ctrl_super_modifier_only_hotkey_monitor(app_handle.clone())?;
        log::info!("自定义快捷键 Ctrl+Win 已注册（纯修饰键监听）");
        return Ok("快捷键 Ctrl+Win 已注册".to_string());
    }

    let ShortcutRegistrationMode::Standard(normalized_shortcut) = normalized else {
        return Err(AppError::Other("快捷键类型不支持".to_string()));
    };

    // 注册新的快捷键
    app_handle
        .global_shortcut()
        .on_shortcut(
            normalized_shortcut.as_str(),
            move |app, _shortcut, event| {
                emit_shortcut_state(
                    app,
                    event.state,
                    "自定义快捷键按下，开始录音",
                    "自定义快捷键松开，停止录音",
                );
            },
        )
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
    Ok(format!(
        "快捷键 {} 已注册",
        normalized_shortcut.replace("Super", "Win")
    ))
}

/// 注销所有全局快捷键
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
