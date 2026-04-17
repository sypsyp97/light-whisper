use std::sync::atomic::Ordering;

use serde::Serialize;

use crate::services::{codex_oauth_service, llm_provider, profile_service};
use crate::state::user_profile::ApiFormat;
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
    state.profile.ai_polish_enabled.store(enabled, Ordering::Release);

    let provider = state.active_llm_provider();
    let keyring_user = llm_provider::keyring_user_for_provider(&provider);

    state.set_ai_polish_api_key(api_key.clone());

    // 若助手与润色共享 provider，同步助手缓存
    let assistant_provider = state.with_profile(|p| p.llm_provider.resolve_assistant_provider());
    if assistant_provider == provider {
        state.set_assistant_api_key(api_key.clone());
    }

    llm_provider::save_or_delete_api_key(&app_handle, &keyring_user, &api_key);

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
pub async fn set_ai_polish_screen_context_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.ai_polish_screen_context_enabled = enabled;
    });
    Ok(())
}

/// Anthropic 硬编码模型列表
fn anthropic_models() -> Vec<AiModelInfo> {
    [
        "claude-opus-4-6",
        "claude-sonnet-4-6",
        "claude-haiku-4-5-20251001",
        "claude-sonnet-4-5-20250929",
        "claude-sonnet-4-20250514",
    ]
    .into_iter()
    .map(|id| AiModelInfo {
        id: id.to_string(),
        owned_by: Some("anthropic".to_string()),
    })
    .collect()
}

fn codex_oauth_models() -> Vec<AiModelInfo> {
    [
        "gpt-5.1-codex",
        "gpt-5.1-codex-max",
        "gpt-5.1-codex-mini",
        "gpt-5.2",
        "gpt-5.2-codex",
        "gpt-5.3-codex",
        "gpt-5.4",
        "gpt-5.4-mini",
    ]
    .into_iter()
    .map(|id| AiModelInfo {
        id: id.to_string(),
        owned_by: Some("openai".to_string()),
    })
    .collect()
}

#[tauri::command]
pub async fn list_ai_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider: String,
    base_url: Option<String>,
    api_key: String,
) -> Result<AiModelListPayload, String> {
    let api_key = codex_oauth_service::resolve_api_key_for_provider(
        &app_handle,
        state.inner(),
        &provider,
        &api_key,
    )
    .await?;
    if api_key.is_empty() {
        return Err("请先填写 API Key 或完成 OpenAI Codex 登录".to_string());
    }

    if provider == "openai" && codex_oauth_service::decode_chatgpt_bearer_token(&api_key).is_some() {
        return Ok(AiModelListPayload {
            models: codex_oauth_models(),
            source_url: codex_oauth_service::CHATGPT_CODEX_RESPONSES_URL.to_string(),
        });
    }

    let config = state.llm_provider_config();
    let is_anthropic = config
        .custom_providers
        .iter()
        .find(|p| p.id == provider)
        .is_some_and(|p| p.api_format == ApiFormat::Anthropic);

    let source_url = llm_provider::models_url(&config, &provider, base_url.as_deref());
    if source_url.is_empty() {
        if is_anthropic {
            return Ok(AiModelListPayload {
                models: anthropic_models(),
                source_url,
            });
        }
        return Ok(AiModelListPayload {
            models: vec![],
            source_url,
        });
    }

    // 构建请求（Anthropic 用 x-api-key，其他用 Bearer）
    let mut req = state
        .http_client
        .get(&source_url)
        .timeout(std::time::Duration::from_secs(12));
    if is_anthropic {
        req = req
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }
    req = req.header("Content-Type", "application/json");

    let response = match req.send().await {
        Ok(r) if r.status().is_success() => r,
        _ if is_anthropic => {
            // Anthropic API 查询失败（代理不支持等），回退硬编码
            return Ok(AiModelListPayload {
                models: anthropic_models(),
                source_url,
            });
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            return Err(format!("模型列表接口返回 {}: {}", status, body));
        }
        Err(e) => return Err(format!("拉取模型列表失败: {}", e)),
    };

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

#[tauri::command]
pub async fn set_assistant_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_key: String,
) -> Result<(), String> {
    let provider = state.with_profile(|p| p.llm_provider.resolve_assistant_provider());
    let keyring_user = llm_provider::keyring_user_for_provider(&provider);

    state.set_assistant_api_key(api_key.clone());

    llm_provider::save_or_delete_api_key(&app_handle, &keyring_user, &api_key);

    // 若与润色共享 provider，同步润色缓存
    let polish_provider = state.active_llm_provider();
    if provider == polish_provider {
        state.set_ai_polish_api_key(api_key);
    }

    log::info!("助手 API Key 已更新: provider={}", provider);
    Ok(())
}

#[tauri::command]
pub async fn get_assistant_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let provider = state.with_profile(|p| p.llm_provider.resolve_assistant_provider());
    Ok(llm_provider::load_api_key_for_provider(
        &app_handle,
        &provider,
    ))
}
