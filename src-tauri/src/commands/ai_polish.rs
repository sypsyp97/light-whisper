use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::services::{
    codex_oauth_service, grok_build_oauth_service, llm_provider, profile_service,
};
use crate::state::user_profile::{ApiFormat, OpenaiAuthMode, XaiAuthMode};
use crate::state::AppState;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub owned_by: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelListPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub models: Vec<AiModelInfo>,
    pub source_url: String,
}

#[tauri::command]
pub async fn set_ai_polish_config<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    enabled: bool,
    api_key: Option<String>,
    provider: Option<String>,
) -> Result<(), String> {
    if let Some(api_key) = api_key {
        update_ai_polish_api_key(&app_handle, state.inner(), api_key, provider)?;
    }
    state
        .profile
        .ai_polish_enabled
        .store(enabled, Ordering::Release);

    log::info!("AI polish enabled state updated: {}", enabled);
    Ok(())
}

fn update_ai_polish_api_key<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    state: &AppState,
    api_key: String,
    provider: Option<String>,
) -> Result<(), String> {
    let provider = provider.unwrap_or_else(|| state.active_llm_provider());
    let keyring_user = llm_provider::keyring_user_for_provider(&provider);

    llm_provider::save_or_delete_api_key(app_handle, &keyring_user, &api_key)
}

#[tauri::command]
pub async fn set_ai_polish_api_key<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    api_key: String,
    provider: Option<String>,
) -> Result<(), String> {
    update_ai_polish_api_key(&app_handle, state.inner(), api_key, provider)
}

#[tauri::command]
pub async fn get_ai_polish_api_key<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    provider: Option<String>,
) -> Result<String, String> {
    let provider = provider.unwrap_or_else(|| state.active_llm_provider());
    Ok(llm_provider::load_api_key_for_provider(
        &app_handle,
        &provider,
    ))
}

#[tauri::command]
pub async fn set_ai_polish_screen_context_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.set_screen_context_enabled(enabled);
    });
    Ok(())
}

#[tauri::command]
pub async fn set_screen_context_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.set_screen_context_enabled(enabled);
    });
    Ok(())
}

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
        display_name: None,
        owned_by: Some("anthropic".to_string()),
    })
    .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelListFormat {
    Openai,
    ChatgptPlan,
    CodexApi,
    CodexChatgpt,
}

impl ModelListFormat {
    fn is_codex(self) -> bool {
        matches!(
            self,
            Self::CodexApi | Self::CodexChatgpt | Self::ChatgptPlan
        )
    }

    fn includes_codex_only_models(self) -> bool {
        matches!(self, Self::CodexChatgpt | Self::ChatgptPlan)
    }

    fn cache_partition(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::ChatgptPlan => "chatgpt-plan",
            Self::CodexApi => "codex-api",
            Self::CodexChatgpt => "codex-chatgpt",
        }
    }
}

#[derive(Clone)]
struct CachedCodexModels {
    fetched_at: Instant,
    models: Vec<AiModelInfo>,
    source_url: String,
}

static CODEX_MODELS_CACHE: OnceLock<Mutex<HashMap<String, CachedCodexModels>>> = OnceLock::new();
const CODEX_MODELS_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

const CODEX_MODELS_CLIENT_VERSION: &str = "0.169.0";
const CODEX_RELEASE_URL: &str = "https://registry.npmjs.org/@openai%2fcodex/latest";
static CODEX_VERSION_CACHE: tokio::sync::Mutex<Option<(tokio::time::Instant, String)>> =
    tokio::sync::Mutex::const_new(None);

fn catalog_version_or(previous: &str, release: Option<&str>) -> String {
    let latest = release.and_then(|value| semver::Version::parse(value).ok());
    let previous_version = semver::Version::parse(previous).expect("cached stable Codex version");
    match latest {
        Some(version)
            if version.pre.is_empty() && version.build.is_empty() && version > previous_version =>
        {
            version.to_string()
        }
        _ => previous.to_owned(),
    }
}

