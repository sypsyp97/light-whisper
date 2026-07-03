use std::sync::atomic::Ordering;

use crate::state::AppState;
use crate::utils::AppError;
use tauri::Manager;

/// 使用 Windows API 强制将窗口置于最顶层
#[cfg(target_os = "windows")]
fn force_window_topmost(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    match window.hwnd() {
        Ok(hwnd) => {
            let raw = hwnd.0;
            let topmost = -1isize as *mut std::ffi::c_void;
            unsafe {
                SetWindowPos(
                    raw,
                    topmost,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        Err(err) => log::warn!("获取字幕窗口句柄失败: {}", err),
    }
}

const DEFAULT_SUBTITLE_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_SUBTITLE_WINDOW_HEIGHT: f64 = 720.0;

fn tauri_error(action: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Tauri(format!("{}: {}", action, err))
}

fn require_window(
    app_handle: &tauri::AppHandle,
    label: &str,
    missing_message: &str,
) -> Result<tauri::WebviewWindow, AppError> {
    app_handle
        .get_webview_window(label)
        .ok_or_else(|| AppError::Tauri(missing_message.to_string()))
}

/// 获取光标所在显示器（物理坐标比对）
#[cfg(target_os = "windows")]
fn find_cursor_monitor(app_handle: &tauri::AppHandle) -> Option<tauri::Monitor> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }

    app_handle
        .available_monitors()
        .ok()?
        .into_iter()
        .find(|monitor| {
            let pos = monitor.position();
            let size = monitor.size();
            point.x >= pos.x
                && point.x < pos.x + size.width as i32
                && point.y >= pos.y
                && point.y < pos.y + size.height as i32
        })
}

#[cfg(not(target_os = "windows"))]
fn find_cursor_monitor(_app_handle: &tauri::AppHandle) -> Option<tauri::Monitor> {
    None
}

#[cfg(target_os = "macos")]
fn apply_macos_subtitle_fullscreen_behavior_on_main(
    window: &tauri::WebviewWindow,
    order_front: bool,
) {
    use objc2_app_kit::{NSScreenSaverWindowLevel, NSWindow, NSWindowCollectionBehavior};

    match window.ns_window() {
        Ok(raw_window) if !raw_window.is_null() => unsafe {
            let ns_window: &NSWindow = &*(raw_window as *mut NSWindow);
            ns_window.setHidesOnDeactivate(false);
            ns_window.setCanHide(false);
            let mut behavior = ns_window.collectionBehavior();
            behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces;
            behavior |= NSWindowCollectionBehavior::FullScreenAuxiliary;
            behavior |= NSWindowCollectionBehavior::Stationary;
            behavior |= NSWindowCollectionBehavior::IgnoresCycle;
            behavior &= !NSWindowCollectionBehavior::FullScreenNone;
            ns_window.setCollectionBehavior(behavior);
            ns_window.setLevel(NSScreenSaverWindowLevel);
            if order_front {
                ns_window.orderFrontRegardless();
            }
        },
        Ok(_) => log::warn!("字幕窗口 NSWindow 句柄为空，无法应用全屏 Space 行为"),
        Err(err) => log::warn!("获取字幕窗口 NSWindow 失败: {}", err),
    }
}

#[cfg(target_os = "macos")]
fn apply_macos_subtitle_fullscreen_behavior(window: &tauri::WebviewWindow, order_front: bool) {
    let window = window.clone();
    let task_window = window.clone();
    if let Err(err) = window.run_on_main_thread(move || {
        apply_macos_subtitle_fullscreen_behavior_on_main(&task_window, order_front);
    }) {
        log::warn!("调度字幕窗口 macOS 全屏 Space 行为失败: {}", err);
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_macos_subtitle_fullscreen_behavior(_window: &tauri::WebviewWindow, _order_front: bool) {}

fn resolve_subtitle_layout(app_handle: &tauri::AppHandle) -> (f64, f64, f64, f64) {
    let monitor = find_cursor_monitor(app_handle)
        .or_else(|| {
            app_handle
                .get_webview_window("main")
                .and_then(|window| window.current_monitor().ok().flatten())
        })
        .or_else(|| app_handle.primary_monitor().ok().flatten())
        .or_else(|| {
            app_handle
                .available_monitors()
                .ok()
                .and_then(|monitors| monitors.into_iter().next())
        });

    if let Some(monitor) = monitor {
        let screen_size = monitor.size();
        let screen_pos = monitor.position();
        let scale_factor = monitor.scale_factor();
        let logical_width = (screen_size.width as f64 / scale_factor).max(1.0);
        let logical_height = (screen_size.height as f64 / scale_factor).max(1.0);
        let x = screen_pos.x as f64 / scale_factor;
        let y = screen_pos.y as f64 / scale_factor;
        (logical_width, logical_height, x, y)
    } else {
        log::warn!("未获取到显示器信息，字幕窗口使用默认布局");
        (
            DEFAULT_SUBTITLE_WINDOW_WIDTH,
            DEFAULT_SUBTITLE_WINDOW_HEIGHT,
            0.0,
            0.0,
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn macos_nswindow_behavior_is_dispatched_to_main_thread() {
        let source = include_str!("window.rs");
        let scheduler_start = source
            .find("fn apply_macos_subtitle_fullscreen_behavior(window: &tauri::WebviewWindow, order_front: bool)")
            .expect("macOS subtitle fullscreen behavior scheduler should exist");
        let scheduler = &source[scheduler_start..];
        let scheduler_end = scheduler
            .find("#[cfg(not(target_os = \"macos\"))]")
            .expect("scheduler should end before non-macOS stub");
        let scheduler_body = &scheduler[..scheduler_end];

        assert!(
            scheduler_body.contains("run_on_main_thread(move ||"),
            "raw AppKit NSWindow collection behavior must be scheduled onto the main thread"
        );
        assert!(
            !scheduler_body.contains("ns_window.setCollectionBehavior"),
            "main-thread scheduler must not mutate NSWindow collection behavior directly"
        );
    }

    #[test]
    fn macos_nswindow_behavior_prevents_hide_and_orders_front_when_showing() {
        let source = include_str!("window.rs");
        let behavior_start = source
            .find("fn apply_macos_subtitle_fullscreen_behavior_on_main")
            .expect("macOS raw NSWindow behavior should exist");
        let behavior = &source[behavior_start..];
        let behavior_end = behavior
            .find("#[cfg(target_os = \"macos\")]\nfn apply_macos_subtitle_fullscreen_behavior")
            .expect("raw NSWindow helper should end before scheduler");
        let body = &behavior[..behavior_end];

        assert!(body.contains("setHidesOnDeactivate(false)"));
        assert!(body.contains("setCanHide(false)"));
        assert!(body.contains("FullScreenAuxiliary"));
        assert!(body.contains("NSScreenSaverWindowLevel"));
        assert!(body.contains("orderFrontRegardless()"));
    }
}

fn apply_subtitle_layout(
    app_handle: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), AppError> {
    let (logical_width, logical_height, x, y) = resolve_subtitle_layout(app_handle);
    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            logical_width,
            logical_height,
        )))
        .map_err(|e| tauri_error("设置字幕窗口尺寸失败", e))?;
    window
        .set_position(tauri::Position::Logical(tauri::LogicalPosition::new(x, y)))
        .map_err(|e| tauri_error("设置字幕窗口位置失败", e))?;
    Ok(())
}

fn reinforce_subtitle_topmost(window: &tauri::WebviewWindow, order_front: bool) {
    let _ = window.set_always_on_top(false);
    if let Err(err) = window.set_always_on_top(true) {
        log::warn!("设置字幕窗口置顶失败: {}", err);
    }

    if let Err(err) = window.set_visible_on_all_workspaces(true) {
        log::warn!("设置字幕窗口全空间可见失败: {}", err);
    }

    apply_macos_subtitle_fullscreen_behavior(window, order_front);

    #[cfg(target_os = "windows")]
    force_window_topmost(window);
}

pub(crate) fn set_subtitle_window_interactive(
    app_handle: &tauri::AppHandle,
    interactive: bool,
) -> Result<(), AppError> {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        window
            .set_ignore_cursor_events(!interactive)
            .map_err(|e| tauri_error("设置字幕窗口交互状态失败", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_main_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    let window = require_window(&app_handle, "main", "主窗口不存在")?;
    window
        .hide()
        .map_err(|e| tauri_error("隐藏主窗口失败", e))?;
    Ok("主窗口已隐藏".to_string())
}

pub async fn create_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    if app_handle.get_webview_window("subtitle").is_some() {
        return Ok("字幕窗口已存在".to_string());
    }

    let (logical_width, logical_height, x, y) = resolve_subtitle_layout(&app_handle);

    let window = tauri::WebviewWindowBuilder::new(
        &app_handle,
        "subtitle",
        tauri::WebviewUrl::App("/?window=subtitle".into()),
    )
    .title("字幕")
    .inner_size(logical_width, logical_height)
    .position(x, y)
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .focused(false)
    .resizable(false)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|e| tauri_error("创建字幕窗口失败", e))?;

    if let Err(err) = window.set_ignore_cursor_events(true) {
        log::warn!("设置字幕窗口鼠标穿透失败，继续运行: {}", err);
    }
    reinforce_subtitle_topmost(&window, false);

    Ok("字幕窗口已创建".to_string())
}

#[tauri::command]
pub async fn show_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    if app_handle.get_webview_window("subtitle").is_none() {
        create_subtitle_window(app_handle.clone()).await?;
    }

    let window = require_window(&app_handle, "subtitle", "字幕窗口创建后仍不存在")?;
    if let Err(err) = apply_subtitle_layout(&app_handle, &window) {
        log::warn!("刷新字幕窗口布局失败，继续尝试显示: {}", err);
    }

    window
        .show()
        .map_err(|e| tauri_error("显示字幕窗口失败", e))?;

    reinforce_subtitle_topmost(&window, true);

    if let Err(err) = set_subtitle_window_interactive(&app_handle, false) {
        log::warn!("重新设置字幕窗口鼠标穿透失败: {}", err);
    }

    // 递增"显示代"，使之前排队的 schedule_hide 全部作废
    let state = app_handle.state::<AppState>();
    state
        .recording
        .subtitle_show_gen
        .fetch_add(1, Ordering::Relaxed);

    Ok("字幕窗口已显示".to_string())
}

#[tauri::command]
pub async fn hide_subtitle_window(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    hide_subtitle_window_inner(&app_handle)
}

pub fn hide_subtitle_window_inner(app_handle: &tauri::AppHandle) -> Result<String, AppError> {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        let _ = set_subtitle_window_interactive(app_handle, false);
        window
            .hide()
            .map_err(|e| tauri_error("隐藏字幕窗口失败", e))?;
        Ok("字幕窗口已隐藏".to_string())
    } else {
        Ok("字幕窗口不存在".to_string())
    }
}
