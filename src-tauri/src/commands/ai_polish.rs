use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::services::{codex_oauth_service, llm_provider, profile_service};
use crate::state::user_profile::{ApiFormat, OpenaiAuthMode};
use crate::state::AppState;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
    state
        .profile
        .ai_polish_enabled
        .store(enabled, Ordering::Release);

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelListFormat {
    Openai,
    CodexApi,
    CodexChatgpt,
}

impl ModelListFormat {
    fn is_codex(self) -> bool {
        matches!(self, Self::CodexApi | Self::CodexChatgpt)
    }

    fn includes_codex_only_models(self) -> bool {
        self == Self::CodexChatgpt
    }

    fn cache_partition(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::CodexApi => "codex-api",
            Self::CodexChatgpt => "codex-chatgpt",
        }
    }
}

#[derive(Clone)]
struct CachedCodexModels {
    fetched_at: Instant,
    models: Vec<AiModelInfo>,
}

static CODEX_MODELS_CACHE: OnceLock<Mutex<HashMap<String, CachedCodexModels>>> = OnceLock::new();
const CODEX_MODELS_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

// This value describes the Codex catalog wire contract implemented here. It
// advances only after the corresponding model metadata behavior is reviewed.
const CODEX_MODELS_CLIENT_VERSION: &str = "0.144.0";

fn codex_models_cache() -> &'static Mutex<HashMap<String, CachedCodexModels>> {
    CODEX_MODELS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn codex_models_cache_identity(token: &codex_oauth_service::ChatgptBearerToken) -> String {
    token
        .account_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let digest = Sha256::digest(token.access_token.as_bytes());
            digest.iter().map(|byte| format!("{byte:02x}")).collect()
        })
}

fn codex_models_cache_key(
    token: &codex_oauth_service::ChatgptBearerToken,
    format: ModelListFormat,
) -> String {
    format!(
        "{}:{}",
        format.cache_partition(),
        codex_models_cache_identity(token)
    )
}

fn cached_codex_models(cache_key: &str) -> Option<Vec<AiModelInfo>> {
    let cache = codex_models_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cached = cache.get(cache_key)?;
    if cached.fetched_at.elapsed() > CODEX_MODELS_CACHE_TTL {
        return None;
    }
    Some(cached.models.clone())
}

fn store_codex_models(cache_key: &str, models: &[AiModelInfo]) {
    codex_models_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            cache_key.to_string(),
            CachedCodexModels {
                fetched_at: Instant::now(),
                models: models.to_vec(),
            },
        );
}

fn remove_cached_codex_models_for_identity(identity: &str) {
    let mut cache = codex_models_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.remove(&format!(
        "{}:{identity}",
        ModelListFormat::CodexApi.cache_partition()
    ));
    cache.remove(&format!(
        "{}:{identity}",
        ModelListFormat::CodexChatgpt.cache_partition()
    ));
}

fn codex_models_source_url() -> String {
    format!(
        "{}?client_version={}",
        codex_oauth_service::CHATGPT_CODEX_MODELS_URL,
        CODEX_MODELS_CLIENT_VERSION
    )
}

fn codex_models_headers(
    token: &codex_oauth_service::ChatgptBearerToken,
) -> Result<reqwest::header::HeaderMap, String> {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token.access_token))
            .map_err(|err| format!("Codex 模型目录认证信息无效: {err}"))?,
    );
    headers.insert(
        HeaderName::from_static("originator"),
        HeaderValue::from_static(codex_oauth_service::ORIGINATOR),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(codex_oauth_service::CHATGPT_BEARER_USER_AGENT),
    );
    headers.insert(
        HeaderName::from_static("version"),
        HeaderValue::from_static(CODEX_MODELS_CLIENT_VERSION),
    );
    if let Some(account_id) = token
        .account_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        headers.insert(
            HeaderName::from_static("chatgpt-account-id"),
            HeaderValue::from_str(account_id)
                .map_err(|err| format!("Codex 账户标识无效: {err}"))?,
        );
    }
    Ok(headers)
}

fn openai_models_bearer(api_key: &str) -> String {
    codex_oauth_service::decode_oauth_api_key(api_key).unwrap_or_else(|| api_key.to_string())
}

