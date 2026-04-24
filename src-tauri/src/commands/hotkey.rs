use crate::commands::audio::{
    start_recording_inner, stop_recording_inner, RECORDING_ALREADY_ACTIVE_ERROR,
    RECORDING_NOT_READY_ERROR,
};
use crate::state::{AppState, RecordingSlot, RecordingTrigger};
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
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT, VK_LCONTROL,
    VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    PM_NOREMOVE, WH_KEYBOARD_LL, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_NULL, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

const HOTKEY_REPRESS_DEBOUNCE_MS: u64 = 180;
/// RegisterHotKey probe uses this atom ID
#[cfg(target_os = "windows")]
const PROBE_HOTKEY_ID: i32 = 0x4C57; // "LW"

/// Counter for injected events ignored by the LLKH filter
#[cfg(target_os = "windows")]
static IGNORED_INJECTED_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// HotkeyBackend — selects between RegisterHotKey and low-level hook
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HotkeyBackend {
    /// Standard Windows RegisterHotKey — most stable, toggle-only
    RegisterHotKey,
    /// WH_KEYBOARD_LL — for hold mode, modifier-only, Win combos
    LowLevelHook,
}

fn classify_backend(spec: &HotkeySpec) -> HotkeyBackend {
    if is_toggle_mode() {
        match spec {
            HotkeySpec::ModifierOnly { .. } => HotkeyBackend::LowLevelHook,
            HotkeySpec::Standard { .. } => HotkeyBackend::RegisterHotKey,
        }
    } else {
        // Hold mode always needs LLKH for key-up detection
        HotkeyBackend::LowLevelHook
    }
}

// ---------------------------------------------------------------------------
// Dispatch channel — moves heavy work off the hook thread
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
enum DispatchEvent {
    Press(Arc<UnifiedHookState>, String),
    Release(Arc<UnifiedHookState>, String),
}

#[cfg(target_os = "windows")]
fn dispatch_channel() -> &'static std::sync::mpsc::Sender<DispatchEvent> {
    static TX: OnceLock<std::sync::mpsc::Sender<DispatchEvent>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<DispatchEvent>();
        std::thread::Builder::new()
            .name("hotkey-dispatch".into())
            .spawn(move || {
                for event in rx {
                    match event {
                        DispatchEvent::Press(state, msg) => {
                            dispatch_hotkey_press(
                                &state.app_handle,
                                &state.gate,
                                state.trigger,
                                &msg,
                                state.spec.label(),
                            );
                        }
                        DispatchEvent::Release(state, msg) => {
                            dispatch_hotkey_release(
                                &state.app_handle,
                                &state.gate,
                                state.trigger,
                                &msg,
                                state.spec.label(),
                            );
                        }
                    }
                }
            })
            .expect("failed to start hotkey dispatch worker");
        tx
    })
}

/// Send dispatch event from hook callback — never blocks.
#[cfg(target_os = "windows")]
fn send_dispatch(event: DispatchEvent) {
    let _ = dispatch_channel().send(event);
}

/// Probe whether a hotkey is already registered system-wide via RegisterHotKey.
/// Returns a human-readable warning if conflicting, None otherwise.
#[cfg(target_os = "windows")]
fn probe_system_hotkey_conflict(spec: &HotkeySpec) -> Option<String> {
    let (win_mods, vk) = match spec {
        HotkeySpec::Standard {
            modifiers, main_vk, ..
        } => {
            let mut m = 0u32;
            if modifiers.ctrl {
                m |= MOD_CONTROL;
            }
            if modifiers.alt {
                m |= MOD_ALT;
            }
            if modifiers.shift {
                m |= MOD_SHIFT;
            }
            if modifiers.super_key {
                m |= MOD_WIN;
            }
            (m, *main_vk as u32)
        }
        // Modifier-only combos can't be probed via RegisterHotKey
        HotkeySpec::ModifierOnly { .. } => return None,
    };

    // Attempt to register — success means no conflict
    let ok = unsafe { RegisterHotKey(std::ptr::null_mut(), PROBE_HOTKEY_ID, win_mods, vk) };
    if ok != 0 {
        unsafe { UnregisterHotKey(std::ptr::null_mut(), PROBE_HOTKEY_ID) };
        None
    } else {
        Some(format!(
            "快捷键 {} 可能已被其他程序占用，部分情况下可能同时触发两个程序的功能",
            spec.label()
        ))
    }
}

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

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn reset_hotkey_event_gate(gate: &HotkeyEventGate) {
    gate.is_pressed.store(false, Ordering::Release);
    gate.last_release_ms.store(0, Ordering::Release);
    gate.toggle_active.store(false, Ordering::Release);
}

#[cfg(target_os = "windows")]
fn reset_hotkey_gate_for_trigger(trigger: RecordingTrigger) {
    let guard = match unified_hook_state_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let state = match trigger {
        RecordingTrigger::DictationOriginal => guard.dictation.as_ref(),
        RecordingTrigger::DictationTranslated => guard.translation.as_ref(),
        RecordingTrigger::Assistant => guard.assistant.as_ref(),
    };
    if let Some(state) = state {
        reset_hotkey_event_gate(&state.gate);
    }
}

#[cfg(not(target_os = "windows"))]
fn reset_hotkey_gate_for_trigger(_trigger: RecordingTrigger) {}

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

