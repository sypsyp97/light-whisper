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

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.assistant_hotkey = normalized;
    });
    Ok(())
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

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.assistant_system_prompt = prompt;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_assistant_screen_context_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.assistant_screen_context_enabled = enabled;
    });
    Ok(())
}