fn parse_models_payload(
    payload: &serde_json::Value,
    format: ModelListFormat,
) -> Result<Vec<AiModelInfo>, String> {
    match format {
        ModelListFormat::Openai => {
            let mut models = payload["data"]
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
            models.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
            models.dedup_by(|a, b| a.id == b.id);
            Ok(models)
        }
        ModelListFormat::CodexApi | ModelListFormat::CodexChatgpt => {
            let mut models = payload["models"]
                .as_array()
                .ok_or_else(|| "Codex 模型目录格式不正确：缺少 models 数组".to_string())?
                .iter()
                .filter_map(|item| {
                    // Light Whisper sends selected models through the Responses
                    // API path, so Codex-only picker entries stay hidden until
                    // that inference route is supported here.
                    if item["visibility"].as_str() != Some("list") {
                        return None;
                    }
                    if !format.includes_codex_only_models()
                        && item["supported_in_api"].as_bool() != Some(true)
                    {
                        return None;
                    }
                    let id = item["slug"].as_str()?.trim();
                    if id.is_empty() {
                        return None;
                    }
                    Some((
                        item["priority"].as_i64().unwrap_or(i64::MAX),
                        AiModelInfo {
                            id: id.to_string(),
                            owned_by: Some("openai".to_string()),
                        },
                    ))
                })
                .collect::<Vec<_>>();
            models.sort_by(|(priority_a, model_a), (priority_b, model_b)| {
                priority_a
                    .cmp(priority_b)
                    .then_with(|| model_a.id.to_lowercase().cmp(&model_b.id.to_lowercase()))
            });
            let mut seen = HashSet::new();
            models.retain(|(_, model)| seen.insert(model.id.clone()));
            Ok(models.into_iter().map(|(_, model)| model).collect())
        }
    }
}

fn model_list_http_error(
    status: reqwest::StatusCode,
    body: &str,
    format: ModelListFormat,
) -> String {
    if format.is_codex() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return "Codex 登录已过期，请重新登录后再拉取模型列表".to_string();
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return "当前 ChatGPT 账户或工作区无权读取 Codex 模型目录".to_string();
        }
    }
    format!("模型列表接口返回 {}: {}", status, body)
}

fn cached_codex_fallback(
    cache_key: Option<&str>,
    source_url: &str,
    error: &str,
) -> Option<AiModelListPayload> {
    let models = cache_key.and_then(cached_codex_models)?;
    log::warn!("Codex 模型目录刷新失败，继续使用最近成功目录: {error}");
    Some(AiModelListPayload {
        models,
        source_url: source_url.to_string(),
    })
}

fn codex_models_auth_context(
    provider: &str,
    resolved_api_key: &str,
    current_session_token: Option<codex_oauth_service::ChatgptBearerToken>,
) -> (Option<codex_oauth_service::ChatgptBearerToken>, bool) {
    if provider != "openai" || !codex_oauth_service::is_oauth_origin_auth(resolved_api_key) {
        return (None, false);
    }

    let token_from_resolved_key =
        codex_oauth_service::decode_chatgpt_bearer_token(resolved_api_key);
    let inference_uses_chatgpt_backend = token_from_resolved_key.is_some();
    (
        current_session_token.or(token_from_resolved_key),
        inference_uses_chatgpt_backend,
    )
}

