use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::services::{
    codex_oauth_service, jev_service, llm_client::LlmRequestOptions, llm_provider, model_connection,
};
use crate::state::user_profile::{
    ApiFormat, JevProvider, LlmProviderConfig, LlmReasoningMode, OpenaiAuthMode, XaiAuthMode,
};
use crate::state::AppState;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelConnectionTarget {
    Decision,
    Polish,
    Assistant,
    Selection,
    Vision,
    Validation,
    Asr,
}

// Deliberately no Debug: this transient request may contain an unsaved API key.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnectionRequest {
    pub target: ModelConnectionTarget,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub reasoning_mode: Option<LlmReasoningMode>,
    pub openai_auth_mode: Option<OpenaiAuthMode>,
    pub xai_auth_mode: Option<XaiAuthMode>,
    pub engine: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnectionResult {
    pub model: String,
    pub elapsed_ms: u64,
}

fn endpoint_for_request(
    config: &LlmProviderConfig,
    request: &ModelConnectionRequest,
) -> Result<llm_provider::LlmEndpoint, String> {
    let saved = match request.target {
        ModelConnectionTarget::Assistant => {
            Some(llm_provider::assistant_endpoint_for_config(config))
        }
        ModelConnectionTarget::Selection => {
            Some(llm_provider::selection_endpoint_for_config(config))
        }
        ModelConnectionTarget::Validation => {
            Some(llm_provider::validation_endpoint_for_config(config))
        }
        ModelConnectionTarget::Vision => llm_provider::screen_vision_endpoint_for_config(config),
        _ => Some(llm_provider::endpoint_for_config(config)),
    };
    let Some(provider) = request.provider.as_deref() else {
        return saved.ok_or_else(|| "模型配置不完整，请选择服务商和模型。".into());
    };
    let custom = config
        .custom_providers
        .iter()
        .find(|item| item.id == provider);
    if !LlmProviderConfig::is_builtin_provider(provider) && custom.is_none() {
        return Err("服务商配置已变化，请重新选择后测试。".into());
    }
    let saved = saved
        .as_ref()
        .filter(|endpoint| endpoint.provider == provider);
    let base_url = request
        .base_url
        .as_deref()
        .or(custom.map(|item| item.base_url.as_str()))
        .or_else(|| {
            (provider == "custom")
                .then_some(config.custom_base_url.as_deref())
                .flatten()
        });
    let endpoint = llm_provider::endpoint_for_preview(
        provider,
        base_url,
        request
            .model
            .as_deref()
            .or(saved.map(|endpoint| endpoint.model.as_str()))
            .or(custom.map(|item| item.model.as_str())),
        custom
            .map(|item| item.api_format.clone())
            .unwrap_or(ApiFormat::OpenaiCompat),
    );
    let url = reqwest::Url::parse(&endpoint.api_url).map_err(|_| "API 地址无效。".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("API 地址无效，请使用不含用户名和密码的 HTTP(S) 地址。".into());
    }
    Ok(endpoint)
}

