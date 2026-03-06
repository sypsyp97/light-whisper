use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri_plugin_keyring::KeyringExt;

use crate::services::llm_provider;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelInfo {
    pub id: String,
    pub owned_by: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelListPayload {
    pub models: Vec<AiModelInfo>,
    pub source_url: String,
}

#[tauri::command]
pub async fn set_ai_polish_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
    api_key: String,
) -> Result<(), String> {
    state.ai_polish_enabled.store(enabled, Ordering::Release);

    let provider = state.active_llm_provider();
    let keyring_user = llm_provider::keyring_user_for_provider(&provider);

    // 存入 AppState（运行时使用）
    state.set_ai_polish_api_key(api_key.clone());

    // 持久化到系统密钥环
    if !api_key.is_empty() {
        if let Err(e) =
            app_handle
                .keyring()
                .set_password(llm_provider::KEYRING_SERVICE, keyring_user, &api_key)
        {
            log::warn!("保存 API Key 到系统密钥环失败: {}", e);
        }
    } else {
        let _ = app_handle
            .keyring()
            .delete_password(llm_provider::KEYRING_SERVICE, keyring_user);
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
    Ok(llm_provider::load_api_key_for_active_provider(
        &app_handle,
        state.inner(),
    ))
}

#[tauri::command]
pub async fn list_ai_models(
    state: tauri::State<'_, AppState>,
    provider: String,
    base_url: Option<String>,
    api_key: String,
) -> Result<AiModelListPayload, String> {
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("请先填写 API Key".to_string());
    }

    let source_url = llm_provider::models_url(&provider, base_url.as_deref());
    let response = state
        .http_client
        .get(&source_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("拉取模型列表失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("模型列表接口返回 {}: {}", status, body));
    }

    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("模型列表响应解析失败: {}", e))?;

    let models = payload["data"]
        .as_array()
        .ok_or_else(|| "模型列表格式不正确：缺少 data 数组".to_string())?
        .iter()
        .filter_map(|item| {
            let id = item["id"].as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            Some(AiModelInfo {
                id: id.to_string(),
                owned_by: item["owned_by"].as_str().map(|value| value.to_string()),
            })
        })
        .collect::<Vec<_>>();

    let mut models = models;
    models.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
    models.dedup_by(|a, b| a.id == b.id);

    Ok(AiModelListPayload { models, source_url })
}
