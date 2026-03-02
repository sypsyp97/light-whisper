use std::sync::atomic::Ordering;

use crate::state::AppState;

#[tauri::command]
pub async fn set_ai_polish_config(
    state: tauri::State<'_, AppState>,
    enabled: bool,
    api_key: String,
) -> Result<(), String> {
    state.ai_polish_enabled.store(enabled, Ordering::Release);
    match state.ai_polish_api_key.lock() {
        Ok(mut key) => *key = api_key,
        Err(poisoned) => *poisoned.into_inner() = api_key,
    }
    log::info!("AI 润色配置已更新: enabled={}", enabled);
    Ok(())
}
