use crate::services::{jev_service, profile_service};
use crate::state::user_profile::JevProvider;
use crate::state::AppState;

#[tauri::command]
pub async fn set_jev_config(
    state: tauri::State<'_, AppState>,
    enabled: bool,
    provider: JevProvider,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.jev.enabled = enabled;
        profile.jev.provider = provider;
    });
    Ok(())
}

#[tauri::command]
pub async fn get_jev_api_key(
    app_handle: tauri::AppHandle,
    provider: JevProvider,
) -> Result<String, String> {
    jev_service::load_api_key_for_provider(&app_handle, provider)
}

#[tauri::command]
pub async fn set_jev_api_key(
    app_handle: tauri::AppHandle,
    provider: JevProvider,
    api_key: String,
) -> Result<(), String> {
    jev_service::save_or_delete_api_key(&app_handle, provider, &api_key)
}
