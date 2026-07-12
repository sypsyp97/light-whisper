#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Manager;

use crate::services::screen_capture_service::{self, CapturedScreen};
use crate::state::AppState;

const OVERLAY_LABEL: &str = "selection-toolbar";
const TOOLBAR_WIDTH: f64 = 548.0;
const TOOLBAR_HEIGHT: f64 = 106.0;
const RESULT_WIDTH: f64 = 548.0;
const RESULT_HEIGHT: f64 = 356.0;
const SELECTION_SETTLE_MS: u64 = 110;
const MIN_DRAG_DISTANCE_PX: i32 = 4;
const DUPLICATE_WINDOW_MS: u64 = 700;
const CURSOR_GAP_LOGICAL: f64 = 12.0;
const EDGE_MARGIN_LOGICAL: f64 = 8.0;
const VERTICAL_DIRECTION_THRESHOLD_LOGICAL: f64 = 24.0;

#[derive(Debug, Clone, Copy)]
struct Anchor {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy)]
struct SelectionGesture {
    start: Anchor,
    end: Anchor,
}

#[derive(Debug, Clone, Copy)]
struct WorkArea {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlacementSide {
    Above,
    Below,
}

#[derive(Debug, Clone)]
struct SelectionScreenshotContext {
    version: u64,
    images: Vec<CapturedScreen>,
}

static CURRENT_SELECTION: OnceLock<parking_lot::Mutex<Option<SelectionDetectedPayload>>> =
    OnceLock::new();
static CURRENT_SELECTION_SCREENSHOTS: OnceLock<
    parking_lot::Mutex<Option<SelectionScreenshotContext>>,
> = OnceLock::new();
static SELECTION_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionDetectedPayload {
    pub version: u64,
    pub text: String,
    pub character_count: usize,
}

pub fn current_selection() -> Option<SelectionDetectedPayload> {
    CURRENT_SELECTION
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock()
        .clone()
}

pub fn current_selection_screenshots(selected_text: &str) -> Vec<CapturedScreen> {
    let Some(selection) = current_selection() else {
        return Vec::new();
    };
    let screenshots = CURRENT_SELECTION_SCREENSHOTS
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock();
    matching_selection_screenshots(&selection, screenshots.as_ref(), selected_text)
}

fn matching_selection_screenshots(
    selection: &SelectionDetectedPayload,
    context: Option<&SelectionScreenshotContext>,
    selected_text: &str,
) -> Vec<CapturedScreen> {
    if selection.text.trim() != selected_text.trim() {
        return Vec::new();
    }
    context
        .filter(|context| context.version == selection.version)
        .map(|context| context.images.clone())
        .unwrap_or_default()
}

fn clear_selection_screenshots() {
    CURRENT_SELECTION_SCREENSHOTS
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock()
        .take();
}

fn tauri_error(action: &str, error: impl std::fmt::Display) -> String {
    format!("{action}: {error}")
}

pub fn create_selection_window(app_handle: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) {
        apply_no_activate_style(&window);
        return Ok(());
    }

    let window = tauri::WebviewWindowBuilder::new(
        app_handle,
        OVERLAY_LABEL,
        tauri::WebviewUrl::App("/?window=selection".into()),
    )
    .title("划词助手")
    .inner_size(TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .resizable(false)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|error| tauri_error("创建划词助手窗口失败", error))?;

    apply_no_activate_style(&window);
    Ok(())
}

