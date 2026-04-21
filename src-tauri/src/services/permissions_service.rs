use crate::utils::AppError;

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
