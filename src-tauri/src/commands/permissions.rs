use crate::services::permissions_service::{
    check_permission as do_check, parse_permission_kind, request_permission as do_request,
    PermissionStatus,
};

#[tauri::command]
pub async fn check_permission(kind: String) -> Result<PermissionStatus, String> {
    let parsed = parse_permission_kind(&kind)
        .ok_or_else(|| format!("unknown permission kind: {}", kind))?;
    Ok(do_check(parsed).await)
}

#[tauri::command]
pub async fn request_permission(kind: String) -> Result<PermissionStatus, String> {
    let parsed = parse_permission_kind(&kind)
        .ok_or_else(|| format!("unknown permission kind: {}", kind))?;
    Ok(do_request(parsed).await)
}
