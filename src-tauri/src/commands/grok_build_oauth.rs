use crate::services::grok_build_oauth_service;
use crate::state::AppState;

#[tauri::command]
pub async fn login_grok_build_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<grok_build_oauth_service::GrokBuildOauthStatus, String> {
    grok_build_oauth_service::login(&app_handle, state.inner()).await
}

#[tauri::command]
pub async fn start_grok_build_oauth_device_code(
    state: tauri::State<'_, AppState>,
) -> Result<grok_build_oauth_service::GrokBuildOauthDeviceCodeChallenge, String> {
    grok_build_oauth_service::start_device_code_login(state.inner()).await
}

#[tauri::command]
pub async fn complete_grok_build_oauth_device_code(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    challenge: grok_build_oauth_service::GrokBuildOauthDeviceCodeChallenge,
) -> Result<grok_build_oauth_service::GrokBuildOauthStatus, String> {
    grok_build_oauth_service::complete_device_code_login(&app_handle, state.inner(), challenge)
        .await
}

#[tauri::command]
pub async fn logout_grok_build_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    grok_build_oauth_service::logout(&app_handle, state.inner());
    Ok(())
}

#[tauri::command]
pub async fn get_grok_build_oauth_status(
    state: tauri::State<'_, AppState>,
) -> Result<grok_build_oauth_service::GrokBuildOauthStatus, String> {
    Ok(grok_build_oauth_service::status(state.inner()))
}
