use crate::services::profile_service;
use crate::state::AppState;

#[tauri::command]
pub async fn set_assistant_hotkey(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    shortcut: Option<String>,
) -> Result<(), String> {
    let normalized = shortcut
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    crate::commands::hotkey::register_assistant_hotkey_inner(app_handle, normalized.clone())
        .map_err(|err| err.to_string())?;

    let (_, profile) = state.update_profile(|profile| {
        profile.assistant_hotkey = normalized;
    });
    profile_service::save_profile(&profile)
}

#[tauri::command]
pub async fn set_assistant_system_prompt(
    state: tauri::State<'_, AppState>,
    prompt: Option<String>,
) -> Result<(), String> {
    let prompt = prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let (_, profile) = state.update_profile(|profile| {
        profile.assistant_system_prompt = prompt;
    });
    profile_service::save_profile(&profile)
}

#[tauri::command]
pub async fn set_assistant_screen_context_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let (_, profile) = state.update_profile(|profile| {
        profile.assistant_screen_context_enabled = enabled;
    });
    profile_service::save_profile(&profile)
}
