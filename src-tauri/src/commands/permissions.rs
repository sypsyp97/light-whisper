use crate::services::permissions_service::{
    check_permission as do_check, open_settings_pane, parse_permission_kind,
    request_permission as do_request, PermissionStatus,
};
use crate::utils::AppError;

#[tauri::command]
pub async fn check_permission(kind: String) -> Result<PermissionStatus, String> {
    let parsed =
        parse_permission_kind(&kind).ok_or_else(|| format!("unknown permission kind: {}", kind))?;
    Ok(do_check(parsed).await)
}

#[tauri::command]
pub async fn request_permission(kind: String) -> Result<PermissionStatus, String> {
    let parsed =
        parse_permission_kind(&kind).ok_or_else(|| format!("unknown permission kind: {}", kind))?;
    Ok(do_request(parsed).await)
}

/// Open the macOS Privacy & Security pane corresponding to `kind`.
/// On non-macOS this resolves to a no-op `Ok(())` so the front-end can
/// always invoke it without platform branching.
#[tauri::command]
pub async fn open_permission_settings(kind: String) -> Result<(), AppError> {
    let parsed = parse_permission_kind(&kind)
        .ok_or_else(|| AppError::Other(format!("unknown permission kind: {}", kind)))?;
    open_settings_pane(parsed)
}
