use tauri_plugin_keyring::KeyringExt;

use crate::services::{llm_provider, profile_service};
use crate::state::user_profile::*;
use crate::state::AppState;

#[tauri::command]
pub async fn get_user_profile(state: tauri::State<'_, AppState>) -> Result<UserProfile, String> {
    let profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    Ok(profile.clone())
}

#[tauri::command]
pub async fn add_hot_word(
    state: tauri::State<'_, AppState>,
    text: String,
    weight: u8,
) -> Result<(), String> {
    let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    profile_service::add_hot_word(&mut profile, text, weight);
    profile_service::save_profile(&profile)
}

#[tauri::command]
pub async fn remove_hot_word(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<(), String> {
    let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    profile_service::remove_hot_word(&mut profile, &text);
    profile_service::save_profile(&profile)
}

#[tauri::command]
pub async fn set_llm_provider_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    active: String,
    custom_base_url: Option<String>,
    custom_model: Option<String>,
) -> Result<(), String> {
    {
        let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
        profile.llm_provider = LlmProviderConfig {
            active: active.clone(),
            custom_base_url,
            custom_model,
        };
        profile_service::save_profile(&profile)?;
    }

    // 从 keyring 加载新 provider 的 API Key 到内存
    let keyring_user = llm_provider::keyring_user_for_provider(&active);
    let new_key = app_handle
        .keyring()
        .get_password("light-whisper", keyring_user)
        .ok()
        .flatten()
        .unwrap_or_default();
    match state.ai_polish_api_key.lock() {
        Ok(mut key) => *key = new_key,
        Err(poisoned) => *poisoned.into_inner() = new_key,
    }

    Ok(())
}

#[tauri::command]
pub async fn export_user_profile(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&*profile).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub async fn import_user_profile(
    state: tauri::State<'_, AppState>,
    json_data: String,
) -> Result<(), String> {
    let imported: UserProfile =
        serde_json::from_str(&json_data).map_err(|e| format!("解析画像数据失败: {}", e))?;
    let mut profile = state.user_profile.lock().map_err(|e| e.to_string())?;
    *profile = imported;
    profile_service::save_profile(&profile)
}