pub fn hide_selection_window(app_handle: &tauri::AppHandle) -> Result<(), String> {
    clear_selection_screenshots();
    let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) else {
        return Ok(());
    };

    let tauri_error = window.hide().err().map(|error| error.to_string());
    if let Some(error) = tauri_error.as_deref() {
        log::warn!("Tauri 隐藏划词助手窗口失败，将尝试 Win32 SW_HIDE: {error}");
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_HIDE};

        let hwnd = window
            .hwnd()
            .map_err(|error| format!("获取划词助手 HWND 失败: {error}"))?;
        unsafe {
            ShowWindow(hwnd.0, SW_HIDE);
        }
        if unsafe { IsWindowVisible(hwnd.0) } != 0 {
            let detail = tauri_error
                .map(|error| format!("Tauri={error}; Win32 SW_HIDE 后窗口仍可见"))
                .unwrap_or_else(|| "Win32 SW_HIDE 后窗口仍可见".to_string());
            log::error!("{detail}");
            return Err(detail);
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        tauri_error
            .map(|error| Err(format!("隐藏划词助手窗口失败: {error}")))
            .unwrap_or(Ok(()))
    }
}

pub fn start_selection_window_drag(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "划词助手窗口不存在".to_string())?;

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetCursorPos, SendMessageW, HTCAPTION, WM_NCLBUTTONDOWN,
        };

        let hwnd = window
            .hwnd()
            .map_err(|error| format!("获取划词助手 HWND 失败: {error}"))?;
        let mut cursor = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut cursor) == 0 {
                return window
                    .start_dragging()
                    .map_err(|error| tauri_error("启动划词窗口拖动失败", error));
            }
            ReleaseCapture();
            // WM_NCLBUTTONDOWN expects screen coordinates packed into LPARAM.
            // Passing 0 makes Tao treat the drag as originating at (0, 0),
            // which is unreliable on multi-monitor and scaled desktops.
            let cursor_lparam =
                (((cursor.y as u16 as u32) << 16) | cursor.x as u16 as u32) as isize;
            SendMessageW(hwnd.0, WM_NCLBUTTONDOWN, HTCAPTION as usize, cursor_lparam);
            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        window
            .start_dragging()
            .map_err(|error| tauri_error("启动划词窗口拖动失败", error))
    }
}

pub fn set_selection_window_expanded(
    app_handle: &tauri::AppHandle,
    expanded: bool,
) -> Result<(), String> {
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "划词助手窗口不存在".to_string())?;
    let current_position = window
        .outer_position()
        .map_err(|error| tauri_error("读取划词助手当前位置失败", error))?;
    let (logical_width, logical_height) = selection_window_size(expanded);
    let (x, y) = if let Some(monitor) = window
        .current_monitor()
        .map_err(|error| tauri_error("读取划词助手所在显示器失败", error))?
    {
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let scale = monitor.scale_factor();
        clamp_window_top_left(
            current_position.x,
            current_position.y,
            monitor_position.x,
            monitor_position.y,
            monitor_size.width as i32,
            monitor_size.height as i32,
            (logical_width * scale).round() as i32,
            (logical_height * scale).round() as i32,
        )
    } else {
        (current_position.x, current_position.y)
    };

    if expanded {
        window
            .set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize::new(
                420.0, 260.0,
            ))))
            .map_err(|error| tauri_error("设置划词助手最小尺寸失败", error))?;
    } else {
        window
            .set_min_size(None::<tauri::Size>)
            .map_err(|error| tauri_error("清除划词助手最小尺寸失败", error))?;
    }
    window
        .set_resizable(expanded)
        .map_err(|error| tauri_error("切换划词助手缩放状态失败", error))?;

    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            logical_width,
            logical_height,
        )))
        .map_err(|error| tauri_error("调整划词助手窗口尺寸失败", error))?;
    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )))
        .map_err(|error| tauri_error("保持划词助手窗口位置失败", error))?;
    show_without_activation(&window);
    Ok(())
}

fn show_selection(
    app_handle: &tauri::AppHandle,
    gesture: SelectionGesture,
    text: String,
    screenshots: Vec<CapturedScreen>,
) -> Result<(), String> {
    create_selection_window(app_handle)?;
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "划词助手窗口创建后不存在".to_string())?;
    position_window(app_handle, &window, gesture, false)?;
    let version = SELECTION_VERSION.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
    *CURRENT_SELECTION
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock() = Some(SelectionDetectedPayload {
        version,
        character_count: text.chars().count(),
        text,
    });
    *CURRENT_SELECTION_SCREENSHOTS
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock() = (!screenshots.is_empty()).then_some(SelectionScreenshotContext {
        version,
        images: screenshots,
    });
    show_without_activation(&window);
    Ok(())
}

