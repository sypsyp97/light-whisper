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

#[cfg(target_os = "macos")]
type AXUIElementRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type AXValueRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type AXError = i32;
#[cfg(target_os = "macos")]
type AXValueType = u32;

#[cfg(target_os = "macos")]
const K_AX_ERROR_SUCCESS: AXError = 0;
#[cfg(target_os = "macos")]
const K_AX_VALUE_CGPOINT_TYPE: AXValueType = 1;
#[cfg(target_os = "macos")]
const K_AX_VALUE_CGSIZE_TYPE: AXValueType = 2;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: core_foundation_sys::string::CFStringRef,
        value: *mut core_foundation::base::CFTypeRef,
    ) -> AXError;
    fn AXValueGetValue(
        value: AXValueRef,
        the_type: AXValueType,
        value_ptr: *mut std::ffi::c_void,
    ) -> core_foundation_sys::base::Boolean;
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

#[cfg(target_os = "macos")]
fn find_frontmost_monitor(app_handle: &tauri::AppHandle) -> Option<tauri::Monitor> {
    let (center_x, center_y) = macos_focused_window_center()?;
    match app_handle.monitor_from_point(center_x, center_y) {
        Ok(monitor) => monitor,
        Err(err) => {
            log::warn!(
                "获取前台窗口所在显示器失败，字幕窗口将回退到光标显示器: {}",
                err
            );
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn find_frontmost_monitor(_app_handle: &tauri::AppHandle) -> Option<tauri::Monitor> {
    None
}

#[cfg(target_os = "macos")]
fn macos_focused_window_center() -> Option<(f64, f64)> {
    use core_foundation::{
        base::{CFRelease, CFTypeRef, TCFType},
        string::CFString,
    };
    use core_graphics::geometry::{CGPoint, CGSize};
    use std::ptr;

    unsafe fn copy_macos_ax_attribute(
        element: AXUIElementRef,
        attribute: core_foundation_sys::string::CFStringRef,
    ) -> Option<core_foundation::base::CFTypeRef> {
        let mut value: core_foundation::base::CFTypeRef = ptr::null_mut();
        let status = AXUIElementCopyAttributeValue(element, attribute, &mut value);
        if status == K_AX_ERROR_SUCCESS && !value.is_null() {
            Some(value)
        } else {
            None
        }
    }

    unsafe fn copy_macos_ax_cgpoint(
        element: AXUIElementRef,
        attribute: core_foundation_sys::string::CFStringRef,
    ) -> Option<CGPoint> {
        let value = copy_macos_ax_attribute(element, attribute)?;
        let mut point = CGPoint::default();
        let ok = AXValueGetValue(
            value as AXValueRef,
            K_AX_VALUE_CGPOINT_TYPE,
            &mut point as *mut _ as *mut std::ffi::c_void,
        ) != 0;
        CFRelease(value);
        ok.then_some(point)
    }

    unsafe fn copy_macos_ax_cgsize(
        element: AXUIElementRef,
        attribute: core_foundation_sys::string::CFStringRef,
    ) -> Option<CGSize> {
        let value = copy_macos_ax_attribute(element, attribute)?;
        let mut size = CGSize::default();
        let ok = AXValueGetValue(
            value as AXValueRef,
            K_AX_VALUE_CGSIZE_TYPE,
            &mut size as *mut _ as *mut std::ffi::c_void,
        ) != 0;
        CFRelease(value);
        ok.then_some(size)
    }

    unsafe {
        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            return None;
        }

        let focused_window_attr = CFString::from_static_string("AXFocusedWindow");
        let position_attr = CFString::from_static_string("AXPosition");
        let size_attr = CFString::from_static_string("AXSize");

        let focused_window =
            match copy_macos_ax_attribute(system_wide, focused_window_attr.as_concrete_TypeRef()) {
                Some(value) => value,
                None => {
                    CFRelease(system_wide as CFTypeRef);
                    return None;
                }
            };

        let position = copy_macos_ax_cgpoint(
            focused_window as AXUIElementRef,
            position_attr.as_concrete_TypeRef(),
        );
        let size = copy_macos_ax_cgsize(
            focused_window as AXUIElementRef,
            size_attr.as_concrete_TypeRef(),
        );

        CFRelease(focused_window);
        CFRelease(system_wide as CFTypeRef);

        let CGPoint { x, y } = position?;
        let CGSize { width, height } = size?;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        Some((x + width / 2.0, y + height / 2.0))
    }
}

/// 获取光标所在显示器（物理坐标比对）
fn find_cursor_monitor(app_handle: &tauri::AppHandle) -> Option<tauri::Monitor> {
    let position = match app_handle.cursor_position() {
        Ok(position) => position,
        Err(err) => {
            log::warn!("获取光标位置失败，字幕窗口将回退到主窗口显示器: {}", err);
            return None;
        }
    };

    match app_handle.monitor_from_point(position.x, position.y) {
        Ok(monitor) => monitor,
        Err(err) => {
            log::warn!(
                "获取光标所在显示器失败，字幕窗口将回退到主窗口显示器: {}",
                err
            );
            None
        }
    }
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
            behavior |= NSWindowCollectionBehavior::Transient;
            behavior |= NSWindowCollectionBehavior::IgnoresCycle;
            behavior &= !NSWindowCollectionBehavior::FullScreenNone;
            ns_window.setCollectionBehavior(behavior);
            ns_window.setLevel(NSScreenSaverWindowLevel);
            ns_window.setIgnoresMouseEvents(true);
            if order_front {
                ns_window.orderFrontRegardless();
            }
            log::info!(
                "已加固字幕窗口 macOS 全屏行为: level=screenSaver, collection_behavior={:#x}, order_front={}",
                behavior.bits(),
                order_front
            );
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

#[cfg(target_os = "macos")]
fn reset_macos_subtitle_window_for_active_space(app_handle: &tauri::AppHandle) {
    if let Some(window) = app_handle.get_webview_window("subtitle") {
        if let Err(err) = window.destroy() {
            log::warn!("销毁旧字幕窗口以重新绑定全屏 Space 失败: {}", err);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn reset_macos_subtitle_window_for_active_space(_app_handle: &tauri::AppHandle) {}

fn resolve_subtitle_layout(app_handle: &tauri::AppHandle) -> (f64, f64, f64, f64) {
    let monitor = find_frontmost_monitor(app_handle)
        .or_else(|| find_cursor_monitor(app_handle))
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
    fn macos_subtitle_behavior_uses_webview_nswindow_without_isa_swap() {
        let source = include_str!("window.rs");
        let behavior_start = source
            .find("fn apply_macos_subtitle_fullscreen_behavior_on_main")
            .expect("macOS raw NSWindow behavior should exist");
        let behavior = &source[behavior_start..];
        let behavior_end = behavior
            .find("#[cfg(target_os = \"macos\")]\nfn apply_macos_subtitle_fullscreen_behavior")
            .expect("raw NSWindow helper should end before scheduler");
        let body = &behavior[..behavior_end];

        assert!(body.contains("NSWindow"));
        assert!(!body.contains("NSPanel"));
        assert!(!body.contains("object_setClass"));
        assert!(!body.contains("NSWindowStyleMask::NonactivatingPanel"));
        assert!(body.contains("setHidesOnDeactivate(false)"));
        assert!(body.contains("setCanHide(false)"));
        assert!(body.contains("FullScreenAuxiliary"));
        assert!(body.contains("CanJoinAllSpaces"));
        assert!(body.contains("Stationary"));
        assert!(body.contains("Transient"));
        assert!(body.contains("IgnoresCycle"));
        assert!(body.contains("NSScreenSaverWindowLevel"));
        assert!(body.contains("setIgnoresMouseEvents(true)"));
        assert!(body.contains("orderFrontRegardless()"));
    }

    #[test]
    fn subtitle_layout_prefers_frontmost_then_cursor_monitor() {
        let source = include_str!("window.rs");
        let cursor_start = source
            .find("fn find_cursor_monitor(app_handle: &tauri::AppHandle)")
            .expect("cursor monitor helper should exist");
        let cursor_segment = &source[cursor_start..];
        let cursor_end = cursor_segment
            .find("#[cfg(target_os = \"macos\")]")
            .expect("cursor monitor helper should end before macOS window behavior");
        let cursor_body = &cursor_segment[..cursor_end];
        let layout_start = source
            .find("fn resolve_subtitle_layout(app_handle: &tauri::AppHandle)")
            .expect("subtitle layout resolver should exist");
        let layout_segment = &source[layout_start..];
        let layout_end = layout_segment
            .find("#[cfg(test)]")
            .expect("layout resolver should end before tests");
        let layout_body = &layout_segment[..layout_end];

        assert!(cursor_body.contains("cursor_position()"));
        assert!(cursor_body.contains("monitor_from_point(position.x, position.y)"));
        assert!(layout_body.contains("find_frontmost_monitor(app_handle)"));
        assert!(layout_body.contains("find_cursor_monitor(app_handle)"));
        assert!(
            layout_body
                .find("find_frontmost_monitor(app_handle)")
                .expect("frontmost monitor should be queried")
                < layout_body
                    .find("find_cursor_monitor(app_handle)")
                    .expect("cursor monitor should be queried"),
            "subtitle layout should prefer the focused/frontmost window before cursor fallback"
        );
        assert!(
            !cursor_body.contains("cfg(target_os = \"windows\")"),
            "cursor monitor selection must also run on macOS fullscreen Spaces"
        );
    }

    #[test]
    fn macos_frontmost_monitor_uses_accessibility_focused_window_frame() {
        let source = include_str!("window.rs");
        let helper_start = source
            .find("fn macos_focused_window_center() -> Option<(f64, f64)>")
            .expect("macOS focused-window helper should exist");
        let helper_segment = &source[helper_start..];
        let helper_end = helper_segment
            .find("/// 获取光标所在显示器")
            .expect("focused-window helper should end before cursor helper");
        let helper_body = &helper_segment[..helper_end];
        let monitor_start = source
            .find("fn find_frontmost_monitor(app_handle: &tauri::AppHandle)")
            .expect("frontmost monitor helper should exist");
        let monitor_segment = &source[monitor_start..];
        let monitor_end = monitor_segment
            .find("#[cfg(not(target_os = \"macos\"))]")
            .expect("frontmost monitor helper should end before non-macOS stub");
        let monitor_body = &monitor_segment[..monitor_end];

        assert!(helper_body.contains("AXFocusedWindow"));
        assert!(helper_body.contains("AXPosition"));
        assert!(helper_body.contains("AXSize"));
        assert!(helper_body.contains("K_AX_VALUE_CGPOINT_TYPE"));
        assert!(helper_body.contains("K_AX_VALUE_CGSIZE_TYPE"));
        assert!(monitor_body.contains("monitor_from_point(center_x, center_y)"));
    }

    #[test]
    fn macos_show_recreates_subtitle_window_for_active_space() {
        let source = include_str!("window.rs");
        let reset_start = source
            .find("fn reset_macos_subtitle_window_for_active_space")
            .expect("macOS subtitle Space reset helper should exist");
        let reset_segment = &source[reset_start..];
        let reset_end = reset_segment
            .find("#[cfg(not(target_os = \"macos\"))]")
            .expect("macOS reset helper should end before non-macOS stub");
        let reset_body = &reset_segment[..reset_end];
        let show_start = source
            .find("pub async fn show_subtitle_window")
            .expect("subtitle show command should exist");
        let show_segment = &source[show_start..];
        let show_end = show_segment
            .find("#[tauri::command]\npub async fn hide_subtitle_window")
            .expect("show command should end before hide command");
        let show_body = &show_segment[..show_end];

        assert!(reset_body.contains("window.destroy()"));
        assert!(show_body.contains("reset_macos_subtitle_window_for_active_space(&app_handle)"));
        assert!(
            show_body
                .find("reset_macos_subtitle_window_for_active_space(&app_handle)")
                .expect("reset should run in show command")
                < show_body
                    .find("create_subtitle_window(app_handle.clone()).await")
                    .expect("create should run after reset"),
            "macOS show should recreate the subtitle window before showing so it joins the active fullscreen Space"
        );
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
    reset_macos_subtitle_window_for_active_space(&app_handle);

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