#[tauri::command]
pub async fn test_model_connection(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: ModelConnectionRequest,
) -> Result<ModelConnectionResult, String> {
    let started = Instant::now();
    let model = match request.target {
        ModelConnectionTarget::Decision => {
            let mut config = state.with_profile(|profile| profile.jev.clone());
            if let Some(provider) = &request.provider {
                config.provider =
                    serde_json::from_value::<JevProvider>(serde_json::json!(provider))
                        .map_err(|_| "未知的决策模型服务商。".to_string())?;
            }
            let mut connection = jev_service::connection_for(&config, config.provider);
            if let Some(url) = request.base_url {
                connection.base_url = url;
            }
            if let Some(model) = request.model {
                connection.model = model;
            }
            let connection = jev_service::validate_connection(connection)?;
            config.connections.insert(config.provider, connection);
            let key = match request.api_key {
                Some(key) => key,
                None => jev_service::load_api_key_for_provider(&app_handle, config.provider)?,
            };
            model_connection::probe_decision(&state.http_client, &config, &key).await?;
            jev_service::connection_for(&config, config.provider).model
        }
        ModelConnectionTarget::Asr => test_asr(&app_handle, state.inner(), &request).await?,
        _ => {
            let config = state.llm_provider_config();
            let endpoint = endpoint_for_request(&config, &request)?;
            let manual_key = request.api_key.clone().unwrap_or_else(|| {
                llm_provider::load_api_key_for_provider(&app_handle, &endpoint.provider)
            });
            let key = tokio::time::timeout(
                Duration::from_secs(30),
                codex_oauth_service::resolve_api_key_for_provider_with_auth_mode(
                    &app_handle,
                    state.inner(),
                    &endpoint.provider,
                    &manual_key,
                    request.openai_auth_mode.or(config.openai_auth_mode),
                    request.xai_auth_mode.or(config.xai_auth_mode),
                ),
            )
            .await
            .map_err(|_| "认证检查超时，请检查网络后重试。".to_string())?
            .map_err(|_| "认证读取或刷新失败，请检查 API Key 或重新登录。".to_string())?;
            let reasoning = request
                .reasoning_mode
                .unwrap_or_else(|| match request.target {
                    ModelConnectionTarget::Assistant => config.assistant_reasoning_mode(),
                    ModelConnectionTarget::Selection => config.selection_reasoning_mode(),
                    _ => config.polish_reasoning_mode(),
                });
            let options = LlmRequestOptions {
                auth_context: Some((&app_handle, state.inner())),
                reasoning_mode: reasoning,
                openai_fast_mode: config.openai_fast_mode,
                stream_total_timeout_secs: Some(30),
                ..Default::default()
            };
            model_connection::probe_llm(
                &state.http_client,
                &endpoint,
                &key,
                matches!(request.target, ModelConnectionTarget::Vision),
                options,
            )
            .await?;
            endpoint.model
        }
    };
    Ok(ModelConnectionResult {
        model,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

struct AsrConnection {
    engine: String,
    base_url: String,
    model: String,
    api_key: String,
}

async fn asr_snapshot(
    state: &AppState,
    request: &ModelConnectionRequest,
) -> Result<AsrConnection, String> {
    use crate::services::glm_asr_service;
    use crate::utils::paths;

    // Engine/region changes and key cache publication use the same lock.
    let _configuration = state.engine.funasr_lifecycle_op.lock().await;
    let engine = paths::read_engine_config();
    if request
        .engine
        .as_deref()
        .is_some_and(|expected| expected != engine)
    {
        return Err("语音引擎已切换，请重新测试。".into());
    }
    let base_url = match engine.as_str() {
        "alibaba-asr" => paths::read_alibaba_endpoint(),
        "glm-asr" => paths::read_glm_endpoint(),
        _ => String::new(),
    };
    if paths::is_online_engine(&engine)
        && request
            .base_url
            .as_deref()
            .is_some_and(|url| url.trim().trim_end_matches('/') != base_url.trim_end_matches('/'))
    {
        return Err("语音服务区域已变化，请等待设置加载后重试。".into());
    }
    let model = match engine.as_str() {
        "alibaba-asr" => request
            .model
            .clone()
            .unwrap_or_else(paths::read_alibaba_model),
        "glm-asr" => glm_asr_service::GLM_ASR_MODEL.to_string(),
        _ => engine.clone(),
    };
    let api_key = request
        .api_key
        .clone()
        .unwrap_or_else(|| state.read_online_asr_api_key());
    Ok(AsrConnection {
        engine,
        base_url,
        model,
        api_key,
    })
}

async fn test_asr(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    request: &ModelConnectionRequest,
) -> Result<String, String> {
    use crate::services::funasr_service;
    use crate::state::RecordingPhase;
    use crate::utils::paths;

    if state.recording.recording.lock().is_some()
        || state
            .recording
            .snapshot()
            .is_some_and(|snapshot| snapshot.phase == RecordingPhase::Processing)
    {
        return Err("录音或处理尚未结束，请稍后测试语音模型。".into());
    }
    let connection = asr_snapshot(state, request).await?;
    if paths::is_online_engine(&connection.engine) {
        model_connection::probe_online_asr(
            &state.http_client,
            &connection.engine,
            &connection.base_url,
            &connection.model,
            &connection.api_key,
        )
        .await?;
        Ok(connection.model)
    } else {
        if !state.is_funasr_ready() {
            return Err("本地语音模型尚未就绪，请等待模型加载，或重试模型加载后再测试。".into());
        }
        let result = funasr_service::probe_model(state, &connection.engine, app_handle)
            .await
            .map_err(|error| model_connection::connection_error(&error.to_string()))?;
        if !result.success || result.text.trim().is_empty() {
            return Err("本地模型未能识别内置测试语音，请检查模型文件和设备状态。".into());
        }
        Ok(connection.model)
    }
}

#[cfg(test)]
#[path = "model_connection_tests.rs"]
mod tests;
