use crate::utils::AppError;
use serde::Serialize;

#[cfg(target_os = "macos")]
use core_foundation::{
    base::TCFType, boolean::CFBoolean, dictionary::CFDictionary, string::CFString,
};
#[cfg(target_os = "macos")]
use core_foundation_sys::{base::Boolean, dictionary::CFDictionaryRef, string::CFStringRef};
#[cfg(target_os = "macos")]
use core_graphics::access::ScreenCaptureAccess;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub granted: bool,
    pub can_request: bool,
}

impl PermissionStatus {
    pub const fn granted() -> Self {
        Self {
            granted: true,
            can_request: false,
        }
    }
    pub const fn pending() -> Self {
        Self {
            granted: false,
            can_request: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PermissionKind {
    Microphone,
    Accessibility,
    Screen,
    Automation,
}

pub fn parse_permission_kind(kind: &str) -> Option<PermissionKind> {
    match kind {
        "microphone" => Some(PermissionKind::Microphone),
        "accessibility" => Some(PermissionKind::Accessibility),
        "screen" => Some(PermissionKind::Screen),
        "automation" => Some(PermissionKind::Automation),
        _ => None,
    }
}

pub async fn check_permission(kind: PermissionKind) -> PermissionStatus {
    match kind {
        PermissionKind::Microphone => check_microphone().await,
        PermissionKind::Accessibility => check_accessibility(),
        PermissionKind::Screen => check_screen_capture(),
        PermissionKind::Automation => check_automation().await,
    }
}

pub async fn request_permission(kind: PermissionKind) -> PermissionStatus {
    match kind {
        PermissionKind::Microphone => request_microphone().await,
        PermissionKind::Accessibility => request_accessibility(),
        PermissionKind::Screen => request_screen_capture(),
        PermissionKind::Automation => request_automation().await,
    }
}

#[cfg(target_os = "macos")]
async fn check_microphone() -> PermissionStatus {
    if probe_microphone_stream(false).await {
        PermissionStatus::granted()
    } else {
        PermissionStatus::pending()
    }
}

#[cfg(target_os = "macos")]
async fn request_microphone() -> PermissionStatus {
    if probe_microphone_stream(true).await {
        PermissionStatus::granted()
    } else {
        PermissionStatus::pending()
    }
}

#[cfg(target_os = "macos")]
async fn probe_microphone_stream(allow_play: bool) -> bool {
    tokio::task::spawn_blocking(move || {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let Some(device) = host.default_input_device() else {
            return false;
        };
        let Ok(config) = device.default_input_config() else {
            return false;
        };
        let stream_config: cpal::StreamConfig = config.into();
        let Ok(stream) = device.build_input_stream(
            &stream_config,
            |_data: &[f32], _info: &cpal::InputCallbackInfo| {},
            |_err| {},
            None,
        ) else {
            return false;
        };
        if allow_play {
            if stream.play().is_err() {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        drop(stream);
        true
    })
    .await
    .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn check_accessibility() -> PermissionStatus {
    if accessibility_trusted_with_prompt(false) {
        PermissionStatus::granted()
    } else {
        PermissionStatus::pending()
    }
}

#[cfg(target_os = "macos")]
fn request_accessibility() -> PermissionStatus {
    if accessibility_trusted_with_prompt(true) {
        PermissionStatus::granted()
    } else {
        PermissionStatus::pending()
    }
}

#[cfg(target_os = "macos")]
fn check_screen_capture() -> PermissionStatus {
    if ScreenCaptureAccess.preflight() {
        PermissionStatus::granted()
    } else {
        PermissionStatus::pending()
    }
}

#[cfg(target_os = "macos")]
fn request_screen_capture() -> PermissionStatus {
    if ScreenCaptureAccess.request() {
        PermissionStatus::granted()
    } else {
        PermissionStatus::pending()
    }
}

#[cfg(target_os = "macos")]
async fn check_automation() -> PermissionStatus {
    run_system_events_probe().await
}

#[cfg(target_os = "macos")]
async fn request_automation() -> PermissionStatus {
    run_system_events_probe().await
}

#[cfg(target_os = "macos")]
async fn run_system_events_probe() -> PermissionStatus {
    match tokio::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to count every process")
        .output()
        .await
    {
        Ok(output) if output.status.success() => PermissionStatus::granted(),
        _ => PermissionStatus::pending(),
    }
}

#[cfg(not(target_os = "macos"))]
async fn check_microphone() -> PermissionStatus {
    PermissionStatus::granted()
}
#[cfg(not(target_os = "macos"))]
async fn request_microphone() -> PermissionStatus {
    PermissionStatus::granted()
}
#[cfg(not(target_os = "macos"))]
fn check_accessibility() -> PermissionStatus {
    PermissionStatus::granted()
}
#[cfg(not(target_os = "macos"))]
fn request_accessibility() -> PermissionStatus {
    PermissionStatus::granted()
}
#[cfg(not(target_os = "macos"))]
fn check_screen_capture() -> PermissionStatus {
    PermissionStatus::granted()
}
#[cfg(not(target_os = "macos"))]
fn request_screen_capture() -> PermissionStatus {
    PermissionStatus::granted()
}
#[cfg(not(target_os = "macos"))]
async fn check_automation() -> PermissionStatus {
    PermissionStatus::granted()
}
#[cfg(not(target_os = "macos"))]
async fn request_automation() -> PermissionStatus {
    PermissionStatus::granted()
}

#[cfg(target_os = "macos")]
pub async fn ensure_accessibility_permission_for_input() -> Result<(), AppError> {
    let trusted = tokio::task::spawn_blocking(|| accessibility_trusted_with_prompt(false))
        .await
        .map_err(|err| AppError::Other(format!("检查辅助功能权限失败: {}", err)))?;
    if trusted {
        return Ok(());
    }

    let prompted = tokio::task::spawn_blocking(|| accessibility_trusted_with_prompt(true))
        .await
        .map_err(|err| AppError::Other(format!("请求辅助功能权限失败: {}", err)))?;
    if prompted {
        return Ok(());
    }

    let exe_path = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "未知路径".to_string());
    Err(AppError::Other(format!(
        "macOS 自动输入需要“辅助功能”权限。系统已尝试弹出授权框，请在 系统设置 > 隐私与安全性 > 辅助功能 中允许当前应用，然后彻底退出并重新打开后再试。当前应用路径: {}",
        exe_path
    )))
}

#[cfg(not(target_os = "macos"))]
pub async fn ensure_accessibility_permission_for_input() -> Result<(), AppError> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub async fn ensure_automation_permission_for_input() -> Result<(), AppError> {
    let output = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to count every process")
        .output()
        .await
        .map_err(|err| AppError::Other(format!("启动 osascript 失败: {}", err)))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if stderr.is_empty() {
        "系统没有返回更多错误信息".to_string()
    } else {
        stderr
    };
    Err(AppError::Other(format!(
        "macOS 自动输入还需要“自动化”权限。请在 系统设置 > 隐私与安全性 > 自动化 中允许当前应用控制 System Events，然后重试。系统返回: {}",
        detail
    )))
}

#[cfg(not(target_os = "macos"))]
pub async fn ensure_automation_permission_for_input() -> Result<(), AppError> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub async fn ensure_screen_capture_permission_for_assistant() -> Result<(), AppError> {
    let granted = tokio::task::spawn_blocking(|| {
        let access = ScreenCaptureAccess;
        if access.preflight() {
            true
        } else {
            access.request()
        }
    })
    .await
    .map_err(|err| AppError::Other(format!("请求屏幕录制权限失败: {}", err)))?;

    if granted {
        return Ok(());
    }

    Err(AppError::Other(
        "屏幕感知需要“屏幕录制”权限。系统已尝试弹出授权框，请在 系统设置 > 隐私与安全性 > 屏幕录制 中允许当前应用；授权后通常需要彻底退出并重新打开 app。".to_string(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub async fn ensure_screen_capture_permission_for_assistant() -> Result<(), AppError> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn accessibility_trusted_with_prompt(prompt: bool) -> bool {
    unsafe {
        let prompt_key: CFString = TCFType::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let options = CFDictionary::from_CFType_pairs(&[(prompt_key, CFBoolean::from(prompt))]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0
    }
}
