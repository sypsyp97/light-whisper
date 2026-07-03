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
    // First, a non-prompting trust check — if the user has already granted us,
    // we never bother them. AXIsProcessTrustedWithOptions caches per-process,
    // so re-querying after the user toggles the switch in System Settings
    // works as long as the cache is busted (we re-call on every paste).
    let trusted = tokio::task::spawn_blocking(|| accessibility_trusted_with_prompt(false))
        .await
        .map_err(|err| AppError::Other(format!("检查辅助功能权限失败: {}", err)))?;
    if trusted {
        return Ok(());
    }

    // Not trusted — fire the system prompt (also lazy, only the first call
    // shows the dialog). If the user grants in this run, we proceed; if they
    // dismiss/deny, we surface a structured error so the UI can render an
    // "Open Settings" button rather than a multi-line paragraph.
    let prompted = tokio::task::spawn_blocking(|| accessibility_trusted_with_prompt(true))
        .await
        .map_err(|err| AppError::Other(format!("请求辅助功能权限失败: {}", err)))?;
    if prompted {
        return Ok(());
    }

    Err(permission_denied(
        PermissionKind::Accessibility,
        "需要「辅助功能」权限才能将转写结果粘贴到目标应用。点击右侧打开系统设置授权后再试。",
    ))
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

    Err(permission_denied(
        PermissionKind::Automation,
        "需要「自动化」权限才能让 System Events 帮你粘贴。点击右侧打开系统设置授权后再试。",
    ))
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

    Err(permission_denied(
        PermissionKind::Screen,
        "屏幕感知需要「屏幕录制」权限。点击右侧打开系统设置授权后再试。",
    ))
}

#[cfg(not(target_os = "macos"))]
pub async fn ensure_screen_capture_permission_for_assistant() -> Result<(), AppError> {
    Ok(())
}

/// Pre-flight microphone permission check for the recording entry path. We
/// run this BEFORE spinning the audio thread so the user gets one structured
/// "permission needed" error up-front instead of a cryptic "no input device"
/// failure two seconds in. On non-macOS this is a no-op.
#[cfg(target_os = "macos")]
pub async fn ensure_microphone_permission_for_recording() -> Result<(), AppError> {
    if check_microphone().await.granted {
        return Ok(());
    }
    // The first prompt also serves as the request; if the user has already
    // denied in TCC we still surface the structured error so the UI can
    // offer a deeplink instead of looping the prompt forever.
    if request_microphone().await.granted {
        return Ok(());
    }
    Err(permission_denied(
        PermissionKind::Microphone,
        "需要「麦克风」权限才能录音。点击右侧打开系统设置授权后再试。",
    ))
}

#[cfg(not(target_os = "macos"))]
pub async fn ensure_microphone_permission_for_recording() -> Result<(), AppError> {
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

/// The lower-case slug we use in IPC payloads + as the i18n key suffix. This
/// MUST match the strings accepted by `parse_permission_kind` so the
/// front-end / back-end share one vocabulary.
pub fn permission_kind_tag(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => "microphone",
        PermissionKind::Accessibility => "accessibility",
        PermissionKind::Screen => "screen",
        PermissionKind::Automation => "automation",
    }
}

/// macOS Privacy & Security deeplink for the matching pane. The UI uses this
/// to open the right tab with one click instead of telling the user to
/// navigate System Settings manually. On non-macOS the deeplink is unused
/// (returned anyway so the IPC shape stays consistent).
pub fn permission_settings_url(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Microphone => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        PermissionKind::Accessibility => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        PermissionKind::Screen => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        }
        PermissionKind::Automation => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
        }
    }
}

/// Build a structured `AppError::PermissionDenied` for the given kind. The
/// `message` is the short prose the UI shows; the deeplink is attached
/// automatically so callers don't have to remember it.
pub fn permission_denied(kind: PermissionKind, message: impl Into<String>) -> AppError {
    AppError::PermissionDenied {
        kind: permission_kind_tag(kind).to_string(),
        settings_url: permission_settings_url(kind).to_string(),
        message: message.into(),
    }
}

/// Open the macOS Privacy & Security pane that controls `kind`. On non-macOS
/// the function is a no-op (returns Ok) so frontend code can call it
/// uniformly without platform branching.
#[cfg(target_os = "macos")]
pub fn open_settings_pane(kind: PermissionKind) -> Result<(), AppError> {
    let url = permission_settings_url(kind);
    let status = std::process::Command::new("open")
        .arg(url)
        .status()
        .map_err(|err| AppError::Other(format!("启动 open 命令失败: {}", err)))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "打开系统设置失败 (exit={:?})",
            status.code()
        )))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn open_settings_pane(_kind: PermissionKind) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Permission-mapping contract tests.
    //!
    //! These pin the slug + deeplink table so that `parse_permission_kind`,
    //! `permission_kind_tag`, and `permission_settings_url` stay in lockstep
    //! — the front-end relies on a single round-trippable name per
    //! permission kind.
    use super::*;

    fn round_trip(slug: &str) -> &'static str {
        let kind = parse_permission_kind(slug).expect("slug must parse");
        permission_kind_tag(kind)
    }

    #[test]
    fn permission_slugs_round_trip_through_parse_and_tag() {
        for slug in ["microphone", "accessibility", "screen", "automation"] {
            assert_eq!(
                round_trip(slug),
                slug,
                "permission_kind_tag(parse_permission_kind({slug:?})) must echo the slug",
            );
        }
    }

    #[test]
    fn parse_permission_kind_rejects_unknown_slugs() {
        assert!(parse_permission_kind("camera").is_none());
        assert!(parse_permission_kind("").is_none());
        assert!(
            parse_permission_kind("Microphone").is_none(),
            "case-sensitive"
        );
    }

    #[test]
    fn permission_settings_url_points_at_apple_privacy_pane() {
        // The deeplink format is `x-apple.systempreferences:com.apple.preference.security?Privacy_<Pane>`.
        // The `<Pane>` token must match the macOS Privacy & Security tab we
        // want — pin each one so we can't silently send users to the wrong
        // pane on a refactor.
        assert!(
            permission_settings_url(PermissionKind::Microphone).ends_with("Privacy_Microphone"),
        );
        assert!(permission_settings_url(PermissionKind::Accessibility)
            .ends_with("Privacy_Accessibility"),);
        assert!(
            permission_settings_url(PermissionKind::Screen).ends_with("Privacy_ScreenCapture"),
            "Screen permission lives in the Screen RECORDING pane (Privacy_ScreenCapture), \
             not Privacy_Screen — calling the wrong pane sends the user to a 404 settings tab.",
        );
        assert!(
            permission_settings_url(PermissionKind::Automation).ends_with("Privacy_Automation"),
        );
        for kind in [
            PermissionKind::Microphone,
            PermissionKind::Accessibility,
            PermissionKind::Screen,
            PermissionKind::Automation,
        ] {
            assert!(
                permission_settings_url(kind).starts_with(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_",
                ),
                "all settings URLs must use the x-apple.systempreferences scheme",
            );
        }
    }

    #[test]
    fn permission_denied_helper_attaches_kind_and_url() {
        let err = permission_denied(PermissionKind::Microphone, "麦克风权限尚未授予");
        match err {
            AppError::PermissionDenied {
                kind,
                settings_url,
                message,
            } => {
                assert_eq!(kind, "microphone");
                assert!(settings_url.ends_with("Privacy_Microphone"));
                assert_eq!(message, "麦克风权限尚未授予");
            }
            other => panic!(
                "permission_denied() must build AppError::PermissionDenied, got {:?}",
                other
            ),
        }
    }
}