fn position_window(
    app_handle: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    gesture: SelectionGesture,
    expanded: bool,
) -> Result<(), String> {
    let (logical_width, logical_height) = selection_window_size(expanded);
    let monitors = app_handle
        .available_monitors()
        .map_err(|error| tauri_error("读取显示器布局失败", error))?;
    let monitor = monitors
        .iter()
        .find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            gesture.end.x >= position.x
                && gesture.end.x < position.x + size.width as i32
                && gesture.end.y >= position.y
                && gesture.end.y < position.y + size.height as i32
        })
        .or_else(|| monitors.first());

    let (x, y) = if let Some(monitor) = monitor {
        let position = monitor.position();
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let width = (logical_width * scale).round() as i32;
        let height = (logical_height * scale).round() as i32;
        let fallback_area = WorkArea {
            x: position.x,
            y: position.y,
            width: size.width as i32,
            height: size.height as i32,
        };
        let area = monitor_work_area(gesture.end).unwrap_or(fallback_area);
        let (x, y, _) = compute_selection_window_position(gesture, area, width, height, scale);
        (x, y)
    } else {
        (
            gesture.end.x - (logical_width / 2.0).round() as i32,
            gesture.end.y + CURSOR_GAP_LOGICAL.round() as i32,
        )
    };

    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            logical_width,
            logical_height,
        )))
        .map_err(|error| tauri_error("调整划词助手窗口尺寸失败", error))?;
    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )))
        .map_err(|error| tauri_error("定位划词助手窗口失败", error))?;
    Ok(())
}

