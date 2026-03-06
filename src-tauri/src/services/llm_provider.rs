use tauri_plugin_keyring::KeyringExt;

use crate::state::user_profile::LlmProviderConfig;
use crate::state::AppState;

/// LLM 提供商配置
pub struct LlmEndpoint {
    pub api_url: String,
    pub model: String,
    pub timeout_secs: u64,
}

pub const KEYRING_SERVICE: &str = "light-whisper";

const CEREBRAS: &str = "cerebras";
const OPENAI: &str = "openai";
/// 已知的 LLM 后端
const DEEPSEEK: &str = "deepseek";
const SILICONFLOW: &str = "siliconflow";
const CUSTOM: &str = "custom";

fn default_endpoint_parts(provider: &str) -> (&'static str, &'static str, u64) {
    match provider {
        OPENAI => ("https://api.openai.com", "gpt-4.1-mini", 10),
        DEEPSEEK => ("https://api.deepseek.com", "deepseek-chat", 10),
        SILICONFLOW => ("https://api.siliconflow.cn", "Qwen/Qwen3-32B", 10),
        CUSTOM => ("http://127.0.0.1:8000", "gpt-4.1-mini", 10),
        _ => ("https://api.cerebras.ai", "gpt-oss-120b", 5),
    }
}

fn normalize_api_url(input: Option<&str>, default_base_url: &str) -> String {
    let raw = input
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base_url);

    if let Some(explicit) = raw.strip_suffix('#') {
        return explicit.trim_end_matches('/').to_string();
    }

    let trimmed = raw.trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("/chat/completions") || lower.ends_with("/responses") {
        return trimmed.to_string();
    }

    if lower.ends_with("/v1") || lower.ends_with("/api/v3") {
        return format!("{trimmed}/chat/completions");
    }

    format!("{trimmed}/v1/chat/completions")
}

fn normalize_models_url(input: Option<&str>, default_base_url: &str) -> String {
    let raw = input
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base_url);

    let trimmed = raw.trim_end_matches('#').trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();

    if lower.ends_with("/models") {
        return trimmed.to_string();
    }
    if lower.ends_with("/chat/completions") {
        return format!(
            "{}/models",
            trimmed[..trimmed.len() - "/chat/completions".len()].trim_end_matches('/')
        );
    }
    if lower.ends_with("/responses") {
        return format!(
            "{}/models",
            trimmed[..trimmed.len() - "/responses".len()].trim_end_matches('/')
        );
    }
    if lower.ends_with("/v1") || lower.ends_with("/api/v3") {
        return format!("{trimmed}/models");
    }

    format!("{trimmed}/v1/models")
}

/// 根据后端名称和配置获取 LLM 端点
pub fn get_endpoint(
    provider: &str,
    custom_base_url: Option<&str>,
    custom_model: Option<&str>,
) -> LlmEndpoint {
    let (default_base_url, default_model, timeout_secs) = default_endpoint_parts(provider);
    LlmEndpoint {
        api_url: normalize_api_url(custom_base_url, default_base_url),
        model: custom_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(default_model)
            .to_string(),
        timeout_secs,
    }
}

pub fn endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    get_endpoint(
        &config.active,
        config.custom_base_url.as_deref(),
        config.custom_model.as_deref(),
    )
}

pub fn models_url(provider: &str, custom_base_url: Option<&str>) -> String {
    let (default_base_url, _, _) = default_endpoint_parts(provider);
    normalize_models_url(custom_base_url, default_base_url)
}

/// 获取密钥环用户名（每个后端独立存储 API Key）
pub fn keyring_user_for_provider(provider: &str) -> &str {
    match provider {
        OPENAI => "openai-api-key",
        DEEPSEEK => "deepseek-api-key",
        SILICONFLOW => "siliconflow-api-key",
        CUSTOM => "custom-api-key",
        CEREBRAS => "cerebras-api-key",
        _ => "custom-api-key",
    }
}

pub fn load_api_key_for_provider(app_handle: &tauri::AppHandle, provider: &str) -> String {
    let keyring_user = keyring_user_for_provider(provider);
    app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, keyring_user)
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub fn load_api_key_for_active_provider(app_handle: &tauri::AppHandle, state: &AppState) -> String {
    load_api_key_for_provider(app_handle, &state.active_llm_provider())
}

pub fn sync_runtime_api_key(app_handle: &tauri::AppHandle, state: &AppState) -> String {
    let api_key = load_api_key_for_active_provider(app_handle, state);
    state.set_ai_polish_api_key(api_key.clone());
    api_key
}