async fn resolve_catalog_version(
    client: &reqwest::Client,
    release_url: &str,
    cache: &tokio::sync::Mutex<Option<(tokio::time::Instant, String)>>,
    ttl: Duration,
) -> String {
    // Serialize release lookups across pickers. This cache contains only a public version.
    let mut cache = cache.lock().await;
    if let Some((checked_at, version)) = cache.as_ref() {
        if checked_at.elapsed() < ttl {
            return version.clone();
        }
    }
    #[derive(serde::Deserialize)]
    struct Release {
        version: String,
    }
    let release = match client
        .get(release_url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response.json::<Release>().await.ok(),
        _ => None,
    };
    let previous = cache
        .as_ref()
        .map_or(CODEX_MODELS_CLIENT_VERSION, |(_, version)| version.as_str());
    let version = catalog_version_or(
        previous,
        release.as_ref().map(|release| release.version.as_str()),
    );
    *cache = Some((tokio::time::Instant::now(), version.clone()));
    version
}

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

fn recent_codex_catalog(cache_key: &str, now: Instant) -> Option<CachedCodexModels> {
    let cache = codex_models_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cached = cache.get(cache_key)?;
    if now.duration_since(cached.fetched_at) > CODEX_MODELS_CACHE_TTL {
        return None;
    }
    Some(cached.clone())
}

fn cached_codex_models(
    cache_key: &str,
    now: Instant,
    source_url: &str,
) -> Option<Vec<AiModelInfo>> {
    recent_codex_catalog(cache_key, now)
        .filter(|cached| cached.source_url == source_url)
        .map(|cached| cached.models)
}

fn store_codex_models(cache_key: &str, models: &[AiModelInfo], source_url: &str) {
    codex_models_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            cache_key.to_string(),
            CachedCodexModels {
                fetched_at: Instant::now(),
                models: models.to_vec(),
                source_url: source_url.to_owned(),
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
    cache.remove(&format!("chatgpt-plan:{identity}"));
}

fn oauth_models_source_url(format: ModelListFormat, version: &str) -> String {
    let base_url = if format == ModelListFormat::ChatgptPlan {
        "https://api.openai.com/v1/models"
    } else {
        codex_oauth_service::CHATGPT_CODEX_MODELS_URL
    };

    format!("{}?client_version={}", base_url, version)
}

fn codex_models_headers(
    token: &codex_oauth_service::ChatgptBearerToken,
    version: &str,
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
        HeaderValue::from_str(version).map_err(|err| format!("Codex 模型目录版本无效: {err}"))?,
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
    grok_build_oauth_service::decode_grok_build_oauth_access_token(api_key)
        .or_else(|| codex_oauth_service::decode_oauth_api_key(api_key))
        .unwrap_or_else(|| api_key.to_string())
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
                        display_name: None,
                        owned_by: item["owned_by"].as_str().map(|value| value.to_string()),
                    })
                })
                .collect::<Vec<_>>();
            models.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
            models.dedup_by(|a, b| a.id == b.id);
            Ok(models)
        }
        ModelListFormat::CodexApi
        | ModelListFormat::CodexChatgpt
        | ModelListFormat::ChatgptPlan => {
            let mut models = payload["models"]
                .as_array()
                .ok_or_else(|| "Codex 模型目录格式不正确：缺少 models 数组".to_string())?
                .iter()
                .filter_map(|item| {
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
                            display_name: item["display_name"]
                                .as_str()
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .map(str::to_string),
                            owned_by: Some("openai".to_string()),
                        },
                    ))
                })
                .collect::<Vec<_>>();
            if format != ModelListFormat::ChatgptPlan {
                models.sort_by(|(priority_a, model_a), (priority_b, model_b)| {
                    priority_a
                        .cmp(priority_b)
                        .then_with(|| model_a.id.to_lowercase().cmp(&model_b.id.to_lowercase()))
                });
            }
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
        if format == ModelListFormat::ChatgptPlan {
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return "ChatGPT 登录或套餐授权已失效，请重新登录后再获取模型列表。".into();
            }
            if status == reqwest::StatusCode::FORBIDDEN {
                return "当前 ChatGPT 账户、工作区或地区未获准使用套餐模型目录。".into();
            }
        }
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
    error: &str,
    now: Instant,
) -> Option<AiModelListPayload> {
    let cached = recent_codex_catalog(cache_key?, now)?;
    log::warn!("Codex 模型目录刷新失败，继续使用最近成功目录: {error}");
    Some(AiModelListPayload {
        warning: Some(
            "模型目录刷新失败，当前显示最近成功获取的列表，请检查连接或登录状态后重试。".into(),
        ),
        models: cached.models,
        source_url: cached.source_url,
    })
}