#[tauri::command]
pub async fn list_ai_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider: String,
    base_url: Option<String>,
    api_key: String,
    force_refresh: bool,
    openai_auth_mode: Option<OpenaiAuthMode>,
) -> Result<AiModelListPayload, String> {
    let api_key = codex_oauth_service::resolve_api_key_for_provider_with_auth_mode(
        &app_handle,
        state.inner(),
        &provider,
        &api_key,
        openai_auth_mode,
    )
    .await?;
    if api_key.is_empty() {
        return Err("请先填写 API Key 或完成 OpenAI Codex 登录".to_string());
    }

    let (chatgpt_token, inference_uses_chatgpt_backend) = codex_models_auth_context(
        &provider,
        &api_key,
        codex_oauth_service::current_chatgpt_bearer_token(state.inner()),
    );

    let config = state.llm_provider_config();
    let is_anthropic = config
        .custom_providers
        .iter()
        .find(|p| p.id == provider)
        .is_some_and(|p| p.api_format == ApiFormat::Anthropic);

    let model_list_format = if chatgpt_token.is_some() && inference_uses_chatgpt_backend {
        ModelListFormat::CodexChatgpt
    } else if chatgpt_token.is_some() {
        ModelListFormat::CodexApi
    } else {
        ModelListFormat::Openai
    };
    let source_url = if model_list_format.is_codex() {
        codex_models_source_url()
    } else {
        llm_provider::models_url(&config, &provider, base_url.as_deref())
    };
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

    let codex_cache_key = chatgpt_token
        .as_ref()
        .map(|token| codex_models_cache_key(token, model_list_format));
    let codex_cache_identity = chatgpt_token.as_ref().map(codex_models_cache_identity);
    if !force_refresh {
        if let Some(models) = codex_cache_key.as_deref().and_then(cached_codex_models) {
            return Ok(AiModelListPayload { models, source_url });
        }
    }

    // 构建请求（Anthropic 用 x-api-key，其他用 Bearer）
    let mut req = state
        .http_client
        .get(&source_url)
        .timeout(std::time::Duration::from_secs(
            if model_list_format.is_codex() { 5 } else { 12 },
        ));
    if let Some(token) = chatgpt_token {
        req = req.headers(codex_models_headers(&token)?);
    } else if is_anthropic {
        req = req
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.header(
            "Authorization",
            format!("Bearer {}", openai_models_bearer(&api_key)),
        );
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
            let error = model_list_http_error(status, &body, model_list_format);
            if matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
                if let Some(identity) = codex_cache_identity.as_deref() {
                    remove_cached_codex_models_for_identity(identity);
                }
                return Err(error);
            }
            if let Some(fallback) =
                cached_codex_fallback(codex_cache_key.as_deref(), &source_url, &error)
            {
                return Ok(fallback);
            }
            return Err(error);
        }
        Err(e) => {
            let error = format!("拉取模型列表失败: {e}");
            if let Some(fallback) =
                cached_codex_fallback(codex_cache_key.as_deref(), &source_url, &error)
            {
                return Ok(fallback);
            }
            return Err(error);
        }
    };

    let payload: serde_json::Value = match response.json().await {
        Ok(payload) => payload,
        Err(e) => {
            let error = format!("模型列表响应解析失败: {e}");
            if let Some(fallback) =
                cached_codex_fallback(codex_cache_key.as_deref(), &source_url, &error)
            {
                return Ok(fallback);
            }
            return Err(error);
        }
    };

    let models = match parse_models_payload(&payload, model_list_format) {
        Ok(models) => models,
        Err(error) => {
            if let Some(fallback) =
                cached_codex_fallback(codex_cache_key.as_deref(), &source_url, &error)
            {
                return Ok(fallback);
            }
            return Err(error);
        }
    };
    if let Some(cache_key) = codex_cache_key.as_deref() {
        store_codex_models(cache_key, &models);
    }

    Ok(AiModelListPayload { models, source_url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_catalog_keeps_visible_api_models_in_server_priority_order() {
        let payload = serde_json::json!({
            "models": [
                {"slug": "gpt-5.6-terra", "visibility": "list", "supported_in_api": true, "priority": 2},
                {"slug": "internal-review", "visibility": "hide", "supported_in_api": true, "priority": 0},
                {"slug": "gpt-5.6-sol", "visibility": "list", "supported_in_api": true, "priority": 1},
                {"slug": "gpt-5.6-sol", "visibility": "list", "supported_in_api": true, "priority": 9},
                {"slug": "gpt-5.3-codex-spark", "visibility": "list", "supported_in_api": false, "priority": 3},
                {"slug": "gpt-5.6-luna", "visibility": "list", "supported_in_api": true, "priority": 3}
            ]
        });

        let ids = parse_models_payload(&payload, ModelListFormat::CodexApi)
            .expect("Codex payload should parse")
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]);
    }

    #[test]
    fn chatgpt_bearer_catalog_keeps_visible_codex_only_models() {
        let payload = serde_json::json!({
            "models": [
                {"slug": "gpt-5.6-sol", "visibility": "list", "supported_in_api": true, "priority": 1},
                {"slug": "gpt-5.3-codex-spark", "visibility": "list", "supported_in_api": false, "priority": 26}
            ]
        });

        let ids = parse_models_payload(&payload, ModelListFormat::CodexChatgpt)
            .expect("ChatGPT Codex payload should parse")
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["gpt-5.6-sol", "gpt-5.3-codex-spark"]);
    }

    #[test]
    fn oauth_derived_api_key_is_unwrapped_for_live_models_endpoint() {
        let wrapped = codex_oauth_service::encode_oauth_api_key("sk-oauth-derived")
            .expect("test API key should be wrapped");

        assert_eq!(openai_models_bearer(&wrapped), "sk-oauth-derived");
    }

    #[test]
    fn resolved_chatgpt_bearer_keeps_catalog_route_after_session_disappears() {
        let expected = codex_oauth_service::ChatgptBearerToken {
            access_token: "logout-race-token".to_string(),
            account_id: Some("logout-race-account".to_string()),
        };
        let wrapped = codex_oauth_service::encode_chatgpt_bearer_token(&expected)
            .expect("test bearer token should be wrapped");

        let (token, uses_chatgpt_backend) = codex_models_auth_context("openai", &wrapped, None);
        let token = token.expect("resolved bearer should remain the request snapshot");

        assert!(uses_chatgpt_backend);
        assert_eq!(token.access_token, expected.access_token);
        assert_eq!(token.account_id, expected.account_id);
    }

    #[test]
    fn codex_models_url_identifies_the_calling_client() {
        assert_eq!(
            codex_models_source_url(),
            format!(
                "{}?client_version={}",
                codex_oauth_service::CHATGPT_CODEX_MODELS_URL,
                CODEX_MODELS_CLIENT_VERSION
            )
        );
    }

    #[test]
    fn codex_catalog_rejects_non_catalog_payloads() {
        let error =
            parse_models_payload(&serde_json::json!({"data": []}), ModelListFormat::CodexApi)
                .expect_err("missing models array should fail loudly");

        assert!(error.contains("缺少 models 数组"));
    }

    #[test]
    fn codex_catalog_headers_include_account_scoped_auth() {
        let headers = codex_models_headers(&codex_oauth_service::ChatgptBearerToken {
            access_token: "chatgpt-access-token".to_string(),
            account_id: Some("account-123".to_string()),
        })
        .expect("valid token should produce headers");

        assert_eq!(
            headers[reqwest::header::AUTHORIZATION],
            "Bearer chatgpt-access-token"
        );
        assert_eq!(headers["chatgpt-account-id"], "account-123");
        assert_eq!(headers["originator"], codex_oauth_service::ORIGINATOR);
        assert_eq!(headers["version"], CODEX_MODELS_CLIENT_VERSION);
        assert_eq!(
            headers[reqwest::header::USER_AGENT],
            codex_oauth_service::CHATGPT_BEARER_USER_AGENT
        );
    }

    #[test]
    fn codex_catalog_auth_errors_require_reauthentication_or_permission() {
        assert!(model_list_http_error(
            reqwest::StatusCode::UNAUTHORIZED,
            "ignored",
            ModelListFormat::CodexApi
        )
        .contains("重新登录"));
        assert!(model_list_http_error(
            reqwest::StatusCode::FORBIDDEN,
            "ignored",
            ModelListFormat::CodexApi
        )
        .contains("无权"));
    }

    #[test]
    fn codex_cache_is_account_and_inference_route_scoped() {
        let token = codex_oauth_service::ChatgptBearerToken {
            access_token: "cache-test-token".to_string(),
            account_id: Some("cache-test-account".to_string()),
        };
        let api_key = codex_models_cache_key(&token, ModelListFormat::CodexApi);
        let chatgpt_key = codex_models_cache_key(&token, ModelListFormat::CodexChatgpt);
        let models = vec![AiModelInfo {
            id: "gpt-5.6-sol".to_string(),
            owned_by: Some("openai".to_string()),
        }];

        store_codex_models(&api_key, &models);

        assert_eq!(cached_codex_models(&api_key), Some(models.clone()));
        assert_eq!(cached_codex_models(&chatgpt_key), None);
        let fallback = cached_codex_fallback(
            Some(&api_key),
            codex_oauth_service::CHATGPT_CODEX_MODELS_URL,
            "temporary failure",
        )
        .expect("a recent successful catalog should remain available");
        assert_eq!(fallback.models, models);
        store_codex_models(&chatgpt_key, &fallback.models);
        remove_cached_codex_models_for_identity(&codex_models_cache_identity(&token));
        assert_eq!(cached_codex_models(&api_key), None);
        assert_eq!(cached_codex_models(&chatgpt_key), None);
    }

    #[test]
    fn codex_fallback_rejects_expired_catalogs() {
        let cache_key = "codex-api:expired-cache-test";
        codex_models_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                cache_key.to_string(),
                CachedCodexModels {
                    fetched_at: Instant::now() - CODEX_MODELS_CACHE_TTL - Duration::from_secs(1),
                    models: vec![AiModelInfo {
                        id: "stale-model".to_string(),
                        owned_by: Some("openai".to_string()),
                    }],
                },
            );

        assert!(cached_codex_fallback(
            Some(cache_key),
            codex_oauth_service::CHATGPT_CODEX_MODELS_URL,
            "temporary failure",
        )
        .is_none());
        codex_models_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(cache_key);
    }
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