fn hotkey_kind_label(kind: HotkeyKind) -> &'static str {
    match kind {
        HotkeyKind::Dictation => "说话",
        HotkeyKind::Translation => "翻译",
        HotkeyKind::Assistant => "助手",
    }
}

fn ensure_hotkey_not_conflicting(
    _app_handle: &tauri::AppHandle,
    kind: HotkeyKind,
    candidate_label: &str,
) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        let guard = match unified_hook_state_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        for (other_kind, state) in [
            (HotkeyKind::Dictation, guard.dictation.as_ref()),
            (HotkeyKind::Translation, guard.translation.as_ref()),
            (HotkeyKind::Assistant, guard.assistant.as_ref()),
        ] {
            if other_kind == kind {
                continue;
            }
            if let Some(state) = state {
                if state.spec.label() == candidate_label {
                    return Err(AppError::Other(format!(
                        "快捷键 {} 已被{}热键占用，请使用不同的组合键",
                        candidate_label,
                        hotkey_kind_label(other_kind)
                    )));
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = kind;
        let _ = candidate_label;
    }

    Ok(())
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

fn update_hotkey_diagnostic_for_trigger<F>(
    app_handle: &tauri::AppHandle,
    trigger: RecordingTrigger,
    update: F,
) where
    F: FnOnce(&mut crate::state::HotkeyDiagnosticState),
{
    if trigger == RecordingTrigger::DictationOriginal {
        update_hotkey_diagnostic(app_handle, update);
    }
}

fn handle_hotkey_start(
    app_handle: tauri::AppHandle,
    shortcut_label: String,
    trigger: RecordingTrigger,
) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();

        // 选中文本抓取并行化：立即 spawn 到后台，作为参数交给 start_recording_inner。
        // handle 由该会话的 RecordingSession.edit_grab 持有，finalize_recording 会以
        // 短超时 join；结果只在 finalize 的本地变量里流转，不写全局，避免跨会话串位。
        let grab_handle =
            tokio::task::spawn_blocking(crate::commands::clipboard::grab_selected_text);

        match start_recording_inner(
            app_handle.clone(),
            state.inner(),
            trigger,
            Some(grab_handle),
        )
        .await
        {
            Ok(session_id) => {
                log::info!(
                    "热键 {} 触发录音开始 (session {}, mode={})",
                    shortcut_label,
                    session_id,
                    trigger.mode().as_str()
                );
            }
            Err(AppError::Audio(message))
                if message == RECORDING_NOT_READY_ERROR
                    || message == RECORDING_ALREADY_ACTIVE_ERROR =>
            {
                if is_toggle_mode() {
                    reset_hotkey_gate_for_trigger(trigger);
                }
                log::debug!("忽略热键 {} 的开始请求: {}", shortcut_label, message);
            }
            Err(err) => {
                if is_toggle_mode() {
                    reset_hotkey_gate_for_trigger(trigger);
                }
                let message = err.to_string();
                log::warn!("热键 {} 开始录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
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

fn handle_hotkey_stop(
    app_handle: tauri::AppHandle,
    shortcut_label: String,
    trigger: RecordingTrigger,
) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();

        let matches_trigger = state
            .recording
            .recording
            .lock()
            .as_ref()
            .is_some_and(|slot| slot.trigger() == trigger);
        if !matches_trigger {
            log::debug!(
                "忽略热键 {} 的停止请求：当前活跃录音不属于 trigger={:?}",
                shortcut_label,
                trigger
            );
            return;
        }

        match stop_recording_inner(app_handle.clone(), state.inner()).await {
            Ok(Some(session_id)) => {
                log::info!(
                    "热键 {} 触发录音停止 (session {}, mode={})",
                    shortcut_label,
                    session_id,
                    trigger.mode().as_str()
                );
            }
            Ok(None) => {
                log::debug!("忽略热键 {} 的停止请求：当前没有活跃录音", shortcut_label);
            }
            Err(err) => {
                let message = err.to_string();
                log::warn!("热键 {} 停止录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
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

fn dispatch_hotkey_press(
    app_handle: &tauri::AppHandle,
    gate: &HotkeyEventGate,
    trigger: RecordingTrigger,
    pressed_log: &str,
    shortcut_label: &str,
) {
    let active_trigger = app_handle
        .state::<AppState>()
        .recording
        .recording
        .lock()
        .as_ref()
        .map(RecordingSlot::trigger);
    let allow_toggle_stop = is_toggle_mode()
        && gate.toggle_active.load(Ordering::Acquire)
        && active_trigger == Some(trigger);

    if active_trigger.is_some() && !allow_toggle_stop {
        log::debug!(
            "忽略热键 {} 的按下：已有录音进行中 (active trigger={:?}, request trigger={:?})",
            shortcut_label,
            active_trigger,
            trigger
        );
        return;
    }

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
            update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
                diagnostic.is_pressed = false;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("released".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_released_at_ms = Some(now_ms);
            });
            handle_hotkey_stop(app_handle.clone(), shortcut_label.to_string(), trigger);
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
            update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
                diagnostic.is_pressed = true;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("pressed".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_pressed_at_ms = Some(now_ms);
            });
            handle_hotkey_start(app_handle.clone(), shortcut_label.to_string(), trigger);
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
        update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
            diagnostic.is_pressed = true;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("pressed".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_pressed_at_ms = Some(now_ms);
        });
        handle_hotkey_start(app_handle.clone(), shortcut_label.to_string(), trigger);
    }
}

fn dispatch_hotkey_release(
    app_handle: &tauri::AppHandle,
    gate: &HotkeyEventGate,
    trigger: RecordingTrigger,
    released_log: &str,
    shortcut_label: &str,
) {
    // In toggle mode, release is a no-op (press handles both start and stop)
    if is_toggle_mode() {
        return;
    }

    if gate.is_pressed.swap(false, Ordering::AcqRel) {
        let now_ms = now_unix_ms();
        gate.last_release_ms.store(now_ms, Ordering::Release);
        log::info!("{}", released_log);
        update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
            diagnostic.is_pressed = false;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("released".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_released_at_ms = Some(now_ms);
        });
        handle_hotkey_stop(app_handle.clone(), shortcut_label.to_string(), trigger);
    }
}

// ---------------------------------------------------------------------------
// Unified low-level keyboard hook (Windows)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
struct UnifiedHookState {
    app_handle: tauri::AppHandle,
    spec: HotkeySpec,
    trigger: RecordingTrigger,
    gate: HotkeyEventGate,
    backend: HotkeyBackend,
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
#[derive(Default, Clone)]
struct UnifiedHookBundle {
    dictation: Option<Arc<UnifiedHookState>>,
    translation: Option<Arc<UnifiedHookState>>,
    assistant: Option<Arc<UnifiedHookState>>,
}

#[cfg(target_os = "windows")]
impl UnifiedHookBundle {
    fn is_empty(&self) -> bool {
        self.dictation.is_none() && self.translation.is_none() && self.assistant.is_none()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HotkeyKind {
    Dictation,
    Translation,
    Assistant,
}

#[cfg(target_os = "windows")]
fn unified_hook_state_slot() -> &'static Mutex<UnifiedHookBundle> {
    static SLOT: OnceLock<Mutex<UnifiedHookBundle>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(UnifiedHookBundle::default()))
}

#[cfg(target_os = "windows")]
fn unified_hook_state_snapshot() -> &'static arc_swap::ArcSwap<UnifiedHookBundle> {
    static SNAPSHOT: OnceLock<arc_swap::ArcSwap<UnifiedHookBundle>> = OnceLock::new();
    SNAPSHOT.get_or_init(|| arc_swap::ArcSwap::from_pointee(UnifiedHookBundle::default()))
}

#[cfg(target_os = "windows")]
fn publish_unified_hook_state_snapshot(bundle: &UnifiedHookBundle) {
    unified_hook_state_snapshot().store(Arc::new(bundle.clone()));
}

#[cfg(target_os = "windows")]
fn set_unified_hook_state(
    kind: HotkeyKind,
    state: Option<Arc<UnifiedHookState>>,
) -> Option<Arc<UnifiedHookState>> {
    let mut guard = match unified_hook_state_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let slot = match kind {
        HotkeyKind::Dictation => &mut guard.dictation,
        HotkeyKind::Translation => &mut guard.translation,
        HotkeyKind::Assistant => &mut guard.assistant,
    };
    let previous = std::mem::replace(slot, state);
    publish_unified_hook_state_snapshot(&guard);
    previous
}

#[cfg(target_os = "windows")]
fn get_unified_hook_states() -> UnifiedHookBundle {
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
    let pass_through = || unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };

    if n_code != HC_ACTION as i32 {
        return pass_through();
    }

    let bundle = unified_hook_state_snapshot().load_full();
    if bundle.is_empty() {
        return pass_through();
    }

    let keyboard = unsafe { &*(l_param as *const KBDLLHOOKSTRUCT) };

    // Skip our own synthetic keys
    if keyboard.dwExtraInfo == SYNTHETIC_KEY_MARKER {
        return pass_through();
    }

    // Filter injected input (macro tools, automation, drivers).
    // LLKHF_INJECTED = 0x10 — set by SendInput / keybd_event from other processes.
    if keyboard.flags & 0x10 != 0 {
        IGNORED_INJECTED_COUNT.fetch_add(1, Ordering::Relaxed);
        return pass_through();
    }

    let message = w_param as u32;
    let is_key_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
    if !is_key_down && !is_key_up {
        return pass_through();
    }

    let vk = keyboard.vkCode;

    let mut swallow = false;
    for state in [
        bundle.dictation.as_ref(),
        bundle.translation.as_ref(),
        bundle.assistant.as_ref(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| s.backend == HotkeyBackend::LowLevelHook)
    {
        swallow |= match &state.spec {
            HotkeySpec::ModifierOnly {
                label,
                required_vks,
            } => handle_modifier_only_event(state, vk, is_key_down, label, required_vks),
            HotkeySpec::Standard {
                label,
                modifiers,
                main_vk,
            } => handle_standard_event(state, vk, is_key_down, label, modifiers, *main_vk),
        };
    }

    if swallow {
        1
    } else {
        pass_through()
    }
}

/// Returns `true` if the event should be swallowed (not forwarded to OS).
/// All heavy work (mutex, IPC) is dispatched via channel — the hook thread
/// only touches atomics.
#[cfg(target_os = "windows")]
fn handle_modifier_only_event(
    state: &Arc<UnifiedHookState>,
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
                send_dispatch(DispatchEvent::Press(
                    state.clone(),
                    format!("{} 按下，开始录音", label),
                ));
            }
        } else {
            // Non-required key pressed → taint
            state.tainted.store(true, Ordering::Release);
            if was_activated {
                state.activated.store(false, Ordering::Release);
                send_dispatch(DispatchEvent::Release(
                    state.clone(),
                    format!("{} 被非热键按键打断，停止录音", label),
                ));
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
                send_dispatch(DispatchEvent::Release(
                    state.clone(),
                    format!("{} 松开，停止录音", label),
                ));
            }
        }

        // When all required modifiers are up, clear taint and handle cleanup
        let any_still_down = state.key_down.iter().any(|b| b.load(Ordering::Acquire));
        if !any_still_down {
            let need_cleanup = was_activated && state.modifier_leaked.swap(false, Ordering::AcqRel);
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
/// All heavy work (mutex, IPC) is dispatched via channel.
#[cfg(target_os = "windows")]
fn handle_standard_event(
    state: &Arc<UnifiedHookState>,
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
            send_dispatch(DispatchEvent::Press(
                state.clone(),
                format!("{} 按下，开始录音", label),
            ));
        }
    } else if !is_key_down && state.activated.load(Ordering::Acquire) {
        let combo_broken = is_main_key || !all_modifiers_down(modifiers);
        if combo_broken {
            state.activated.store(false, Ordering::Release);
            send_dispatch(DispatchEvent::Release(
                state.clone(),
                format!("{} 松开，停止录音", label),
            ));

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
    if is_key_down
        && !currently_activated
        && ((is_win_vk(vk) && modifiers.super_key) || (is_alt_vk(vk) && modifiers.alt))
    {
        state.modifier_leaked.store(true, Ordering::Release);
    }

    currently_activated && is_main_key
}

// ---------------------------------------------------------------------------
// RegisterHotKey backend — dedicated thread with message pump
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
enum RegHotkeyCmd {
    Register {
        id: i32,
        mods: u32,
        vk: u32,
        state: Arc<UnifiedHookState>,
        result_tx: std::sync::mpsc::SyncSender<Result<(), String>>,
    },
    Unregister {
        id: i32,
    },
}

#[cfg(target_os = "windows")]
fn reg_hotkey_cmd_channel() -> &'static (
    std::sync::mpsc::Sender<RegHotkeyCmd>,
    Mutex<std::sync::mpsc::Receiver<RegHotkeyCmd>>,
) {
    static CH: OnceLock<(
        std::sync::mpsc::Sender<RegHotkeyCmd>,
        Mutex<std::sync::mpsc::Receiver<RegHotkeyCmd>>,
    )> = OnceLock::new();
    CH.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        (tx, Mutex::new(rx))
    })
}