fn codex_models_auth_context(
    provider: &str,
    resolved_api_key: &str,
    resolved_session_token: Option<codex_oauth_service::ChatgptBearerToken>,
) -> (Option<codex_oauth_service::ChatgptBearerToken>, bool) {
    if provider != "openai" || !codex_oauth_service::is_oauth_origin_auth(resolved_api_key) {
        return (None, false);
    }

    let token_from_resolved_key =
        codex_oauth_service::decode_chatgpt_bearer_token(resolved_api_key);
    let inference_uses_chatgpt_backend = token_from_resolved_key.is_some();
    (
        token_from_resolved_key.or(resolved_session_token),
        inference_uses_chatgpt_backend,
    )
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_ai_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider: String,
    base_url: Option<String>,
    api_key: String,
    force_refresh: bool,
    openai_auth_mode: Option<OpenaiAuthMode>,
    xai_auth_mode: Option<XaiAuthMode>,
) -> Result<AiModelListPayload, String> {
    let config = state.llm_provider_config();
    let auth = codex_oauth_service::resolve_provider_auth_with_auth_mode(
        &app_handle,
        state.inner(),
        &provider,
        &api_key,
        openai_auth_mode.or(config.openai_auth_mode),
        xai_auth_mode.or(config.xai_auth_mode),
    )
    .await?;
    let api_key = auth.api_key;
    if api_key.is_empty() {
        if provider == "xai" {
            return Err("请先填写 API Key 或完成 Grok Build 登录".to_string());
        }
        return Err("请先填写 API Key 或完成 OpenAI Codex 登录".to_string());
    }

    let (chatgpt_token, inference_uses_chatgpt_backend) =
        codex_models_auth_context(&provider, &api_key, auth.chatgpt_token);

    let is_anthropic = config
        .custom_providers
        .iter()
        .find(|p| p.id == provider)
        .is_some_and(|p| p.api_format == ApiFormat::Anthropic);

    let grok_oauth_token = grok_build_oauth_service::decode_grok_build_oauth_access_token(&api_key);
    let model_list_format = if codex_oauth_service::is_chatgpt_plan_auth(&api_key) {
        ModelListFormat::ChatgptPlan
    } else if chatgpt_token.is_some() && inference_uses_chatgpt_backend {
        ModelListFormat::CodexChatgpt
    } else if chatgpt_token.is_some() {
        ModelListFormat::CodexApi
    } else {
        ModelListFormat::Openai
    };
    let catalog_version = if model_list_format.is_codex() {
        Some(
            resolve_catalog_version(
                &state.http_client,
                CODEX_RELEASE_URL,
                &CODEX_VERSION_CACHE,
                CODEX_MODELS_CACHE_TTL,
            )
            .await,
        )
    } else {
        None
    };
    let source_url = if let Some(version) = &catalog_version {
        oauth_models_source_url(model_list_format, version)
    } else if grok_oauth_token.is_some() {
        grok_build_oauth_service::GROK_BUILD_MODELS_URL.to_string()
    } else if provider == "xai" {
        grok_build_oauth_service::XAI_API_MODELS_URL.to_string()
    } else {
        llm_provider::models_url(&config, &provider, base_url.as_deref())
    };
    if source_url.is_empty() {
        if is_anthropic {
            return Ok(AiModelListPayload {
                warning: None,
                models: anthropic_models(),
                source_url,
            });
        }
        return Ok(AiModelListPayload {
            warning: None,
            models: vec![],
            source_url,
        });
    }

    let codex_cache_key = chatgpt_token
        .as_ref()
        .map(|token| codex_models_cache_key(token, model_list_format));
    let codex_cache_identity = chatgpt_token.as_ref().map(codex_models_cache_identity);
    if !force_refresh {
        if let Some(models) = codex_cache_key
            .as_deref()
            .and_then(|key| cached_codex_models(key, Instant::now(), &source_url))
        {
            return Ok(AiModelListPayload {
                models,
                source_url,
                warning: None,
            });
        }
    }

    let mut req = state
        .http_client
        .get(&source_url)
        .timeout(std::time::Duration::from_secs(
            if model_list_format.is_codex() { 5 } else { 12 },
        ));
    if let Some(token) = chatgpt_token {
        req = req.headers(if model_list_format == ModelListFormat::ChatgptPlan {
            llm_provider::build_auth_headers(&ApiFormat::OpenaiCompat, &api_key)?
        } else {
            codex_models_headers(
                &token,
                catalog_version.as_deref().expect("Codex catalog version"),
            )?
        });
    } else if let Some(token) = grok_oauth_token {
        req = req.headers(grok_build_oauth_service::grok_cli_request_headers(&token)?);
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

    let retry_request = req.try_clone();
    let mut result = req.send().await;
    if result
        .as_ref()
        .is_ok_and(|r| r.status() == reqwest::StatusCode::UNAUTHORIZED)
    {
        if let Some(refreshed) = codex_oauth_service::recover_rejected_auth(
            &app_handle,
            state.inner(),
            &provider,
            &api_key,
        )
        .await?
        {
            if let Some(retry) = retry_request {
                let headers = if model_list_format.is_codex()
                    && model_list_format != ModelListFormat::ChatgptPlan
                {
                    let token = codex_oauth_service::decode_chatgpt_bearer_token(&refreshed)
                        .or_else(|| {
                            state.read_openai_codex_oauth_session().map(|s| {
                                codex_oauth_service::ChatgptBearerToken {
                                    access_token: s.access_token,
                                    account_id: s.account_id,
                                }
                            })
                        })
                        .ok_or("ChatGPT 登录已失效，请重新登录。")?;
                    codex_models_headers(
                        &token,
                        catalog_version.as_deref().expect("Codex catalog version"),
                    )?
                } else {
                    llm_provider::build_auth_headers(&ApiFormat::OpenaiCompat, &refreshed)?
                };
                result = retry.headers(headers).send().await;
            }
        }
    }
    let response = match result {
        Ok(r) if r.status().is_success() => r,
        _ if is_anthropic => {
            return Ok(AiModelListPayload {
                warning: Some(
                    "模型目录刷新失败，当前显示内置列表，请检查连接或 API 配置后重试。".into(),
                ),
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
                cached_codex_fallback(codex_cache_key.as_deref(), &error, Instant::now())
            {
                return Ok(fallback);
            }
            return Err(error);
        }
        Err(e) => {
            let error = format!("拉取模型列表失败: {e}");
            if let Some(fallback) =
                cached_codex_fallback(codex_cache_key.as_deref(), &error, Instant::now())
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
                cached_codex_fallback(codex_cache_key.as_deref(), &error, Instant::now())
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
                cached_codex_fallback(codex_cache_key.as_deref(), &error, Instant::now())
            {
                return Ok(fallback);
            }
            return Err(error);
        }
    };
    if model_list_format.is_codex() {
        let entries = payload["models"].as_array();
        log::info!(
            "模型目录 source={} received={} selectable={} entries={:?}",
            source_url,
            entries.map_or(0, Vec::len),
            models.len(),
            entries
                .map(|entries| entries
                    .iter()
                    .filter_map(|entry| {
                        Some((
                            entry["slug"].as_str()?,
                            entry["visibility"].as_str().unwrap_or("unspecified"),
                        ))
                    })
                    .collect::<Vec<_>>())
                .unwrap_or_default(),
        );
    }
    if let Some(cache_key) = codex_cache_key.as_deref() {
        store_codex_models(cache_key, &models, &source_url);
    }

    Ok(AiModelListPayload {
        models,
        source_url,
        warning: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_version_only_advances_to_canonical_stable_releases() {
        assert_eq!(CODEX_MODELS_CLIENT_VERSION, "0.169.0");
        let previous = "0.199.9";
        for release in [
            None,
            Some(""),
            Some("0.168.99"),
            Some("0.199.8"),
            Some("v1.0.0"),
            Some("1.0"),
            Some("1.0.0.0"),
            Some("1.0.0-rc.1"),
            Some("1.0.0+build"),
            Some("01.0.0"),
            Some("1.00.0"),
            Some("1.0.00"),
            Some("+1.0.0"),
            Some(" 1.0.0"),
            Some("1.0.0\r\n"),
        ] {
            assert_eq!(
                catalog_version_or(previous, release),
                previous,
                "{release:?}"
            );
        }
        for release in ["0.199.9", "0.199.10", "0.200.0", "1.0.0"] {
            assert_eq!(catalog_version_or(previous, Some(release)), release);
        }
    }

    #[tokio::test]
    async fn catalog_version_cache_shares_refresh_and_throttles_failures() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                let (status, body) = match counter.fetch_add(1, Ordering::SeqCst) {
                    0 | 2 => ("503 Unavailable", r#"{"version":"9.0.0"}"#),
                    1 => ("200 OK", r#"{"version":"0.200.1"}"#),
                    3 => ("200 OK", "malformed JSON"),
                    4 => ("200 OK", r#"{"version":123}"#),
                    _ => ("200 OK", r#"{"version":"1.0.0"}"#),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let cache = tokio::sync::Mutex::new(None);
        let ttl = Duration::from_secs(3600);
        let resolve = |ttl| resolve_catalog_version(&client, &url, &cache, ttl);
        let (first, second) = tokio::join!(resolve(ttl), resolve(ttl));
        assert_eq!((first.as_str(), second.as_str()), ("0.169.0", "0.169.0"));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(resolve(Duration::ZERO).await, "0.200.1");
        assert_eq!(resolve(ttl).await, "0.200.1");
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        for expected_count in 3..=5 {
            assert_eq!(resolve(Duration::ZERO).await, "0.200.1");
            assert_eq!(resolve(ttl).await, "0.200.1");
            assert_eq!(requests.load(Ordering::SeqCst), expected_count);
        }
        assert_eq!(resolve(Duration::ZERO).await, "1.0.0");
        assert_eq!(requests.load(Ordering::SeqCst), 6);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn catalog_version_refreshes_at_the_exact_expiry_boundary() {
        let checked_at = tokio::time::Instant::now();
        let ttl = Duration::from_secs(60);
        let cache = tokio::sync::Mutex::new(Some((checked_at, "1.0.0".to_string())));
        let client = reqwest::Client::new();
        // URL construction fails immediately on refresh, before a socket or timer is needed.
        let resolve = || resolve_catalog_version(&client, "invalid URL", &cache, ttl);

        tokio::time::advance(ttl - Duration::from_nanos(1)).await;
        assert_eq!(resolve().await, "1.0.0");
        assert_eq!(cache.lock().await.as_ref().unwrap().0, checked_at);

        tokio::time::advance(Duration::from_nanos(1)).await;
        assert_eq!(resolve().await, "1.0.0");
        assert_eq!(cache.lock().await.as_ref().unwrap().0, checked_at + ttl);
    }

    #[test]
    fn chatgpt_plan_catalog_preserves_server_order_without_api_allowlist() {
        let payload = serde_json::json!({"models":[
            {"slug":"future-model-z","visibility":"list","priority":99},
            {"slug":"hidden","visibility":"hidden"},
            {"slug":"future-model-a","visibility":"list","supported_in_api":false,"priority":0},
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list","supported_in_api":false},
            {"slug":"future-model-z","visibility":"list"}
        ]});
        let models = parse_models_payload(&payload, ModelListFormat::ChatgptPlan).unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["future-model-z", "future-model-a", "gpt-6.1-sol"]
        );
        assert_eq!(
            serde_json::to_value(&models[2]).unwrap()["displayName"],
            "GPT-6.1 Sol"
        );
    }

    #[test]
    fn codex_catalog_keeps_visible_api_models_in_server_priority_order() {
        let payload = serde_json::json!({
            "models": [
                {"slug": "future-compatible-model", "visibility": "list", "supported_in_api": true, "priority": 0},
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

        assert_eq!(
            ids,
            vec![
                "future-compatible-model",
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna"
            ]
        );
    }

    #[test]
    fn codex_catalog_keeps_visible_gpt6_models_in_server_order() {
        let payload = serde_json::json!({
            "models": [
                {"slug": "gpt-6.1-sol", "visibility": "list", "supported_in_api": true, "priority": 0},
                {"slug": "gpt-6-astra", "visibility": "list", "supported_in_api": true, "priority": 1},
                {"slug": "gpt-6-sol", "visibility": "list", "supported_in_api": true, "priority": 2},
                {"slug": "gpt-6-luna", "visibility": "list", "supported_in_api": true, "priority": 3},
                {"slug": "gpt-6-internal", "visibility": "hide", "supported_in_api": true, "priority": 0}
            ]
        });

        let ids = parse_models_payload(&payload, ModelListFormat::CodexApi)
            .expect("Codex payload should parse")
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();

        let expected = vec!["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna"];
        assert_eq!(ids, expected);
        let chatgpt_ids = parse_models_payload(&payload, ModelListFormat::CodexChatgpt)
            .expect("ChatGPT Codex payload should parse")
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        assert_eq!(chatgpt_ids, expected);
        assert_eq!(
            oauth_models_source_url(ModelListFormat::CodexChatgpt, "0.201.0"),
            format!(
                "{}?client_version=0.201.0",
                codex_oauth_service::CHATGPT_CODEX_MODELS_URL
            )
        );

        let headers = codex_models_headers(
            &codex_oauth_service::ChatgptBearerToken {
                access_token: "gpt6-catalog-token".to_string(),
                account_id: Some("gpt6-account".to_string()),
            },
            "0.201.0",
        )
        .expect("valid token should produce catalog headers");
        assert_eq!(headers["version"], "0.201.0");
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
    fn resolved_catalog_bearer_is_not_replaced_by_a_new_login() {
        let account_a = codex_oauth_service::ChatgptBearerToken {
            access_token: "account-a-token".into(),
            account_id: Some("account-a".into()),
        };
        let account_b = codex_oauth_service::ChatgptBearerToken {
            access_token: "account-b-token".into(),
            account_id: Some("account-b".into()),
        };
        let key = codex_oauth_service::encode_chatgpt_bearer_token(&account_a).unwrap();
        let (token, _) = codex_models_auth_context("openai", &key, Some(account_b));
        let token = token.unwrap();
        assert_eq!(token.access_token, account_a.access_token);
        assert_eq!(token.account_id, account_a.account_id);
    }

    #[test]
    fn codex_models_url_identifies_the_calling_client() {
        assert_eq!(
            oauth_models_source_url(ModelListFormat::CodexChatgpt, "1.2.3"),
            format!(
                "{}?client_version={}",
                codex_oauth_service::CHATGPT_CODEX_MODELS_URL,
                "1.2.3"
            )
        );
    }

    #[test]
    fn chatgpt_plan_catalog_identifies_client_on_public_api() {
        let request = reqwest::Client::new()
            .get(oauth_models_source_url(
                ModelListFormat::ChatgptPlan,
                "1.2.3",
            ))
            .build()
            .unwrap();
        assert_eq!(request.url().host_str(), Some("api.openai.com"));
        assert_eq!(request.url().path(), "/v1/models");
        assert_eq!(
            request
                .url()
                .query_pairs()
                .find(|(key, _)| key == "client_version")
                .map(|(_, value)| value.into_owned()),
            Some("1.2.3".to_string())
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
        let headers = codex_models_headers(
            &codex_oauth_service::ChatgptBearerToken {
                access_token: "chatgpt-access-token".to_string(),
                account_id: Some("account-123".to_string()),
            },
            "1.2.3",
        )
        .expect("valid token should produce headers");

        assert_eq!(
            headers[reqwest::header::AUTHORIZATION],
            "Bearer chatgpt-access-token"
        );
        assert_eq!(headers["chatgpt-account-id"], "account-123");
        assert_eq!(headers["originator"], codex_oauth_service::ORIGINATOR);
        assert_eq!(headers["version"], "1.2.3");
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
            display_name: None,
            owned_by: Some("openai".to_string()),
        }];

        let original_url = oauth_models_source_url(ModelListFormat::CodexApi, "0.169.0");
        let updated_url = oauth_models_source_url(ModelListFormat::CodexApi, "0.170.0");
        store_codex_models(&api_key, &models, &original_url);

        assert_eq!(
            cached_codex_models(&api_key, Instant::now(), &original_url),
            Some(models.clone())
        );
        assert_eq!(
            cached_codex_models(&api_key, Instant::now(), &updated_url),
            None
        );
        assert_eq!(
            cached_codex_models(&chatgpt_key, Instant::now(), &original_url),
            None
        );
        let fallback = cached_codex_fallback(Some(&api_key), "temporary failure", Instant::now())
            .expect("a recent successful catalog should remain available");
        assert_eq!(fallback.source_url, original_url);
        assert_eq!(fallback.models, models);
        let serialized = serde_json::to_value(&fallback).unwrap();
        let warning = serialized["warning"]
            .as_str()
            .expect("cached fallback must expose its refresh failure");
        assert!(!warning.trim().is_empty());
        assert!(!warning.contains("temporary failure"));
        assert!(!warning.contains(&api_key));
        assert!(!warning.contains(codex_oauth_service::CHATGPT_CODEX_MODELS_URL));
        store_codex_models(&chatgpt_key, &fallback.models, &original_url);
        remove_cached_codex_models_for_identity(&codex_models_cache_identity(&token));
        assert_eq!(
            cached_codex_models(&api_key, Instant::now(), &original_url),
            None
        );
        assert_eq!(
            cached_codex_models(&chatgpt_key, Instant::now(), &original_url),
            None
        );
    }

    #[test]
    fn codex_fallback_rejects_expired_catalogs() {
        let cache_key = "codex-api:expired-cache-test";
        let fetched_at = Instant::now();
        codex_models_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                cache_key.to_string(),
                CachedCodexModels {
                    fetched_at,
                    source_url: oauth_models_source_url(ModelListFormat::CodexApi, "0.169.0"),
                    models: vec![AiModelInfo {
                        id: "stale-model".to_string(),
                        display_name: None,
                        owned_by: Some("openai".to_string()),
                    }],
                },
            );

        assert!(cached_codex_fallback(
            Some(cache_key),
            "temporary failure",
            fetched_at + CODEX_MODELS_CACHE_TTL,
        )
        .is_some());
        assert!(cached_codex_fallback(
            Some(cache_key),
            "temporary failure",
            fetched_at + CODEX_MODELS_CACHE_TTL + Duration::from_secs(1),
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
    provider: Option<String>,
) -> Result<(), String> {
    let provider = provider
        .unwrap_or_else(|| state.with_profile(|p| p.llm_provider.resolve_assistant_provider()));
    let keyring_user = llm_provider::keyring_user_for_provider(&provider);

    llm_provider::save_or_delete_api_key(&app_handle, &keyring_user, &api_key)?;

    log::info!("助手 API Key 已更新: provider={}", provider);
    Ok(())
}

#[tauri::command]
pub async fn get_assistant_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider: Option<String>,
) -> Result<String, String> {
    let provider = provider
        .unwrap_or_else(|| state.with_profile(|p| p.llm_provider.resolve_assistant_provider()));
    Ok(llm_provider::load_api_key_for_provider(
        &app_handle,
        &provider,
    ))
}
