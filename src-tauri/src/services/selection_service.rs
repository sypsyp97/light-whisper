#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    #[serde(skip)]
    source_window: Option<isize>,
}

pub fn current_selection() -> Option<SelectionDetectedPayload> {
    CURRENT_SELECTION
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock()
        .clone()
}

pub fn current_selection_matches(version: u64, selected_text: &str) -> bool {
    let selection = CURRENT_SELECTION
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock();
    selection.as_ref().is_some_and(|selection| {
        selection_context_matches(selection, version, selected_text, foreground_window_id())
    })
}

fn selection_context_matches(
    selection: &SelectionDetectedPayload,
    version: u64,
    selected_text: &str,
    foreground_window: Option<isize>,
) -> bool {
    selection.version == version
        && selection.text == selected_text
        && selection.source_window == foreground_window
}

#[cfg(target_os = "windows")]
fn foreground_window_id() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let window = unsafe { GetForegroundWindow() };
    (!window.is_null()).then_some(window as isize)
}

#[cfg(not(target_os = "windows"))]
fn foreground_window_id() -> Option<isize> {
    None
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
    let source_window = foreground_window_id();
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
        source_window,
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
        selection_context_matches, Anchor, PlacementSide, SelectionDetectedPayload,
        SelectionGesture, SelectionScreenshotContext, WorkArea,
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
            source_window: Some(42),
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
    fn replacement_context_requires_the_exact_version_text_and_source_window() {
        let selection = SelectionDetectedPayload {
            version: 7,
            text: "selected text".to_string(),
            character_count: 13,
            source_window: Some(42),
        };

        assert!(selection_context_matches(
            &selection,
            7,
            "selected text",
            Some(42)
        ));
        assert!(!selection_context_matches(
            &selection,
            8,
            "selected text",
            Some(42)
        ));
        assert!(!selection_context_matches(
            &selection,
            7,
            "other text",
            Some(42)
        ));
        assert!(!selection_context_matches(
            &selection,
            7,
            "selected text",
            Some(99)
        ));
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

#[cfg(all(test, target_os = "windows"))]
mod selection_gesture_tests {
    use super::{
        capture_mouse_hook_event, Anchor, DoubleClickLimits, HookEvent, MouseSample,
        SelectionGesture, SelectionGestureTracker, SelectionInputActivity,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
        WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };

    const POINT: Anchor = Anchor { x: 640, y: 480 };
    const SOURCE_WINDOW: isize = 42;
    const OTHER_SOURCE_WINDOW: isize = 84;
    const DOUBLE_CLICK_LIMITS: DoubleClickLimits = DoubleClickLimits {
        max_delay_ms: 500,
        rectangle_width: 4,
        rectangle_height: 4,
    };

    fn sample(point: Anchor, scroll_generation: u64, time_ms: u32) -> MouseSample {
        MouseSample {
            point,
            scroll_generation,
            interruption_generation: 0,
            time_ms,
        }
    }

    fn click(
        tracker: &mut SelectionGestureTracker,
        point: Anchor,
        scroll_generation: u64,
        time_ms: u32,
        source_window: Option<isize>,
    ) -> Option<SelectionGesture> {
        tracker.begin(sample(point, scroll_generation, time_ms));
        tracker.finish(
            sample(point, scroll_generation, time_ms.wrapping_add(10)),
            source_window,
            DOUBLE_CLICK_LIMITS,
        )
    }

    #[test]
    fn wheel_assisted_selection_is_eligible_even_when_release_position_is_unchanged() {
        let mut tracker = SelectionGestureTracker::default();
        tracker.begin(sample(POINT, 7, 100));

        let gesture = tracker
            .finish(
                sample(POINT, 8, 150),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS,
            )
            .expect("wheel activity while the button is held indicates selection intent");
        assert_eq!((gesture.start.x, gesture.start.y), (POINT.x, POINT.y));
        assert_eq!((gesture.end.x, gesture.end.y), (POINT.x, POINT.y));
    }

    #[test]
    fn movement_below_the_existing_threshold_without_scroll_remains_ineligible() {
        let mut tracker = SelectionGestureTracker::default();
        tracker.begin(sample(POINT, 7, 100));

        assert!(tracker
            .finish(
                sample(
                    Anchor {
                        x: POINT.x + 3,
                        y: POINT.y + 3,
                    },
                    7,
                    150,
                ),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS
            )
            .is_none());
    }

    #[test]
    fn ordinary_drag_at_the_existing_threshold_remains_eligible() {
        for end in [
            Anchor {
                x: POINT.x + 4,
                y: POINT.y,
            },
            Anchor {
                x: POINT.x,
                y: POINT.y + 4,
            },
        ] {
            let mut tracker = SelectionGestureTracker::default();
            tracker.begin(sample(POINT, 7, 100));

            assert!(tracker
                .finish(
                    sample(end, 7, 150),
                    Some(SOURCE_WINDOW),
                    DOUBLE_CLICK_LIMITS,
                )
                .is_some());
        }
    }

    #[test]
    fn one_plain_click_remains_ineligible() {
        let mut tracker = SelectionGestureTracker::default();

        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn second_plain_click_in_the_same_window_emits_a_gesture() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());

        let gesture = click(&mut tracker, POINT, 7, 350, Some(SOURCE_WINDOW))
            .expect("the second click should make the native word selection eligible");
        assert_eq!((gesture.start.x, gesture.start.y), (POINT.x, POINT.y));
        assert_eq!((gesture.end.x, gesture.end.y), (POINT.x, POINT.y));
    }

    #[test]
    fn third_plain_click_remains_eligible_for_native_triple_click_selection() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 7, 200, Some(SOURCE_WINDOW)).is_some());
        assert!(click(&mut tracker, POINT, 7, 300, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn double_click_delay_is_inclusive_and_a_late_click_becomes_the_next_candidate() {
        let mut boundary_tracker = SelectionGestureTracker::default();
        assert!(click(&mut boundary_tracker, POINT, 7, 100, Some(SOURCE_WINDOW),).is_none());
        assert!(click(&mut boundary_tracker, POINT, 7, 600, Some(SOURCE_WINDOW),).is_some());

        let mut late_tracker = SelectionGestureTracker::default();
        assert!(click(&mut late_tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut late_tracker, POINT, 7, 601, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut late_tracker, POINT, 7, 800, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn double_click_rectangle_uses_its_full_width_and_height() {
        for point in [
            Anchor {
                x: POINT.x - 2,
                y: POINT.y - 2,
            },
            Anchor {
                x: POINT.x + 1,
                y: POINT.y + 1,
            },
        ] {
            let mut tracker = SelectionGestureTracker::default();
            assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
            assert!(click(&mut tracker, point, 7, 200, Some(SOURCE_WINDOW)).is_some());
        }

        for point in [
            Anchor {
                x: POINT.x - 3,
                y: POINT.y,
            },
            Anchor {
                x: POINT.x + 2,
                y: POINT.y,
            },
            Anchor {
                x: POINT.x,
                y: POINT.y - 3,
            },
            Anchor {
                x: POINT.x,
                y: POINT.y + 2,
            },
        ] {
            let mut tracker = SelectionGestureTracker::default();
            assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
            assert!(click(&mut tracker, point, 7, 200, Some(SOURCE_WINDOW)).is_none());
            assert!(click(&mut tracker, point, 7, 300, Some(SOURCE_WINDOW)).is_some());
        }
    }

    #[test]
    fn clicks_from_different_foreground_windows_do_not_pair() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 7, 200, Some(OTHER_SOURCE_WINDOW),).is_none());
        assert!(click(&mut tracker, POINT, 7, 300, Some(OTHER_SOURCE_WINDOW),).is_some());
    }

    #[test]
    fn scroll_between_clicks_breaks_the_pair() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 8, 200, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 8, 300, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn drag_selection_clears_the_previous_click_candidate() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());

        tracker.begin(sample(POINT, 7, 200));
        assert!(tracker
            .finish(
                sample(
                    Anchor {
                        x: POINT.x + 4,
                        y: POINT.y,
                    },
                    7,
                    250,
                ),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS,
            )
            .is_some());

        assert!(click(&mut tracker, POINT, 7, 300, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 7, 400, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn clear_removes_both_pending_and_previous_click_state() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
        tracker.clear();

        assert!(click(&mut tracker, POINT, 7, 200, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 7, 300, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn unmatched_button_up_clears_a_stale_click_candidate() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(tracker
            .finish(
                sample(POINT, 7, 150),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS,
            )
            .is_none());

        assert!(click(&mut tracker, POINT, 7, 200, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn double_click_time_handles_u32_wraparound() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 7, u32::MAX - 100, Some(SOURCE_WINDOW),).is_none());
        assert!(click(&mut tracker, POINT, 7, 50, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn scrolling_without_an_active_press_does_not_pollute_the_next_click() {
        let input_activity = SelectionInputActivity::default();
        assert!(
            capture_mouse_hook_event(WM_MOUSEWHEEL, POINT, 50, None, &input_activity,).is_none()
        );

        let mut tracker = SelectionGestureTracker::default();
        let Some(HookEvent::LeftDown(sample)) =
            capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 100, None, &input_activity)
        else {
            panic!("left-button down must be forwarded");
        };
        tracker.begin(sample);

        let Some(HookEvent::LeftUp(sample, source_window)) = capture_mouse_hook_event(
            WM_LBUTTONUP,
            POINT,
            150,
            Some(SOURCE_WINDOW),
            &input_activity,
        ) else {
            panic!("left-button up must be forwarded");
        };
        assert!(tracker
            .finish(sample, source_window, DOUBLE_CLICK_LIMITS)
            .is_none());
    }

    #[test]
    fn low_level_mouse_hook_records_vertical_and_horizontal_wheel_activity() {
        for wheel_message in [WM_MOUSEWHEEL, WM_MOUSEHWHEEL] {
            let input_activity = SelectionInputActivity::default();
            let mut tracker = SelectionGestureTracker::default();

            let Some(HookEvent::LeftDown(sample)) =
                capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 100, None, &input_activity)
            else {
                panic!("left-button down must be forwarded");
            };
            tracker.begin(sample);

            assert!(
                capture_mouse_hook_event(wheel_message, POINT, 125, None, &input_activity,)
                    .is_none()
            );

            let Some(HookEvent::LeftUp(sample, source_window)) = capture_mouse_hook_event(
                WM_LBUTTONUP,
                POINT,
                150,
                Some(SOURCE_WINDOW),
                &input_activity,
            ) else {
                panic!("left-button up must be forwarded");
            };
            assert!(tracker
                .finish(sample, source_window, DOUBLE_CLICK_LIMITS)
                .is_some());
        }
    }

    #[test]
    fn other_mouse_buttons_interrupt_a_double_click_sequence() {
        for interrupt_message in [WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN] {
            let input_activity = SelectionInputActivity::default();
            let mut tracker = SelectionGestureTracker::default();

            let Some(HookEvent::LeftDown(first_down)) =
                capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 100, None, &input_activity)
            else {
                panic!("left-button down must be forwarded");
            };
            tracker.begin(first_down);
            let Some(HookEvent::LeftUp(first_up, first_window)) = capture_mouse_hook_event(
                WM_LBUTTONUP,
                POINT,
                110,
                Some(SOURCE_WINDOW),
                &input_activity,
            ) else {
                panic!("left-button up must be forwarded");
            };
            assert!(tracker
                .finish(first_up, first_window, DOUBLE_CLICK_LIMITS)
                .is_none());

            assert!(
                capture_mouse_hook_event(interrupt_message, POINT, 150, None, &input_activity,)
                    .is_none()
            );

            let Some(HookEvent::LeftDown(second_down)) =
                capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 200, None, &input_activity)
            else {
                panic!("left-button down must be forwarded");
            };
            tracker.begin(second_down);
            let Some(HookEvent::LeftUp(second_up, second_window)) = capture_mouse_hook_event(
                WM_LBUTTONUP,
                POINT,
                210,
                Some(SOURCE_WINDOW),
                &input_activity,
            ) else {
                panic!("left-button up must be forwarded");
            };
            assert!(tracker
                .finish(second_up, second_window, DOUBLE_CLICK_LIMITS)
                .is_none());
        }
    }

    #[test]
    fn other_mouse_button_during_a_left_press_is_not_treated_as_scroll_selection() {
        let input_activity = SelectionInputActivity::default();
        let mut tracker = SelectionGestureTracker::default();
        let Some(HookEvent::LeftDown(sample)) =
            capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 100, None, &input_activity)
        else {
            panic!("left-button down must be forwarded");
        };
        tracker.begin(sample);

        assert!(
            capture_mouse_hook_event(WM_RBUTTONDOWN, POINT, 125, None, &input_activity,).is_none()
        );
        let Some(HookEvent::LeftUp(sample, source_window)) = capture_mouse_hook_event(
            WM_LBUTTONUP,
            Anchor {
                x: POINT.x + 4,
                y: POINT.y,
            },
            150,
            Some(SOURCE_WINDOW),
            &input_activity,
        ) else {
            panic!("left-button up must be forwarded");
        };
        assert!(tracker
            .finish(sample, source_window, DOUBLE_CLICK_LIMITS)
            .is_none());
    }

    #[test]
    fn low_level_mouse_hook_preserves_the_message_timestamp() {
        let input_activity = SelectionInputActivity::default();
        input_activity
            .scroll_generation
            .store(9, std::sync::atomic::Ordering::Relaxed);
        let Some(HookEvent::LeftDown(sample)) =
            capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 1_234, None, &input_activity)
        else {
            panic!("left-button down must be forwarded");
        };

        assert_eq!(sample.time_ms, 1_234);
        assert_eq!(sample.scroll_generation, 9);
    }

    #[test]
    fn low_level_mouse_hook_preserves_the_release_window_snapshot() {
        let input_activity = SelectionInputActivity::default();
        let Some(HookEvent::LeftUp(_, source_window)) = capture_mouse_hook_event(
            WM_LBUTTONUP,
            POINT,
            1_234,
            Some(SOURCE_WINDOW),
            &input_activity,
        ) else {
            panic!("left-button up must be forwarded");
        };

        assert_eq!(source_window, Some(SOURCE_WINDOW));
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
    LeftDown(MouseSample),
    LeftUp(MouseSample, Option<isize>),
    Dismiss,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
struct MouseSample {
    point: Anchor,
    scroll_generation: u64,
    interruption_generation: u64,
    time_ms: u32,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
struct SelectionInputActivity {
    scroll_generation: AtomicU64,
    interruption_generation: AtomicU64,
}

#[cfg(target_os = "windows")]
impl SelectionInputActivity {
    fn record_scroll(&self) {
        self.scroll_generation.fetch_add(1, Ordering::Relaxed);
    }

    fn interrupt_selection(&self) {
        self.interruption_generation.fetch_add(1, Ordering::Relaxed);
    }

    fn sample(&self, point: Anchor, time_ms: u32) -> MouseSample {
        MouseSample {
            point,
            scroll_generation: self.scroll_generation.load(Ordering::Relaxed),
            interruption_generation: self.interruption_generation.load(Ordering::Relaxed),
            time_ms,
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
struct DoubleClickLimits {
    max_delay_ms: u32,
    rectangle_width: i32,
    rectangle_height: i32,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
struct ClickCandidate {
    press: MouseSample,
    source_window: isize,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
struct SelectionGestureTracker {
    pending: Option<MouseSample>,
    previous_click: Option<ClickCandidate>,
}

#[cfg(target_os = "windows")]
impl SelectionGestureTracker {
    fn begin(&mut self, sample: MouseSample) {
        if self.pending.replace(sample).is_some() {
            self.previous_click = None;
        }
    }

    fn clear(&mut self) {
        self.pending = None;
        self.previous_click = None;
    }

    fn finish(
        &mut self,
        sample: MouseSample,
        source_window: Option<isize>,
        double_click_limits: DoubleClickLimits,
    ) -> Option<SelectionGesture> {
        let Some(start) = self.pending.take() else {
            self.previous_click = None;
            return None;
        };
        let moved = sample.point.x.abs_diff(start.point.x) >= MIN_DRAG_DISTANCE_PX as u32
            || sample.point.y.abs_diff(start.point.y) >= MIN_DRAG_DISTANCE_PX as u32;
        let scrolled = sample.scroll_generation != start.scroll_generation;
        let interrupted = sample.interruption_generation != start.interruption_generation;
        let gesture = SelectionGesture {
            start: start.point,
            end: sample.point,
        };
        if interrupted {
            self.previous_click = None;
            return None;
        }
        if moved || scrolled {
            self.previous_click = None;
            return Some(gesture);
        }

        let Some(source_window) = source_window else {
            self.previous_click = None;
            return None;
        };
        let current_click = ClickCandidate {
            press: start,
            source_window,
        };
        let is_double_click = self.previous_click.take().is_some_and(|previous_click| {
            clicks_form_double_click(previous_click, current_click, double_click_limits)
        });
        if is_double_click {
            self.previous_click = Some(current_click);
            Some(gesture)
        } else {
            self.previous_click = Some(current_click);
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn clicks_form_double_click(
    first: ClickCandidate,
    second: ClickCandidate,
    limits: DoubleClickLimits,
) -> bool {
    first.source_window == second.source_window
        && first.press.scroll_generation == second.press.scroll_generation
        && first.press.interruption_generation == second.press.interruption_generation
        && second.press.time_ms.wrapping_sub(first.press.time_ms) <= limits.max_delay_ms
        && point_inside_centered_rectangle(
            first.press.point,
            second.press.point,
            limits.rectangle_width,
            limits.rectangle_height,
        )
}

#[cfg(target_os = "windows")]
fn point_inside_centered_rectangle(center: Anchor, point: Anchor, width: i32, height: i32) -> bool {
    fn coordinate_inside_span(center: i32, value: i32, size: i32) -> bool {
        let size = i64::from(size.max(1));
        let start = i64::from(center) - size / 2;
        let value = i64::from(value);
        value >= start && value < start + size
    }

    coordinate_inside_span(center.x, point.x, width)
        && coordinate_inside_span(center.y, point.y, height)
}

#[cfg(target_os = "windows")]
fn windows_double_click_limits() -> DoubleClickLimits {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXDOUBLECLK, SM_CYDOUBLECLK,
    };

    unsafe {
        DoubleClickLimits {
            max_delay_ms: GetDoubleClickTime(),
            rectangle_width: GetSystemMetrics(SM_CXDOUBLECLK).max(1),
            rectangle_height: GetSystemMetrics(SM_CYDOUBLECLK).max(1),
        }
    }
}

#[cfg(target_os = "windows")]
static HOOK_ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static INPUT_ACTIVITY: SelectionInputActivity = SelectionInputActivity {
    scroll_generation: AtomicU64::new(0),
    interruption_generation: AtomicU64::new(0),
};
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, MSLLHOOKSTRUCT};
    if code >= 0 {
        let hook_data = &*(lparam as *const MSLLHOOKSTRUCT);
        let point = hook_data.pt;
        let anchor = Anchor {
            x: point.x,
            y: point.y,
        };
        let message = wparam as u32;
        let source_window = (message == windows_sys::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP)
            .then(foreground_window_id)
            .flatten();
        let event = capture_mouse_hook_event(
            message,
            anchor,
            hook_data.time,
            source_window,
            &INPUT_ACTIVITY,
        );
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
fn is_mouse_wheel_message(message: u32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_MOUSEHWHEEL, WM_MOUSEWHEEL};

    matches!(message, WM_MOUSEWHEEL | WM_MOUSEHWHEEL)
}

#[cfg(target_os = "windows")]
fn is_other_mouse_button_down(message: u32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };

    matches!(message, WM_MBUTTONDOWN | WM_RBUTTONDOWN | WM_XBUTTONDOWN)
}

#[cfg(target_os = "windows")]
fn capture_mouse_hook_event(
    message: u32,
    point: Anchor,
    time_ms: u32,
    source_window: Option<isize>,
    input_activity: &SelectionInputActivity,
) -> Option<HookEvent> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_LBUTTONDOWN, WM_LBUTTONUP};

    if is_mouse_wheel_message(message) {
        input_activity.record_scroll();
        return None;
    }
    if is_other_mouse_button_down(message) {
        input_activity.interrupt_selection();
        return None;
    }

    let sample = input_activity.sample(point, time_ms);
    match message {
        WM_LBUTTONDOWN => Some(HookEvent::LeftDown(sample)),
        WM_LBUTTONUP => Some(HookEvent::LeftUp(sample, source_window)),
        _ => None,
    }
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
            INPUT_ACTIVITY.interrupt_selection();
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
    let mut gesture_tracker = SelectionGestureTracker::default();
    let double_click_limits = windows_double_click_limits();
    let mut last_value = String::new();
    let mut last_seen = Instant::now() - Duration::from_secs(2);

    while let Ok(event) = receiver.recv() {
        match event {
            HookEvent::Dismiss => {
                gesture_tracker.clear();
                if let Err(error) = hide_selection_window(&app_handle) {
                    log::warn!("外部键盘事件隐藏划词助手失败: {error}");
                }
                cancel_active_request(&app_handle);
            }
            HookEvent::LeftDown(sample) => {
                let point = sample.point;
                if point_inside_overlay(&app_handle, point) {
                    gesture_tracker.clear();
                } else {
                    if let Err(error) = hide_selection_window(&app_handle) {
                        log::warn!("外部鼠标事件隐藏划词助手失败: {error}");
                    }
                    cancel_active_request(&app_handle);
                    gesture_tracker.begin(sample);
                }
            }
            HookEvent::LeftUp(sample, source_window) => {
                let point = sample.point;
                if point_inside_overlay(&app_handle, point) {
                    gesture_tracker.clear();
                    continue;
                }
                let Some(gesture) =
                    gesture_tracker.finish(sample, source_window, double_click_limits)
                else {
                    continue;
                };

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
                if let Err(error) = show_selection(&app_handle, gesture, text, screenshots) {
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
