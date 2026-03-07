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
    VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT,
    VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT, VK_PRIOR, VK_RCONTROL,
    VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const HOTKEY_REPRESS_DEBOUNCE_MS: u64 = 180;

// ---------------------------------------------------------------------------
// HotkeySpec — describes what key combination triggers the hotkey
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum HotkeySpec {
    /// Pure modifier-key combination (e.g. Ctrl+Win, Alt alone)
    ModifierOnly {
        label: String,
        required_vks: Vec<u16>,
    },
    /// Modifier(s) + a main key (e.g. F2, Ctrl+Space)
    Standard {
        label: String,
        modifiers: ShortcutModifiers,
        main_vk: u16,
    },
}

impl HotkeySpec {
    fn label(&self) -> &str {
        match self {
            Self::ModifierOnly { label, .. } => label,
            Self::Standard { label, .. } => label,
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
struct ShortcutModifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
}

// ---------------------------------------------------------------------------
// Event gate — deduplicates press/release across rapid repeats
// ---------------------------------------------------------------------------

#[derive(Default)]
struct HotkeyEventGate {
    is_pressed: AtomicBool,
    last_release_ms: AtomicU64,
    toggle_active: AtomicBool,
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
    gate.toggle_active.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Toggle mode — persisted as a static AtomicBool
// ---------------------------------------------------------------------------

fn toggle_mode_flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

fn is_toggle_mode() -> bool {
    toggle_mode_flag().load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Diagnostic & error helpers
// ---------------------------------------------------------------------------

fn emit_recording_error(app_handle: &tauri::AppHandle, message: &str) {
    let _ = app_handle.emit(
        "recording-error",
        serde_json::json!({
            "message": message,
        }),
    );
}

fn hotkey_warning_message(shortcut_label: &str) -> Option<String> {
    if shortcut_label.contains("Win") {
        return Some("部分 Win 组合键可能被系统或其他软件保留。".to_string());
    }
    if shortcut_label == "Alt" {
        return Some("独立 Alt 热键会阻止菜单栏激活。".to_string());
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

// ---------------------------------------------------------------------------
// Recording start/stop business logic
// ---------------------------------------------------------------------------

fn handle_hotkey_start(app_handle: tauri::AppHandle, shortcut_label: String) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();

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

// ---------------------------------------------------------------------------
// Dispatch — supports both hold and toggle modes
// ---------------------------------------------------------------------------

fn dispatch_hotkey_press(app_handle: &tauri::AppHandle, pressed_log: &str, shortcut_label: &str) {
    let gate = hotkey_event_gate();

    if is_toggle_mode() {
        // Toggle mode: each press flips recording on/off
        let was_active = gate.toggle_active.load(Ordering::Acquire);
        if was_active {
            // Turn off
            gate.toggle_active.store(false, Ordering::Release);
            gate.is_pressed.store(false, Ordering::Release);
            let now_ms = now_unix_ms();
            gate.last_release_ms.store(now_ms, Ordering::Release);
            log::info!("切换模式：再次按下 {}，停止录音", shortcut_label);
            update_hotkey_diagnostic(app_handle, |diagnostic| {
                diagnostic.is_pressed = false;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("released".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_released_at_ms = Some(now_ms);
            });
            handle_hotkey_stop(app_handle.clone(), shortcut_label.to_string());
        } else {
            // Turn on — apply debounce
            let now_ms = now_unix_ms();
            let last_release_ms = gate.last_release_ms.load(Ordering::Acquire);
            if now_ms.saturating_sub(last_release_ms) < HOTKEY_REPRESS_DEBOUNCE_MS {
                return;
            }
            gate.toggle_active.store(true, Ordering::Release);
            gate.is_pressed.store(true, Ordering::Release);
            log::info!("切换模式：{}", pressed_log);
            update_hotkey_diagnostic(app_handle, |diagnostic| {
                diagnostic.is_pressed = true;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("pressed".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_pressed_at_ms = Some(now_ms);
            });
            handle_hotkey_start(app_handle.clone(), shortcut_label.to_string());
        }
        return;
    }

    // Hold mode (original behavior)
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

    // In toggle mode, release is a no-op (press handles both start and stop)
    if is_toggle_mode() {
        return;
    }

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

// ---------------------------------------------------------------------------
// Unified low-level keyboard hook (Windows)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
struct UnifiedHookState {
    app_handle: tauri::AppHandle,
    spec: HotkeySpec,
    /// Per-VK key-down tracking for modifier-only mode
    key_down: Vec<AtomicBool>,
    /// Hotkey combination currently active
    activated: AtomicBool,
    /// A non-hotkey key was pressed during modifier-only hold → taint (cancel)
    tainted: AtomicBool,
    /// A modifier key leaked to OS before the combo was complete
    modifier_leaked: AtomicBool,
}

#[cfg(target_os = "windows")]
fn unified_hook_state_slot() -> &'static Mutex<Option<Arc<UnifiedHookState>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<UnifiedHookState>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "windows")]
fn set_unified_hook_state(state: Option<Arc<UnifiedHookState>>) {
    let mut guard = match unified_hook_state_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    *guard = state;
}

#[cfg(target_os = "windows")]
fn get_unified_hook_state() -> Option<Arc<UnifiedHookState>> {
    let guard = match unified_hook_state_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.clone()
}

#[cfg(target_os = "windows")]
const SYNTHETIC_KEY_MARKER: usize = 0x4C575F53; // "LW_S"

#[cfg(target_os = "windows")]
fn send_synthetic_key_up(vk: u16) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: SYNTHETIC_KEY_MARKER,
            },
        },
    };

    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(target_os = "windows")]
fn is_modifier_vk(vk: u32) -> bool {
    matches!(
        vk as u16,
        x if x == VK_LCONTROL
            || x == VK_RCONTROL
            || x == VK_LMENU
            || x == VK_RMENU
            || x == VK_LSHIFT
            || x == VK_RSHIFT
            || x == VK_LWIN
            || x == VK_RWIN
    )
}

#[cfg(target_os = "windows")]
fn is_win_vk(vk: u32) -> bool {
    vk as u16 == VK_LWIN || vk as u16 == VK_RWIN
}

#[cfg(target_os = "windows")]
fn is_alt_vk(vk: u32) -> bool {
    vk as u16 == VK_LMENU || vk as u16 == VK_RMENU
}

/// Map a VK code to an index in UnifiedHookState::key_down for ModifierOnly specs.
/// Returns None if the VK is not one of the required VKs.
#[cfg(target_os = "windows")]
fn vk_to_required_index(spec: &HotkeySpec, vk: u32) -> Option<usize> {
    if let HotkeySpec::ModifierOnly { required_vks, .. } = spec {
        required_vks
            .iter()
            .position(|&required| vk_matches_required(vk, required))
    } else {
        None
    }
}

/// Check if a physical VK matches a required VK (handling L/R variants).
/// e.g. VK_LCONTROL matches VK_LCONTROL, VK_RCONTROL also matches VK_LCONTROL
/// because we use the left variant as the canonical representative.
#[cfg(target_os = "windows")]
fn vk_matches_required(physical_vk: u32, required_vk: u16) -> bool {
    let pv = physical_vk as u16;
    if pv == required_vk {
        return true;
    }
    // Map L/R pairs: if required is the left variant, also accept right variant
    match required_vk {
        x if x == VK_LCONTROL => pv == VK_RCONTROL,
        x if x == VK_LMENU => pv == VK_RMENU,
        x if x == VK_LSHIFT => pv == VK_RSHIFT,
        x if x == VK_LWIN => pv == VK_RWIN,
        _ => false,
    }
}

// --- Standard mode helpers ---

#[cfg(target_os = "windows")]
fn modifier_flags_to_vk_pairs(mods: &ShortcutModifiers) -> Vec<(u16, u16)> {
    let mut pairs = Vec::new();
    if mods.ctrl {
        pairs.push((VK_LCONTROL, VK_RCONTROL));
    }
    if mods.alt {
        pairs.push((VK_LMENU, VK_RMENU));
    }
    if mods.shift {
        pairs.push((VK_LSHIFT, VK_RSHIFT));
    }
    if mods.super_key {
        pairs.push((VK_LWIN, VK_RWIN));
    }
    pairs
}

#[cfg(target_os = "windows")]
fn is_key_physically_down(vk: u16) -> bool {
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk as i32) < 0 }
}

#[cfg(target_os = "windows")]
fn all_modifiers_down(mods: &ShortcutModifiers) -> bool {
    for (left, right) in modifier_flags_to_vk_pairs(mods) {
        if !is_key_physically_down(left) && !is_key_physically_down(right) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// The unified hook callback
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
unsafe extern "system" fn unified_low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let pass_through =
        || unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };

    if n_code != HC_ACTION as i32 {
        return pass_through();
    }

    let Some(state) = get_unified_hook_state() else {
        return pass_through();
    };

    let keyboard = unsafe { &*(l_param as *const KBDLLHOOKSTRUCT) };

    // Skip our own synthetic keys
    if keyboard.dwExtraInfo == SYNTHETIC_KEY_MARKER {
        return pass_through();
    }

    let message = w_param as u32;
    let is_key_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
    if !is_key_down && !is_key_up {
        return pass_through();
    }

    let vk = keyboard.vkCode;

    // Handlers return true to swallow the event, false to pass through.
    let swallow = match &state.spec {
        HotkeySpec::ModifierOnly {
            label,
            required_vks,
        } => handle_modifier_only_event(&state, vk, is_key_down, label, required_vks),
        HotkeySpec::Standard {
            label,
            modifiers,
            main_vk,
        } => handle_standard_event(&state, vk, is_key_down, label, modifiers, *main_vk),
    };

    if swallow {
        1
    } else {
        pass_through()
    }
}

/// Returns `true` if the event should be swallowed (not forwarded to OS).
#[cfg(target_os = "windows")]
fn handle_modifier_only_event(
    state: &UnifiedHookState,
    vk: u32,
    is_key_down: bool,
    label: &str,
    required_vks: &[u16],
) -> bool {
    let is_required = vk_to_required_index(&state.spec, vk).is_some();
    let was_activated = state.activated.load(Ordering::Acquire);

    if is_key_down {
        if is_required {
            if let Some(idx) = vk_to_required_index(&state.spec, vk) {
                state.key_down[idx].store(true, Ordering::Release);
            }

            let all_down = state.key_down.iter().all(|b| b.load(Ordering::Acquire));
            if all_down && !was_activated && !state.tainted.load(Ordering::Acquire) {
                state.activated.store(true, Ordering::Release);
                dispatch_hotkey_press(
                    &state.app_handle,
                    &format!("{} 按下，开始录音", label),
                    label,
                );
            }
        } else {
            // Non-required key pressed → taint
            state.tainted.store(true, Ordering::Release);
            if was_activated {
                state.activated.store(false, Ordering::Release);
                dispatch_hotkey_release(
                    &state.app_handle,
                    &format!("{} 被非热键按键打断，停止录音", label),
                    label,
                );
            }
        }
    } else {
        // Key up
        if is_required {
            if let Some(idx) = vk_to_required_index(&state.spec, vk) {
                state.key_down[idx].store(false, Ordering::Release);
            }

            if was_activated {
                state.activated.store(false, Ordering::Release);
                dispatch_hotkey_release(
                    &state.app_handle,
                    &format!("{} 松开，停止录音", label),
                    label,
                );
            }
        }

        // When all required modifiers are up, clear taint and handle cleanup
        let any_still_down = state.key_down.iter().any(|b| b.load(Ordering::Acquire));
        if !any_still_down {
            let need_cleanup =
                was_activated && state.modifier_leaked.swap(false, Ordering::AcqRel);
            state.tainted.store(false, Ordering::Release);
            state.activated.store(false, Ordering::Release);

            if need_cleanup {
                for &rvk in required_vks {
                    if is_win_vk(rvk as u32) {
                        log::debug!("补发合成 Win key-up 修复 OS 修饰键状态");
                        send_synthetic_key_up(VK_LWIN);
                        send_synthetic_key_up(VK_RWIN);
                    }
                    if is_alt_vk(rvk as u32) {
                        log::debug!("补发合成 Alt key-up 修复 OS 修饰键状态");
                        send_synthetic_key_up(VK_LMENU);
                        send_synthetic_key_up(VK_RMENU);
                    }
                }
            }
        }
    }

    // Decide whether to swallow — must happen AFTER state updates above
    let swallow = should_swallow_modifier_only(state, vk);

    // Track modifier leak: key-down of a required modifier that we didn't swallow
    if is_key_down && is_required && !swallow && is_modifier_vk(vk) {
        state.modifier_leaked.store(true, Ordering::Release);
    }

    swallow
}

#[cfg(target_os = "windows")]
fn should_swallow_modifier_only(state: &UnifiedHookState, vk: u32) -> bool {
    let is_required = vk_to_required_index(&state.spec, vk).is_some();
    if !is_required {
        return false;
    }

    let currently_activated = state.activated.load(Ordering::Acquire);
    let any_required_down = state.key_down.iter().any(|b| b.load(Ordering::Acquire));

    // For Win/Alt keys: swallow whenever any required key is down or was activated.
    // This prevents Win from opening start menu, Alt from activating menus.
    if is_win_vk(vk) || is_alt_vk(vk) {
        return currently_activated || any_required_down;
    }

    // For other modifiers (Ctrl, Shift): only swallow when activated
    currently_activated
}

/// Returns `true` if the event should be swallowed.
#[cfg(target_os = "windows")]
fn handle_standard_event(
    state: &UnifiedHookState,
    vk: u32,
    is_key_down: bool,
    label: &str,
    modifiers: &ShortcutModifiers,
    main_vk: u16,
) -> bool {
    let is_main_key = vk as u16 == main_vk;

    if is_key_down && is_main_key {
        let was_activated = state.activated.load(Ordering::Acquire);
        if all_modifiers_down(modifiers) && !was_activated {
            state.activated.store(true, Ordering::Release);
            dispatch_hotkey_press(
                &state.app_handle,
                &format!("{} 按下，开始录音", label),
                label,
            );
        }
    } else if !is_key_down && state.activated.load(Ordering::Acquire) {
        let combo_broken = is_main_key || !all_modifiers_down(modifiers);
        if combo_broken {
            state.activated.store(false, Ordering::Release);
            dispatch_hotkey_release(
                &state.app_handle,
                &format!("{} 松开，停止录音", label),
                label,
            );

            // Clean up leaked modifier state in one read
            let leaked = state.modifier_leaked.swap(false, Ordering::AcqRel);
            if leaked {
                if modifiers.super_key {
                    send_synthetic_key_up(VK_LWIN);
                    send_synthetic_key_up(VK_RWIN);
                }
                if modifiers.alt {
                    send_synthetic_key_up(VK_LMENU);
                    send_synthetic_key_up(VK_RMENU);
                }
            }
        }
    }

    // Read activated state AFTER updates — swallow the main key while hotkey is active.
    // This correctly swallows:
    //   - The triggering key-down (activated just set to true above)
    //   - Repeated key-downs while held
    // And does NOT swallow:
    //   - The key-up that deactivated (activated just set to false above)
    let currently_activated = state.activated.load(Ordering::Acquire);

    // Track modifier leaks for Win/Alt (key-down that we won't swallow)
    if is_key_down && !currently_activated {
        if (is_win_vk(vk) && modifiers.super_key) || (is_alt_vk(vk) && modifiers.alt) {
            state.modifier_leaked.store(true, Ordering::Release);
        }
    }

    currently_activated && is_main_key
}

// ---------------------------------------------------------------------------
// Monitor thread management
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
struct UnifiedHotkeyMonitor {
    thread_id: u32,
    handle: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
fn monitor_slot() -> &'static Mutex<Option<UnifiedHotkeyMonitor>> {
    static SLOT: OnceLock<Mutex<Option<UnifiedHotkeyMonitor>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "windows")]
fn stop_unified_hotkey_monitor() {
    let monitor = {
        let mut guard = match monitor_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.take()
    };

    if let Some(monitor) = monitor {
        let _ = unsafe { PostThreadMessageW(monitor.thread_id, WM_QUIT, 0, 0) };
        let _ = monitor.handle.join();
    }

    set_unified_hook_state(None);
}

#[cfg(not(target_os = "windows"))]
fn stop_unified_hotkey_monitor() {}

#[cfg(target_os = "windows")]
fn start_unified_hotkey_monitor(
    app_handle: tauri::AppHandle,
    spec: HotkeySpec,
) -> Result<(), AppError> {
    stop_unified_hotkey_monitor();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let key_down_count = match &spec {
        HotkeySpec::ModifierOnly { required_vks, .. } => required_vks.len(),
        HotkeySpec::Standard { .. } => 0,
    };

    let handle = std::thread::Builder::new()
        .name("unified-hotkey-monitor".to_string())
        .spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };

            let key_down: Vec<AtomicBool> =
                (0..key_down_count).map(|_| AtomicBool::new(false)).collect();

            let hook_state = Arc::new(UnifiedHookState {
                app_handle: app_handle.clone(),
                spec,
                key_down,
                activated: AtomicBool::new(false),
                tainted: AtomicBool::new(false),
                modifier_leaked: AtomicBool::new(false),
            });

            set_unified_hook_state(Some(hook_state.clone()));

            let module = unsafe { GetModuleHandleW(std::ptr::null()) };
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(unified_low_level_keyboard_proc),
                    module,
                    0,
                )
            };

            if hook.is_null() {
                set_unified_hook_state(None);
                let _ = ready_tx.send(Err(format!(
                    "安装键盘钩子失败: {}",
                    std::io::Error::last_os_error()
                )));
                return;
            }

            // Ensure message queue exists
            let mut peek_msg: MSG = unsafe { std::mem::zeroed() };
            let _ =
                unsafe { PeekMessageW(&mut peek_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };

            if ready_tx.send(Ok(thread_id)).is_err() {
                unsafe { UnhookWindowsHookEx(hook) };
                force_release_hotkey(&hook_state);
                set_unified_hook_state(None);
                return;
            }

            // Message loop
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

            unsafe { UnhookWindowsHookEx(hook) };
            force_release_hotkey(&hook_state);
            set_unified_hook_state(None);
        })
        .map_err(|e| AppError::Other(format!("启动热键监听线程失败: {}", e)))?;

    let thread_id = ready_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| AppError::Other(format!("等待热键监听就绪超时: {}", e)))?
        .map_err(AppError::Other)?;

    let mut guard = match monitor_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    *guard = Some(UnifiedHotkeyMonitor {
        thread_id,
        handle,
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn force_release_hotkey(state: &UnifiedHookState) {
    let label = state.spec.label();
    dispatch_hotkey_release(
        &state.app_handle,
        &format!("{} 监听结束，补发松开事件", label),
        label,
    );
    // Also clean up toggle state
    hotkey_event_gate().toggle_active.store(false, Ordering::Release);
}

#[cfg(not(target_os = "windows"))]
fn start_unified_hotkey_monitor(
    _app_handle: tauri::AppHandle,
    _spec: HotkeySpec,
) -> Result<(), AppError> {
    Err(AppError::Other(
        "当前系统暂不支持低层键盘钩子热键".to_string(),
    ))
}

// ---------------------------------------------------------------------------
// Key parsing
// ---------------------------------------------------------------------------

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

/// Convert modifier flags to a list of canonical (left variant) VK codes.
#[cfg(target_os = "windows")]
fn modifiers_to_required_vks(mods: &ShortcutModifiers) -> Vec<u16> {
    let mut vks = Vec::new();
    if mods.ctrl {
        vks.push(VK_LCONTROL);
    }
    if mods.alt {
        vks.push(VK_LMENU);
    }
    if mods.shift {
        vks.push(VK_LSHIFT);
    }
    if mods.super_key {
        vks.push(VK_LWIN);
    }
    vks
}

// ---------------------------------------------------------------------------
// normalize_shortcut → HotkeySpec
// ---------------------------------------------------------------------------

fn normalize_shortcut(raw: &str) -> Result<HotkeySpec, AppError> {
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

    // Build ordered label
    let mut label_parts = Vec::with_capacity(5);
    if modifiers.ctrl {
        label_parts.push("Ctrl");
    }
    if modifiers.alt {
        label_parts.push("Alt");
    }
    if modifiers.shift {
        label_parts.push("Shift");
    }
    if modifiers.super_key {
        label_parts.push("Win");
    }

    if let Some(ref mk) = main_key {
        label_parts.push(mk);

        #[cfg(target_os = "windows")]
        let main_vk = parse_main_key_to_vk(mk).ok_or_else(|| {
            AppError::Other(format!("无法识别的按键：{}", mk))
        })?;
        #[cfg(not(target_os = "windows"))]
        let main_vk = 0u16;

        let label = label_parts.join("+");
        Ok(HotkeySpec::Standard {
            label,
            modifiers,
            main_vk,
        })
    } else {
        // Pure modifier-only hotkey
        let has_any_modifier = modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.super_key;
        if !has_any_modifier {
            return Err(AppError::Other(
                "快捷键格式无效：请至少指定一个按键".to_string(),
            ));
        }

        #[cfg(target_os = "windows")]
        let required_vks = modifiers_to_required_vks(&modifiers);
        #[cfg(not(target_os = "windows"))]
        let required_vks = Vec::new();

        let label = label_parts.join("+");
        Ok(HotkeySpec::ModifierOnly {
            label,
            required_vks,
        })
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn register_custom_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    let spec = match normalize_shortcut(&shortcut) {
        Ok(spec) => spec,
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

    reset_hotkey_event_gate();

    let label = spec.label().to_string();

    start_unified_hotkey_monitor(app_handle.clone(), spec).map_err(|err| {
        let now_ms = now_unix_ms();
        update_hotkey_diagnostic(&app_handle, |diagnostic| {
            diagnostic.shortcut = label.clone();
            diagnostic.registered = false;
            diagnostic.backend = "lowLevelHook".to_string();
            diagnostic.is_pressed = false;
            diagnostic.last_error = Some(err.to_string());
            diagnostic.warning = hotkey_warning_message(&label);
            diagnostic.last_event = Some("error".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
        });
        err
    })?;

    let now_ms = now_unix_ms();
    update_hotkey_diagnostic(&app_handle, |diagnostic| {
        diagnostic.shortcut = label.clone();
        diagnostic.registered = true;
        diagnostic.backend = "lowLevelHook".to_string();
        diagnostic.is_pressed = false;
        diagnostic.last_error = None;
        diagnostic.warning = hotkey_warning_message(&label);
        diagnostic.last_event = Some("registered".to_string());
        diagnostic.last_event_at_ms = Some(now_ms);
        diagnostic.last_registered_at_ms = Some(now_ms);
    });

    log::info!("自定义快捷键 {} 已注册（统一低层键盘钩子）", label);
    Ok(format!("快捷键 {} 已注册", label))
}

#[tauri::command]
pub async fn unregister_all_hotkeys(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    stop_unified_hotkey_monitor();
    reset_hotkey_event_gate();

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
pub async fn set_recording_mode(
    _app_handle: tauri::AppHandle,
    toggle: bool,
) -> Result<(), AppError> {
    toggle_mode_flag().store(toggle, Ordering::Release);
    // If switching from toggle→hold while toggle is active, stop recording
    if !toggle {
        let gate = hotkey_event_gate();
        if gate.toggle_active.swap(false, Ordering::AcqRel) {
            gate.is_pressed.store(false, Ordering::Release);
            let now_ms = now_unix_ms();
            gate.last_release_ms.store(now_ms, Ordering::Release);
            handle_hotkey_stop(_app_handle, "切换到按住模式，停止当前录音".to_string());
        }
    }
    log::info!("录音模式已设置为: {}", if toggle { "切换" } else { "按住" });
    Ok(())
}

#[tauri::command]
pub async fn get_hotkey_diagnostic(
    state: tauri::State<'_, AppState>,
) -> Result<crate::state::HotkeyDiagnosticState, AppError> {
    Ok(state.hotkey_diagnostic_snapshot())
}
