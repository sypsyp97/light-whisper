use crate::services::codex_oauth_service;
use crate::state::AppState;

#[tauri::command]
pub async fn login_openai_codex_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<codex_oauth_service::OpenaiCodexOauthStatus, String> {
    codex_oauth_service::login(&app_handle, state.inner()).await
}

#[tauri::command]
pub async fn logout_openai_codex_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    codex_oauth_service::logout(&app_handle, state.inner());
    Ok(())
}

#[tauri::command]
pub async fn get_openai_codex_oauth_status(
    state: tauri::State<'_, AppState>,
) -> Result<codex_oauth_service::OpenaiCodexOauthStatus, String> {
    Ok(codex_oauth_service::status(state.inner()))
}
