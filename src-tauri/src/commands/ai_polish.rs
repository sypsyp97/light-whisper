use std::sync::atomic::Ordering;

use tauri_plugin_keyring::KeyringExt;

use crate::services::llm_provider;
use crate::state::AppState;

const KEYRING_SERVICE: &str = "light-whisper";

#[tauri::command]
pub async fn set_ai_polish_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
    api_key: String,
) -> Result<(), String> {
    state.ai_polish_enabled.store(enabled, Ordering::Release);

    // 获取当前活跃的 provider
    let provider = match state.user_profile.lock() {
        Ok(p) => p.llm_provider.active.clone(),
        Err(poisoned) => poisoned.into_inner().llm_provider.active.clone(),
    };

    let keyring_user = llm_provider::keyring_user_for_provider(&provider);

    // 存入 AppState（运行时使用）
    match state.ai_polish_api_key.lock() {
        Ok(mut key) => *key = api_key.clone(),
        Err(poisoned) => *poisoned.into_inner() = api_key.clone(),
    }

    // 持久化到系统密钥环
    if !api_key.is_empty() {
        if let Err(e) = app_handle
            .keyring()
            .set_password(KEYRING_SERVICE, keyring_user, &api_key)
        {
            log::warn!("保存 API Key 到系统密钥环失败: {}", e);
        }
    } else {
        let _ = app_handle
            .keyring()
            .delete_password(KEYRING_SERVICE, keyring_user);
    }

    log::info!(
        "AI 润色配置已更新: enabled={}, provider={}",
        enabled,
        provider
    );
    Ok(())
}

#[tauri::command]
pub async fn get_ai_polish_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let provider = match state.user_profile.lock() {
        Ok(p) => p.llm_provider.active.clone(),
        Err(poisoned) => poisoned.into_inner().llm_provider.active.clone(),
    };

    let keyring_user = llm_provider::keyring_user_for_provider(&provider);

    match app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, keyring_user)
    {
        Ok(Some(key)) => Ok(key),
        Ok(None) | Err(_) => Ok(String::new()),
    }
}
