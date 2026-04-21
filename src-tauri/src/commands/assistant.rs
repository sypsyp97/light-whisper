use tauri_plugin_keyring::KeyringExt;

use crate::services::{llm_provider, profile_service};
use crate::state::user_profile::WebSearchProvider;
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
    if enabled {
        crate::services::permissions_service::ensure_screen_capture_permission_for_assistant()
            .await
            .map_err(|err| err.to_string())?;
    }

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.assistant_screen_context_enabled = enabled;
    });
    Ok(())
}

// ── 联网搜索 ────────────────────────────────────────────────────────

/// keyring 用户名：只有 Tavily 需要存 API Key
pub fn web_search_keyring_user(provider: &WebSearchProvider) -> &'static str {
    match provider {
        WebSearchProvider::Tavily => "web-search-tavily-key",
        // Exa MCP 免费无需 Key, ModelNative 用 LLM provider 自己的 Key
        _ => "web-search-key",
    }
}

#[tauri::command]
pub async fn set_web_search_config(
    state: tauri::State<'_, AppState>,
    enabled: bool,
    provider: WebSearchProvider,
    max_results: Option<u8>,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.web_search.enabled = enabled;
        profile.web_search.provider = provider;
        if let Some(n) = max_results {
            profile.web_search.max_results = n.clamp(1, 10);
        }
    });
    log::info!("联网搜索配置已更新: enabled={enabled}");
    Ok(())
}

#[tauri::command]
pub async fn set_web_search_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_key: String,
) -> Result<(), String> {
    state.set_web_search_api_key(api_key.clone());
    let keyring_user = web_search_keyring_user(&WebSearchProvider::Tavily);
    llm_provider::save_or_delete_api_key(&app_handle, keyring_user, &api_key);
    Ok(())
}

#[tauri::command]
pub async fn get_web_search_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let cached = state.read_web_search_api_key();
    if !cached.is_empty() {
        return Ok(cached);
    }
    let keyring_user = web_search_keyring_user(&WebSearchProvider::Tavily);
    let key = app_handle
        .keyring()
        .get_password(llm_provider::KEYRING_SERVICE, keyring_user)
        .ok()
        .flatten()
        .unwrap_or_default();
    if !key.is_empty() {
        state.set_web_search_api_key(key.clone());
    }
    Ok(key)
}