fn selection_window_size(expanded: bool) -> (f64, f64) {
    if expanded {
        (RESULT_WIDTH, RESULT_HEIGHT)
    } else {
        (TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    }
}

fn midpoint(first: i32, second: i32) -> i32 {
    ((first as i64 + second as i64).div_euclid(2)) as i32
}

fn compute_selection_window_position(
    gesture: SelectionGesture,
    area: WorkArea,
    window_width: i32,
    window_height: i32,
    scale: f64,
) -> (i32, i32, PlacementSide) {
    let gap = (CURSOR_GAP_LOGICAL * scale).round() as i32;
    let edge_margin = (EDGE_MARGIN_LOGICAL * scale).round() as i32;
    let direction_threshold = (VERTICAL_DIRECTION_THRESHOLD_LOGICAL * scale).round() as i32;

    // Keep the popup close to the release cursor, but pull its center one step
    // toward the selection midpoint so long drags do not leave it hanging off
    // the far edge of the selected text.
    let selection_mid_x = midpoint(gesture.start.x, gesture.end.x);
    let focus_x = midpoint(selection_mid_x, gesture.end.x);
    let desired_x = focus_x - window_width / 2;

    let below_y = gesture.end.y + gap;
    let above_y = gesture.end.y - window_height - gap;
    let margin_x = edge_margin.min((area.width - window_width).max(0) / 2);
    let margin_y = edge_margin.min((area.height - window_height).max(0) / 2);
    let top_limit = area.y + margin_y;
    let bottom_limit = area.y + area.height - margin_y;
    let fits_above = above_y >= top_limit;
    let fits_below = below_y + window_height <= bottom_limit;
    let prefers_above = gesture.end.y < gesture.start.y - direction_threshold;

    let side = if prefers_above {
        if fits_above || !fits_below {
            PlacementSide::Above
        } else {
            PlacementSide::Below
        }
    } else if fits_below || !fits_above {
        PlacementSide::Below
    } else {
        PlacementSide::Above
    };
    let desired_y = match side {
        PlacementSide::Above => above_y,
        PlacementSide::Below => below_y,
    };
    let (x, y) = clamp_window_top_left(
        desired_x,
        desired_y,
        area.x + margin_x,
        area.y + margin_y,
        area.width - margin_x * 2,
        area.height - margin_y * 2,
        window_width,
        window_height,
    );
    (x, y, side)
}

#[cfg(target_os = "windows")]
fn monitor_work_area(anchor: Anchor) -> Option<WorkArea> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: anchor.x,
                y: anchor.y,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        if monitor.is_null() {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some(WorkArea {
            x: info.rcWork.left,
            y: info.rcWork.top,
            width: info.rcWork.right - info.rcWork.left,
            height: info.rcWork.bottom - info.rcWork.top,
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn monitor_work_area(_anchor: Anchor) -> Option<WorkArea> {
    None
}

#[allow(clippy::too_many_arguments)]
fn clamp_window_top_left(
    desired_x: i32,
    desired_y: i32,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: i32,
    monitor_height: i32,
    window_width: i32,
    window_height: i32,
) -> (i32, i32) {
    let max_x = monitor_x + monitor_width - window_width;
    let max_y = monitor_y + monitor_height - window_height;
    (
        desired_x.clamp(monitor_x, max_x.max(monitor_x)),
        desired_y.clamp(monitor_y, max_y.max(monitor_y)),
    )
}

#[cfg(test)]
mod window_position_tests {
    use super::{
        clamp_window_top_left, compute_selection_window_position, matching_selection_screenshots,
        Anchor, PlacementSide, SelectionDetectedPayload, SelectionGesture,
        SelectionScreenshotContext, WorkArea,
    };
    use crate::services::screen_capture_service::CapturedScreen;

    const PRIMARY_WORK_AREA: WorkArea = WorkArea {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
    };

    #[test]
    fn screenshot_context_is_scoped_to_the_exact_selection_version_and_text() {
        let selection = SelectionDetectedPayload {
            version: 7,
            text: " selected text ".to_string(),
            character_count: 13,
        };
        let context = SelectionScreenshotContext {
            version: 7,
            images: vec![CapturedScreen {
                mime_type: "image/jpeg".to_string(),
                data_base64: "encoded".to_string(),
            }],
        };

        assert_eq!(
            matching_selection_screenshots(&selection, Some(&context), "selected text").len(),
            1
        );
        assert!(matching_selection_screenshots(&selection, Some(&context), "other").is_empty());
        assert!(matching_selection_screenshots(
            &selection,
            Some(&SelectionScreenshotContext {
                version: 6,
                images: context.images,
            }),
            "selected text",
        )
        .is_empty());
    }

    #[test]
    fn centers_near_the_release_cursor_but_nudges_toward_the_selection() {
        let (x, y, side) = compute_selection_window_position(
            SelectionGesture {
                start: Anchor { x: 400, y: 280 },
                end: Anchor { x: 700, y: 300 },
            },
            PRIMARY_WORK_AREA,
            548,
            106,
            1.0,
        );

        assert_eq!((x, y, side), (351, 312, PlacementSide::Below));
        assert_eq!(x + 548 / 2, 625);
    }

    #[test]
    fn upward_multiline_selection_places_the_popup_above_the_release_cursor() {
        assert_eq!(
            compute_selection_window_position(
                SelectionGesture {
                    start: Anchor { x: 700, y: 500 },
                    end: Anchor { x: 650, y: 250 },
                },
                PRIMARY_WORK_AREA,
                548,
                106,
                1.0,
            ),
            (388, 132, PlacementSide::Above)
        );
    }

    #[test]
    fn slight_same_line_vertical_jitter_does_not_flip_the_popup() {
        let (_, y, side) = compute_selection_window_position(
            SelectionGesture {
                start: Anchor { x: 400, y: 302 },
                end: Anchor { x: 700, y: 300 },
            },
            PRIMARY_WORK_AREA,
            548,
            106,
            1.0,
        );

        assert_eq!((y, side), (312, PlacementSide::Below));
    }

    #[test]
    fn flips_above_and_keeps_an_edge_margin_near_the_taskbar() {
        assert_eq!(
            compute_selection_window_position(
                SelectionGesture {
                    start: Anchor { x: 1800, y: 1000 },
                    end: Anchor { x: 1915, y: 1035 },
                },
                PRIMARY_WORK_AREA,
                548,
                106,
                1.0,
            ),
            (1364, 917, PlacementSide::Above)
        );
    }

    #[test]
    fn supports_negative_coordinate_work_areas() {
        assert_eq!(
            compute_selection_window_position(
                SelectionGesture {
                    start: Anchor { x: -1240, y: 35 },
                    end: Anchor { x: -1200, y: 40 },
                },
                WorkArea {
                    x: -1280,
                    y: 24,
                    width: 1280,
                    height: 984,
                },
                548,
                106,
                1.0,
            ),
            (-1272, 52, PlacementSide::Below)
        );
    }

    #[test]
    fn keeps_a_dragged_position_that_still_fits_after_resize() {
        assert_eq!(
            clamp_window_top_left(420, 180, 0, 0, 1920, 1080, 548, 356),
            (420, 180)
        );
    }

    #[test]
    fn moves_only_enough_to_keep_an_expanded_window_on_screen() {
        assert_eq!(
            clamp_window_top_left(1600, 900, 0, 0, 1920, 1080, 548, 356),
            (1372, 724)
        );
    }

    #[test]
    fn supports_monitors_with_negative_desktop_coordinates() {
        assert_eq!(
            clamp_window_top_left(-1800, -20, -1920, 0, 1920, 1080, 548, 356),
            (-1800, 0)
        );
    }
}

#[cfg(target_os = "windows")]
fn apply_no_activate_style(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            let style = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd.0,
                GWL_EXSTYLE,
                style | WS_EX_NOACTIVATE as isize | WS_EX_TOOLWINDOW as isize,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_no_activate_style(_window: &tauri::WebviewWindow) {}

#[cfg(target_os = "windows")]
fn show_without_activation(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SW_SHOWNOACTIVATE,
    };
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            ShowWindow(hwnd.0, SW_SHOWNOACTIVATE);
            SetWindowPos(
                hwnd.0,
                -1isize as _,
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn show_without_activation(window: &tauri::WebviewWindow) {
    let _ = window.show();
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
enum HookEvent {
    LeftDown(Anchor),
    LeftUp(Anchor),
    Dismiss,
}

#[cfg(target_os = "windows")]
static HOOK_ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static HOOK_SENDER: OnceLock<parking_lot::Mutex<Option<std::sync::mpsc::SyncSender<HookEvent>>>> =
    OnceLock::new();

#[cfg(target_os = "windows")]
fn hook_sender() -> &'static parking_lot::Mutex<Option<std::sync::mpsc::SyncSender<HookEvent>>> {
    HOOK_SENDER.get_or_init(|| parking_lot::Mutex::new(None))
}

#[cfg(target_os = "windows")]
pub fn start_selection_listener(app_handle: tauri::AppHandle) -> Result<(), String> {
    HOOK_ENABLED.store(true, Ordering::Release);
    let mut current_sender = hook_sender().lock();
    if current_sender.is_some() {
        return Ok(());
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);
    *current_sender = Some(sender);
    drop(current_sender);

    if let Err(error) = std::thread::Builder::new()
        .name("selection-worker".to_string())
        .spawn(move || run_selection_worker(app_handle, receiver))
    {
        *hook_sender().lock() = None;
        HOOK_ENABLED.store(false, Ordering::Release);
        return Err(format!("启动划词处理线程失败: {error}"));
    }

    if let Err(error) = std::thread::Builder::new()
        .name("selection-hook".to_string())
        .spawn(run_hook_message_loop)
    {
        *hook_sender().lock() = None;
        HOOK_ENABLED.store(false, Ordering::Release);
        return Err(format!("启动划词输入监听失败: {error}"));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn start_selection_listener(_app_handle: tauri::AppHandle) -> Result<(), String> {
    Ok(())
}

pub fn set_selection_listener_enabled(enabled: bool) {
    #[cfg(target_os = "windows")]
    HOOK_ENABLED.store(enabled, Ordering::Release);

    #[cfg(not(target_os = "windows"))]
    let _ = enabled;
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn mouse_hook(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDOWN, WM_LBUTTONUP,
    };
    if code >= 0 {
        let point = (*(lparam as *const MSLLHOOKSTRUCT)).pt;
        let anchor = Anchor {
            x: point.x,
            y: point.y,
        };
        let event = match wparam as u32 {
            WM_LBUTTONDOWN => Some(HookEvent::LeftDown(anchor)),
            WM_LBUTTONUP => Some(HookEvent::LeftUp(anchor)),
            _ => None,
        };
        let sender = if HOOK_ENABLED.load(Ordering::Acquire) {
            hook_sender().lock().clone()
        } else {
            None
        };
        if let (Some(sender), Some(event)) = (sender, event) {
            let _ = sender.try_send(event);
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn keyboard_hook(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, WM_KEYDOWN, WM_SYSKEYDOWN,
    };
    if code >= 0 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let event = &*(lparam as *const KBDLLHOOKSTRUCT);
        if event.flags & LLKHF_INJECTED == 0 {
            let sender = if HOOK_ENABLED.load(Ordering::Acquire) {
                hook_sender().lock().clone()
            } else {
                None
            };
            if let Some(sender) = sender {
                let _ = sender.try_send(HookEvent::Dismiss);
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn run_hook_message_loop() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        WH_KEYBOARD_LL, WH_MOUSE_LL,
    };
    unsafe {
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), std::ptr::null_mut(), 0);
        let keyboard =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0);
        if mouse.is_null() || keyboard.is_null() {
            log::error!(
                "安装划词全局输入监听失败: {}",
                std::io::Error::last_os_error()
            );
            if !mouse.is_null() {
                UnhookWindowsHookEx(mouse);
            }
            if !keyboard.is_null() {
                UnhookWindowsHookEx(keyboard);
            }
            *hook_sender().lock() = None;
            HOOK_ENABLED.store(false, Ordering::Release);
            return;
        }
        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(mouse);
        UnhookWindowsHookEx(keyboard);
        *hook_sender().lock() = None;
        HOOK_ENABLED.store(false, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn run_selection_worker(
    app_handle: tauri::AppHandle,
    receiver: std::sync::mpsc::Receiver<HookEvent>,
) {
    let mut left_down = None;
    let mut last_value = String::new();
    let mut last_seen = Instant::now() - Duration::from_secs(2);

    while let Ok(event) = receiver.recv() {
        match event {
            HookEvent::Dismiss => {
                if let Err(error) = hide_selection_window(&app_handle) {
                    log::warn!("外部键盘事件隐藏划词助手失败: {error}");
                }
                cancel_active_request(&app_handle);
            }
            HookEvent::LeftDown(point) => {
                if point_inside_overlay(&app_handle, point) {
                    left_down = None;
                } else {
                    if let Err(error) = hide_selection_window(&app_handle) {
                        log::warn!("外部鼠标事件隐藏划词助手失败: {error}");
                    }
                    cancel_active_request(&app_handle);
                    left_down = Some(point);
                }
            }
            HookEvent::LeftUp(point) => {
                if point_inside_overlay(&app_handle, point) {
                    left_down = None;
                    continue;
                }
                let Some(start) = left_down.take() else {
                    continue;
                };
                if (point.x - start.x).abs() < MIN_DRAG_DISTANCE_PX
                    && (point.y - start.y).abs() < MIN_DRAG_DISTANCE_PX
                {
                    continue;
                }

                let config = app_handle
                    .state::<AppState>()
                    .with_profile(|profile| profile.selection_assistant.clone());
                if !config.enabled {
                    continue;
                }
                let Some(source_app) = foreground_app_source(&config.excluded_apps) else {
                    continue;
                };
                let screenshot_task = if config.auto_screenshot {
                    match std::thread::Builder::new()
                        .name("selection-screenshot".to_string())
                        .spawn(move || {
                            screen_capture_service::capture_screen_context_at_point(
                                point.x, point.y,
                            )
                        }) {
                        Ok(task) => Some(task),
                        Err(error) => {
                            log::warn!("启动划词自动截图失败，回退纯文本: {error}");
                            None
                        }
                    }
                } else {
                    None
                };

                std::thread::sleep(Duration::from_millis(SELECTION_SETTLE_MS));
                let Some(text) = crate::commands::clipboard::grab_selected_text_robust(&app_handle)
                else {
                    continue;
                };
                let character_count = text.chars().count();
                let min_chars = config.min_chars.clamp(1, 100);
                let max_chars = config.max_chars.clamp(min_chars, 50_000);
                if !(min_chars..=max_chars).contains(&character_count) {
                    log::debug!(
                        "划词长度超出范围，已忽略: {character_count} (允许 {min_chars}-{max_chars})"
                    );
                    continue;
                }
                let dedupe_value = format!("{source_app}\0{text}");
                if dedupe_value == last_value
                    && last_seen.elapsed() < Duration::from_millis(DUPLICATE_WINDOW_MS)
                {
                    continue;
                }
                last_value = dedupe_value;
                last_seen = Instant::now();
                let screenshots = match screenshot_task {
                    Some(task) => match task.join() {
                        Ok(Ok(images)) => images,
                        Ok(Err(error)) => {
                            log::warn!("划词自动截图失败，回退纯文本: {error}");
                            Vec::new()
                        }
                        Err(_) => {
                            log::warn!("划词自动截图线程异常，回退纯文本");
                            Vec::new()
                        }
                    },
                    None => Vec::new(),
                };
                if let Err(error) = show_selection(
                    &app_handle,
                    SelectionGesture { start, end: point },
                    text,
                    screenshots,
                ) {
                    log::warn!("显示划词助手失败: {error}");
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn cancel_active_request(app_handle: &tauri::AppHandle) {
    if let Some(task) = app_handle
        .state::<AppState>()
        .ui
        .selection_cancel
        .lock()
        .take()
    {
        let _ = task.cancel.send(());
    }
}

#[cfg(target_os = "windows")]
fn point_inside_overlay(app_handle: &tauri::AppHandle, point: Anchor) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};

    let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) else {
        return false;
    };
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    if unsafe { IsWindowVisible(hwnd.0) } == 0 {
        return false;
    }
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let valid = unsafe { GetWindowRect(hwnd.0, &mut rect) != 0 };
    valid
        && point.x >= rect.left
        && point.x < rect.right
        && point.y >= rect.top
        && point.y < rect.bottom
}

#[cfg(target_os = "windows")]
fn foreground_app_source(excluded_apps: &[String]) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, GWL_STYLE,
        WS_CAPTION,
    };

    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }

        // Borderless monitor-sized windows are typically games or capture
        // overlays. Maximized normal applications retain WS_CAPTION and remain
        // eligible for selection.
        let mut rect: RECT = std::mem::zeroed();
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetWindowRect(window, &mut rect) != 0
            && !monitor.is_null()
            && GetMonitorInfoW(monitor, &mut info) != 0
            && GetWindowLongW(window, GWL_STYLE) as u32 & WS_CAPTION == 0
            && rect.left <= info.rcMonitor.left
            && rect.top <= info.rcMonitor.top
            && rect.right >= info.rcMonitor.right
            && rect.bottom >= info.rcMonitor.bottom
        {
            return None;
        }

        let mut process_id = 0;
        GetWindowThreadProcessId(window, &mut process_id);
        if process_id == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) != 0;
        CloseHandle(process);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        let name = path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(path.as_str())
            .to_ascii_lowercase();
        let filtered = excluded_apps
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .any(|value| !value.is_empty() && value == name);
        (!filtered).then_some(name)
    }
}