#[cfg(target_os = "windows")]
struct RegisterHotkeyBackend {
    thread_id: u32,
    handle: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
fn reg_backend_slot() -> &'static Mutex<Option<RegisterHotkeyBackend>> {
    static SLOT: OnceLock<Mutex<Option<RegisterHotkeyBackend>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Map hotkey kind to a stable RegisterHotKey id
#[cfg(target_os = "windows")]
fn hotkey_kind_to_reg_id(kind: HotkeyKind) -> i32 {
    match kind {
        HotkeyKind::Dictation => 1,
        HotkeyKind::Translation => 2,
        HotkeyKind::Assistant => 3,
    }
}

/// Build MOD_ flags for RegisterHotKey from ShortcutModifiers
#[cfg(target_os = "windows")]
fn shortcut_mods_to_reg_mods(mods: &ShortcutModifiers) -> u32 {
    let mut m = MOD_NOREPEAT;
    if mods.ctrl {
        m |= MOD_CONTROL;
    }
    if mods.alt {
        m |= MOD_ALT;
    }
    if mods.shift {
        m |= MOD_SHIFT;
    }
    if mods.super_key {
        m |= MOD_WIN;
    }
    m
}

#[cfg(target_os = "windows")]
fn ensure_reg_hotkey_backend() -> Result<(), AppError> {
    let backend_running = {
        let guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.is_some()
    };
    if backend_running {
        return Ok(());
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("register-hotkey-backend".into())
        .spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };

            // Ensure message queue exists
            let mut peek_msg: MSG = unsafe { std::mem::zeroed() };
            let _ = unsafe { PeekMessageW(&mut peek_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };

            if ready_tx.send(Ok(thread_id)).is_err() {
                return;
            }

            // Registered hotkey states, keyed by id
            let mut registered: std::collections::HashMap<i32, Arc<UnifiedHookState>> =
                std::collections::HashMap::new();

            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
                if result <= 0 {
                    break;
                }

                match message.message {
                    WM_HOTKEY => {
                        let id = message.wParam as i32;
                        if let Some(state) = registered.get(&id) {
                            send_dispatch(DispatchEvent::Press(
                                state.clone(),
                                format!("{} 按下（RegisterHotKey）", state.spec.label()),
                            ));
                        }
                    }
                    WM_NULL => {
                        // Drain command channel
                        let rx_guard = match reg_hotkey_cmd_channel().1.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        while let Ok(cmd) = rx_guard.try_recv() {
                            match cmd {
                                RegHotkeyCmd::Register {
                                    id,
                                    mods,
                                    vk,
                                    state,
                                    result_tx,
                                } => {
                                    // Unregister previous if any
                                    if registered.contains_key(&id) {
                                        unsafe { UnregisterHotKey(std::ptr::null_mut(), id) };
                                        registered.remove(&id);
                                    }
                                    let ok = unsafe {
                                        RegisterHotKey(std::ptr::null_mut(), id, mods, vk)
                                    };
                                    if ok != 0 {
                                        registered.insert(id, state);
                                        let _ = result_tx.send(Ok(()));
                                    } else {
                                        let err = std::io::Error::last_os_error();
                                        let _ = result_tx
                                            .send(Err(format!("RegisterHotKey 失败: {}", err)));
                                    }
                                }
                                RegHotkeyCmd::Unregister { id } => {
                                    if registered.remove(&id).is_some() {
                                        unsafe {
                                            UnregisterHotKey(std::ptr::null_mut(), id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    },
                }
            }

            // Cleanup all registered hotkeys
            for &id in registered.keys() {
                unsafe { UnregisterHotKey(std::ptr::null_mut(), id) };
            }
        })
        .map_err(|e| AppError::Other(format!("启动 RegisterHotKey 线程失败: {}", e)))?;

    let thread_id = ready_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| AppError::Other(format!("等待 RegisterHotKey 就绪超时: {}", e)))?
        .map_err(AppError::Other)?;

    let mut guard = match reg_backend_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    *guard = Some(RegisterHotkeyBackend { thread_id, handle });
    Ok(())
}

/// Register a hotkey via the RegisterHotKey backend.
/// Blocks until the registration is confirmed or rejected.
#[cfg(target_os = "windows")]
fn register_via_reg_hotkey(
    kind: HotkeyKind,
    mods: &ShortcutModifiers,
    main_vk: u16,
    state: Arc<UnifiedHookState>,
) -> Result<(), AppError> {
    ensure_reg_hotkey_backend()?;

    let id = hotkey_kind_to_reg_id(kind);
    let win_mods = shortcut_mods_to_reg_mods(mods);
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);

    reg_hotkey_cmd_channel()
        .0
        .send(RegHotkeyCmd::Register {
            id,
            mods: win_mods,
            vk: main_vk as u32,
            state,
            result_tx,
        })
        .map_err(|_| AppError::Other("RegisterHotKey 命令通道已关闭".into()))?;

    // Wake the backend thread
    let tid = {
        let guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.as_ref().map(|b| b.thread_id)
    };
    if let Some(tid) = tid {
        unsafe { PostThreadMessageW(tid, WM_NULL, 0, 0) };
    }

    result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| AppError::Other("RegisterHotKey 注册超时".into()))?
        .map_err(AppError::Other)
}

/// Unregister a hotkey from the RegisterHotKey backend.
#[cfg(target_os = "windows")]
fn unregister_via_reg_hotkey(kind: HotkeyKind) {
    let _ = reg_hotkey_cmd_channel().0.send(RegHotkeyCmd::Unregister {
        id: hotkey_kind_to_reg_id(kind),
    });

    let tid = {
        let guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.as_ref().map(|b| b.thread_id)
    };
    if let Some(tid) = tid {
        unsafe { PostThreadMessageW(tid, WM_NULL, 0, 0) };
    }
}

#[cfg(target_os = "windows")]
fn stop_reg_hotkey_backend() {
    let backend = {
        let mut guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.take()
    };

    if let Some(backend) = backend {
        let _ = unsafe { PostThreadMessageW(backend.thread_id, WM_QUIT, 0, 0) };
        let _ = backend.handle.join();
    }
}

// ---------------------------------------------------------------------------
// Monitor thread management (LLKH backend)
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
}

#[cfg(not(target_os = "windows"))]
fn stop_unified_hotkey_monitor() {}

#[cfg(target_os = "windows")]
/// HHOOK is *mut c_void in windows-sys
type HookHandle = *mut std::ffi::c_void;

fn install_hook_on_thread() -> Result<HookHandle, String> {
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
        Err(format!(
            "安装键盘钩子失败: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(hook)
    }
}

#[cfg(target_os = "windows")]
fn ensure_unified_hotkey_monitor(_app_handle: tauri::AppHandle) -> Result<(), AppError> {
    let monitor_running = {
        let guard = match monitor_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.is_some()
    };
    if monitor_running {
        return Ok(());
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("unified-hotkey-monitor".to_string())
        .spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };

            let hook = match install_hook_on_thread() {
                Ok(h) => h,
                Err(msg) => {
                    let _ = ready_tx.send(Err(msg));
                    return;
                }
            };
            let hook_handle: HookHandle = hook;

            // Ensure message queue exists
            let mut peek_msg: MSG = unsafe { std::mem::zeroed() };
            let _ = unsafe { PeekMessageW(&mut peek_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };

            if ready_tx.send(Ok(thread_id)).is_err() {
                unsafe { UnhookWindowsHookEx(hook_handle) };
                return;
            }

            // Simple message loop — the hook callback is kept fast enough that
            // Windows will not silently remove it.
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

            unsafe { UnhookWindowsHookEx(hook_handle) };
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
    *guard = Some(UnifiedHotkeyMonitor { thread_id, handle });
    Ok(())
}

#[cfg(target_os = "windows")]
fn force_release_hotkey(state: &UnifiedHookState) {
    let label = state.spec.label();
    dispatch_hotkey_release(
        &state.app_handle,
        &state.gate,
        state.trigger,
        &format!("{} 监听结束，补发松开事件", label),
        label,
    );
    reset_hotkey_event_gate(&state.gate);
}

#[cfg(not(target_os = "windows"))]
fn ensure_unified_hotkey_monitor(_app_handle: tauri::AppHandle) -> Result<(), AppError> {
    Err(AppError::Other(
        "当前系统暂不支持低层键盘钩子热键".to_string(),
    ))
}

#[cfg(target_os = "windows")]
fn build_hook_state(
    app_handle: tauri::AppHandle,
    spec: HotkeySpec,
    trigger: RecordingTrigger,
) -> Arc<UnifiedHookState> {
    let backend = classify_backend(&spec);
    build_hook_state_with_backend(app_handle, spec, trigger, backend)
}

fn build_hook_state_with_backend(
    app_handle: tauri::AppHandle,
    spec: HotkeySpec,
    trigger: RecordingTrigger,
    backend: HotkeyBackend,
) -> Arc<UnifiedHookState> {
    let key_down_count = match &spec {
        HotkeySpec::ModifierOnly { required_vks, .. } => required_vks.len(),
        HotkeySpec::Standard { .. } => 0,
    };

    Arc::new(UnifiedHookState {
        app_handle,
        backend,
        spec,
        trigger,
        gate: HotkeyEventGate::default(),
        key_down: (0..key_down_count)
            .map(|_| AtomicBool::new(false))
            .collect(),
        activated: AtomicBool::new(false),
        tainted: AtomicBool::new(false),
        modifier_leaked: AtomicBool::new(false),
    })
}

#[cfg(target_os = "windows")]
fn sync_hotkey_monitor_lifecycle(app_handle: tauri::AppHandle) -> Result<(), AppError> {
    let (has_llkh, _has_any) = {
        let guard = match unified_hook_state_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let has_llkh = [
            guard.dictation.as_ref(),
            guard.translation.as_ref(),
            guard.assistant.as_ref(),
        ]
        .iter()
        .flatten()
        .any(|s| s.backend == HotkeyBackend::LowLevelHook);
        let has_any = !guard.is_empty();
        (has_llkh, has_any)
    };

    // Only install LLKH when at least one hotkey actually needs it
    if has_llkh {
        ensure_unified_hotkey_monitor(app_handle)
    } else {
        stop_unified_hotkey_monitor();
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn sync_hotkey_monitor_lifecycle(app_handle: tauri::AppHandle) -> Result<(), AppError> {
    ensure_unified_hotkey_monitor(app_handle)
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
        let main_vk = parse_main_key_to_vk(mk)
            .ok_or_else(|| AppError::Other(format!("无法识别的按键：{}", mk)))?;
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
        let has_any_modifier =
            modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.super_key;
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

/// Register a hotkey on the appropriate backend. Returns the backend label on success.
#[cfg(target_os = "windows")]
fn register_on_chosen_backend(
    app_handle: &tauri::AppHandle,
    kind: HotkeyKind,
    chosen_backend: HotkeyBackend,
    hook_state: Arc<UnifiedHookState>,
    previous_state: Option<Arc<UnifiedHookState>>,
    label: &str,
) -> Result<&'static str, AppError> {
    if chosen_backend == HotkeyBackend::RegisterHotKey {
        if let HotkeySpec::Standard {
            modifiers, main_vk, ..
        } = &hook_state.spec
        {
            let (mods_copy, vk_copy) = (*modifiers, *main_vk);
            match register_via_reg_hotkey(kind, &mods_copy, vk_copy, hook_state.clone()) {
                Ok(()) => {
                    // Sync LLKH lifecycle — may stop hook if not needed by others
                    let _ = sync_hotkey_monitor_lifecycle(app_handle.clone());
                    return Ok("registerHotKey");
                }
                Err(reg_err) => {
                    // RegisterHotKey failed — rebuild state with LowLevelHook backend
                    // so the LLKH callback will process this hotkey.
                    log::warn!(
                        "RegisterHotKey 注册 {} 失败，回退到低层键盘钩子: {}",
                        label,
                        reg_err
                    );
                    let fallback_state = build_hook_state_with_backend(
                        hook_state.app_handle.clone(),
                        hook_state.spec.clone(),
                        hook_state.trigger,
                        HotkeyBackend::LowLevelHook,
                    );
                    set_unified_hook_state(kind, Some(fallback_state));
                    // Fall through to LLKH path below
                }
            }
        }
    }

    // LLKH path (either direct or fallback from RegisterHotKey failure)
    if let Err(err) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
        let _ = set_unified_hook_state(kind, previous_state);
        let _ = sync_hotkey_monitor_lifecycle(app_handle.clone());
        let now_ms = now_unix_ms();
        update_hotkey_diagnostic(app_handle, |diagnostic| {
            diagnostic.shortcut = label.to_string();
            diagnostic.registered = false;
            diagnostic.backend = "none".to_string();
            diagnostic.is_pressed = false;
            diagnostic.last_error = Some(err.to_string());
            diagnostic.warning = hotkey_warning_message(label);
            diagnostic.last_event = Some("error".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
        });
        return Err(err);
    }

    Ok("lowLevelHook")
}

/// Try to register a hotkey via RegisterHotKey if its backend demands it.
/// On failure, rebuilds state with LowLevelHook backend in the slot.
#[cfg(target_os = "windows")]
fn try_register_hotkey_backend(kind: HotkeyKind, state: &Arc<UnifiedHookState>) {
    if state.backend != HotkeyBackend::RegisterHotKey {
        return;
    }
    if let HotkeySpec::Standard {
        modifiers, main_vk, ..
    } = &state.spec
    {
        let (mods_copy, vk_copy) = (*modifiers, *main_vk);
        if let Err(e) = register_via_reg_hotkey(kind, &mods_copy, vk_copy, state.clone()) {
            log::warn!(
                "{} RegisterHotKey 失败，回退到 LLKH: {}",
                state.spec.label(),
                e
            );
            let fallback = build_hook_state_with_backend(
                state.app_handle.clone(),
                state.spec.clone(),
                state.trigger,
                HotkeyBackend::LowLevelHook,
            );
            set_unified_hook_state(kind, Some(fallback));
        }
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

    let label = spec.label().to_string();
    ensure_hotkey_not_conflicting(&app_handle, HotkeyKind::Dictation, &label)?;

    // Unregister from the previous backend BEFORE probing — otherwise our own
    // RegisterHotKey registration would cause a false positive system conflict.
    #[cfg(target_os = "windows")]
    {
        unregister_via_reg_hotkey(HotkeyKind::Dictation);
    }

    // Probe system-wide conflict
    #[cfg(target_os = "windows")]
    let system_conflict = probe_system_hotkey_conflict(&spec);
    #[cfg(not(target_os = "windows"))]
    let system_conflict: Option<String> = None;

    if let Some(ref msg) = system_conflict {
        log::warn!("系统快捷键冲突检测: {}", msg);
    }

    #[cfg(target_os = "windows")]
    let hook_state = build_hook_state(
        app_handle.clone(),
        spec,
        RecordingTrigger::DictationOriginal,
    );

    #[cfg(target_os = "windows")]
    let chosen_backend = hook_state.backend;

    #[cfg(target_os = "windows")]
    let previous_state = set_unified_hook_state(HotkeyKind::Dictation, Some(hook_state.clone()));

    #[cfg(target_os = "windows")]
    if let Some(previous) = previous_state.as_ref() {
        force_release_hotkey(previous);
    }

    #[cfg(target_os = "windows")]
    let backend_label = register_on_chosen_backend(
        &app_handle,
        HotkeyKind::Dictation,
        chosen_backend,
        hook_state,
        previous_state,
        &label,
    )?;

    #[cfg(not(target_os = "windows"))]
    let backend_label = {
        sync_hotkey_monitor_lifecycle(app_handle.clone())?;
        "lowLevelHook"
    };

    let now_ms = now_unix_ms();
    update_hotkey_diagnostic(&app_handle, |diagnostic| {
        diagnostic.shortcut = label.clone();
        diagnostic.registered = true;
        diagnostic.backend = backend_label.to_string();
        diagnostic.is_pressed = false;
        diagnostic.last_error = None;
        diagnostic.warning = hotkey_warning_message(&label);
        diagnostic.system_conflict = system_conflict;
        diagnostic.last_event = Some("registered".to_string());
        diagnostic.last_event_at_ms = Some(now_ms);
        diagnostic.last_registered_at_ms = Some(now_ms);
    });

    log::info!("自定义快捷键 {} 已注册（{}）", label, backend_label);
    Ok(format!("快捷键 {} 已注册", label))
}

#[tauri::command]
pub async fn register_translation_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    register_translation_hotkey_inner(
        app_handle,
        Some(shortcut.trim().to_string()).filter(|value| !value.is_empty()),
    )
}

pub(crate) fn register_translation_hotkey_inner(
    app_handle: tauri::AppHandle,
    shortcut: Option<String>,
) -> Result<String, AppError> {
    #[cfg(not(target_os = "windows"))]
    {
        if shortcut.is_some() {
            ensure_unified_hotkey_monitor(app_handle)?;
        }
        return Ok("翻译热键已更新".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        // Unregister from previous backend first
        unregister_via_reg_hotkey(HotkeyKind::Translation);

        let next_state = if let Some(shortcut) = shortcut {
            let spec = normalize_shortcut(&shortcut)?;
            ensure_hotkey_not_conflicting(&app_handle, HotkeyKind::Translation, spec.label())?;
            Some(build_hook_state(
                app_handle.clone(),
                spec,
                RecordingTrigger::DictationTranslated,
            ))
        } else {
            None
        };

        let previous_state = set_unified_hook_state(HotkeyKind::Translation, next_state.clone());

        if let Some(previous) = previous_state.as_ref() {
            force_release_hotkey(previous);
        }

        if let Some(ref state) = next_state {
            try_register_hotkey_backend(HotkeyKind::Translation, state);
        }

        if let Err(err) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
            let _ = set_unified_hook_state(HotkeyKind::Translation, previous_state);
            let _ = sync_hotkey_monitor_lifecycle(app_handle.clone());
            return Err(err);
        }

        let label = next_state
            .as_ref()
            .map(|state| state.spec.label().to_string())
            .unwrap_or_else(|| "未设置".to_string());
        log::info!("翻译热键已更新: {}", label);
        Ok(format!("翻译热键已更新: {}", label))
    }
}

#[tauri::command]
pub async fn register_assistant_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    register_assistant_hotkey_inner(
        app_handle,
        Some(shortcut.trim().to_string()).filter(|value| !value.is_empty()),
    )
}

pub(crate) fn register_assistant_hotkey_inner(
    app_handle: tauri::AppHandle,
    shortcut: Option<String>,
) -> Result<String, AppError> {
    #[cfg(not(target_os = "windows"))]
    {
        if shortcut.is_some() {
            ensure_unified_hotkey_monitor(app_handle)?;
        }
        return Ok("助手热键已更新".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        // Unregister from previous backend first
        unregister_via_reg_hotkey(HotkeyKind::Assistant);

        let next_state = if let Some(shortcut) = shortcut {
            let spec = normalize_shortcut(&shortcut)?;
            ensure_hotkey_not_conflicting(&app_handle, HotkeyKind::Assistant, spec.label())?;
            Some(build_hook_state(
                app_handle.clone(),
                spec,
                RecordingTrigger::Assistant,
            ))
        } else {
            None
        };

        let previous_state = set_unified_hook_state(HotkeyKind::Assistant, next_state.clone());

        if let Some(previous) = previous_state.as_ref() {
            force_release_hotkey(previous);
        }

        if let Some(ref state) = next_state {
            try_register_hotkey_backend(HotkeyKind::Assistant, state);
        }

        if let Err(err) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
            let _ = set_unified_hook_state(HotkeyKind::Assistant, previous_state);
            let _ = sync_hotkey_monitor_lifecycle(app_handle.clone());
            return Err(err);
        }

        let label = next_state
            .as_ref()
            .map(|state| state.spec.label().to_string())
            .unwrap_or_else(|| "未设置".to_string());
        log::info!("助手热键已更新: {}", label);
        Ok(format!("助手热键已更新: {}", label))
    }
}

#[tauri::command]
pub async fn unregister_all_hotkeys(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    #[cfg(target_os = "windows")]
    {
        // Unregister from RegisterHotKey backend
        unregister_via_reg_hotkey(HotkeyKind::Dictation);
        unregister_via_reg_hotkey(HotkeyKind::Translation);
        unregister_via_reg_hotkey(HotkeyKind::Assistant);

        // Unregister from LLKH backend
        if let Some(previous) = set_unified_hook_state(HotkeyKind::Dictation, None) {
            force_release_hotkey(&previous);
        }
        if let Some(previous) = set_unified_hook_state(HotkeyKind::Translation, None) {
            force_release_hotkey(&previous);
        }
        if let Some(previous) = set_unified_hook_state(HotkeyKind::Assistant, None) {
            force_release_hotkey(&previous);
        }
    }
    stop_unified_hotkey_monitor();
    #[cfg(target_os = "windows")]
    stop_reg_hotkey_backend();

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
        let state = _app_handle.state::<AppState>();
        let active_trigger = state
            .recording
            .recording
            .lock()
            .as_ref()
            .map(RecordingSlot::trigger);
        if let Some(trigger) = active_trigger {
            handle_hotkey_stop(
                _app_handle.clone(),
                "切换到按住模式，停止当前录音".to_string(),
                trigger,
            );
        }
    }

    // Mode change may require backend migration for all registered hotkeys.
    // Re-register each active hotkey so classify_backend picks the right backend.
    #[cfg(target_os = "windows")]
    {
        let bundle = get_unified_hook_states();
        for (kind, state) in [
            (HotkeyKind::Dictation, bundle.dictation.as_ref()),
            (HotkeyKind::Translation, bundle.translation.as_ref()),
            (HotkeyKind::Assistant, bundle.assistant.as_ref()),
        ] {
            if let Some(old_state) = state {
                let new_backend = classify_backend(&old_state.spec);
                if new_backend != old_state.backend {
                    let label = old_state.spec.label().to_string();
                    log::info!(
                        "模式切换：{} 从 {:?} 迁移到 {:?}",
                        label,
                        old_state.backend,
                        new_backend
                    );

                    // Unregister from old backend
                    unregister_via_reg_hotkey(kind);

                    // Rebuild state with new backend classification
                    let new_state = build_hook_state(
                        old_state.app_handle.clone(),
                        old_state.spec.clone(),
                        old_state.trigger,
                    );

                    force_release_hotkey(old_state);
                    set_unified_hook_state(kind, Some(new_state.clone()));

                    try_register_hotkey_backend(kind, &new_state);
                }
            }
        }
        let _ = sync_hotkey_monitor_lifecycle(_app_handle.clone());
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

#[cfg(test)]
mod tests {
    #[test]
    fn low_level_hook_callback_does_not_lock_state_slot() {
        let source = include_str!("hotkey.rs");
        let hook_start = source
            .find("fn unified_low_level_keyboard_proc")
            .expect("low-level hook callback should exist");
        let hook_rest = &source[hook_start..];
        let hook_end = hook_rest
            .find("/// Returns `true` if the event should be swallowed")
            .expect("hook callback section should end before helper docs");
        let hook_body = &hook_rest[..hook_end];

        assert!(
            !hook_body.contains("get_unified_hook_states()")
                && !hook_body.contains("unified_hook_state_slot()")
                && !hook_body.contains(".lock()"),
            "WH_KEYBOARD_LL callback must read lock-free state only; mutex locking in the hook path can stall global keyboard input"
        );
    }
}
