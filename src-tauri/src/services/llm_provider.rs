use tauri_plugin_keyring::KeyringExt;

use crate::state::user_profile::{ApiFormat, LlmProviderConfig};
use crate::state::AppState;

/// LLM 提供商配置
pub struct LlmEndpoint {
    pub provider: String,
    pub api_url: String,
    pub model: String,
    pub timeout_secs: u64,
    pub api_format: ApiFormat,
}

pub const KEYRING_SERVICE: &str = "light-whisper";

const CEREBRAS: &str = "cerebras";
const OPENAI: &str = "openai";
const DEEPSEEK: &str = "deepseek";
const SILICONFLOW: &str = "siliconflow";
const CUSTOM: &str = "custom";

/// 预置服务商列表（用于判断是否为预置）
const PRESET_PROVIDERS: &[&str] = &[CEREBRAS, OPENAI, DEEPSEEK, SILICONFLOW, CUSTOM];

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

fn normalize_anthropic_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.anthropic.com/v1/messages".to_string();
    }

    if let Some(explicit) = trimmed.strip_suffix('#') {
        return explicit.trim_end_matches('/').to_string();
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("/messages") {
        return trimmed.to_string();
    }
    if lower.ends_with("/v1") {
        return format!("{trimmed}/messages");
    }

    format!("{trimmed}/v1/messages")
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

fn is_preset(provider: &str) -> bool {
    PRESET_PROVIDERS.contains(&provider)
}

/// 根据后端配置获取 LLM 端点
pub fn endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    let active_provider = config.resolve_active_provider();

    if is_preset(&active_provider) {
        let (default_base_url, default_model, timeout_secs) =
            default_endpoint_parts(&active_provider);
        let use_custom_endpoint = active_provider == CUSTOM;
        LlmEndpoint {
            provider: active_provider.clone(),
            api_url: if use_custom_endpoint {
                normalize_api_url(config.custom_base_url.as_deref(), default_base_url)
            } else {
                normalize_api_url(None, default_base_url)
            },
            model: config
                .custom_model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(default_model)
                .to_string(),
            timeout_secs,
            api_format: ApiFormat::OpenaiCompat,
        }
    } else if let Some(cp) = config
        .custom_providers
        .iter()
        .find(|p| p.id == active_provider)
    {
        let api_url = match cp.api_format {
            ApiFormat::Anthropic => normalize_anthropic_url(&cp.base_url),
            ApiFormat::OpenaiCompat => {
                normalize_api_url(Some(&cp.base_url), "http://127.0.0.1:8000")
            }
        };
        LlmEndpoint {
            provider: active_provider,
            api_url,
            model: if cp.model.trim().is_empty() {
                "gpt-4.1-mini".to_string()
            } else {
                cp.model.clone()
            },
            timeout_secs: if cp.api_format == ApiFormat::Anthropic {
                30
            } else {
                10
            },
            api_format: cp.api_format.clone(),
        }
    } else {
        // fallback to cerebras
        let (base, model, timeout) = default_endpoint_parts(CEREBRAS);
        LlmEndpoint {
            provider: CEREBRAS.to_string(),
            api_url: normalize_api_url(None, base),
            model: model.to_string(),
            timeout_secs: timeout,
            api_format: ApiFormat::OpenaiCompat,
        }
    }
}

pub fn looks_like_image_input_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_image = normalized.contains("image")
        || normalized.contains("vision")
        || normalized.contains("multimodal")
        || normalized.contains("input_image")
        || normalized.contains("image_url");

    let indicates_unsupported = normalized.contains("not supported")
        || normalized.contains("unsupported")
        || normalized.contains("does not support")
        || normalized.contains("invalid image")
        || normalized.contains("invalid content type")
        || normalized.contains("unsupported content type")
        || normalized.contains("unsupported modality")
        || normalized.contains("modalities are not supported")
        || normalized.contains("invalid_value");

    mentions_image && indicates_unsupported
}

/// 获取模型列表 URL
pub fn models_url(config: &LlmProviderConfig, provider: &str, base_url: Option<&str>) -> String {
    if !is_preset(provider) {
        if let Some(cp) = config.custom_providers.iter().find(|p| p.id == provider) {
            let effective_url = base_url.unwrap_or(&cp.base_url);
            if cp.api_format == ApiFormat::Anthropic {
                return normalize_anthropic_models_url(effective_url);
            }
            return normalize_models_url(Some(effective_url), &cp.base_url);
        }
    }
    let (default_base_url, _, _) = default_endpoint_parts(provider);
    if provider == CUSTOM {
        normalize_models_url(base_url, default_base_url)
    } else {
        normalize_models_url(None, default_base_url)
    }
}

/// 规范化 Anthropic 模型列表 URL（`/v1/models`）
fn normalize_anthropic_models_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.anthropic.com/v1/models".to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("/v1/models") {
        return trimmed.to_string();
    }
    if lower.ends_with("/v1/messages") {
        return format!(
            "{}/models",
            trimmed[..trimmed.len() - "/messages".len()].trim_end_matches('/')
        );
    }
    if lower.ends_with("/v1") {
        return format!("{trimmed}/models");
    }
    format!("{trimmed}/v1/models")
}

/// 获取密钥环用户名（每个后端独立存储 API Key）
pub fn keyring_user_for_provider(provider: &str) -> String {
    match provider {
        OPENAI => "openai-api-key".to_string(),
        DEEPSEEK => "deepseek-api-key".to_string(),
        SILICONFLOW => "siliconflow-api-key".to_string(),
        CUSTOM => "custom-api-key".to_string(),
        CEREBRAS => "cerebras-api-key".to_string(),
        id => format!("custom-{id}-api-key"),
    }
}

/// 构建认证 headers（按 api_format 分支）
pub fn build_auth_headers(
    api_format: &ApiFormat,
    api_key: &str,
) -> Result<reqwest::header::HeaderMap, String> {
    let api_key = api_key.trim();
    let mut headers = reqwest::header::HeaderMap::new();
    let parse = |v: &str| {
        v.parse::<reqwest::header::HeaderValue>()
            .map_err(|_| format!("API Key 包含非法字符，无法作为 HTTP header 使用"))
    };
    match api_format {
        ApiFormat::Anthropic => {
            headers.insert("x-api-key", parse(api_key)?);
            headers.insert("anthropic-version", parse("2023-06-01")?);
            headers.insert("content-type", parse("application/json")?);
        }
        ApiFormat::OpenaiCompat => {
            headers.insert("Authorization", parse(&format!("Bearer {api_key}"))?);
            headers.insert("Content-Type", parse("application/json")?);
        }
    }
    Ok(headers)
}

pub fn load_api_key_for_provider(app_handle: &tauri::AppHandle, provider: &str) -> String {
    let keyring_user = keyring_user_for_provider(provider);
    app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, &keyring_user)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_presets_ignore_custom_endpoint_overrides() {
        let config = LlmProviderConfig {
            active: CEREBRAS.to_string(),
            custom_base_url: Some("https://example.com".to_string()),
            custom_model: Some("gpt-oss-20b".to_string()),
            custom_providers: Vec::new(),
        };

        let endpoint = endpoint_for_config(&config);

        assert_eq!(endpoint.provider, CEREBRAS);
        assert_eq!(
            endpoint.api_url,
            "https://api.cerebras.ai/v1/chat/completions"
        );
        assert_eq!(endpoint.model, "gpt-oss-20b");
    }

    #[test]
    fn named_presets_preserve_manual_model_override() {
        let config = LlmProviderConfig {
            active: CEREBRAS.to_string(),
            custom_base_url: None,
            custom_model: Some("openai/gpt-5.3-chat-latest".to_string()),
            custom_providers: Vec::new(),
        };

        let endpoint = endpoint_for_config(&config);

        assert_eq!(endpoint.provider, CEREBRAS);
        assert_eq!(endpoint.model, "openai/gpt-5.3-chat-latest");
    }

    #[test]
    fn custom_preset_keeps_custom_endpoint_and_model() {
        let config = LlmProviderConfig {
            active: CUSTOM.to_string(),
            custom_base_url: Some("https://example.com".to_string()),
            custom_model: Some("foo-model".to_string()),
            custom_providers: Vec::new(),
        };

        let endpoint = endpoint_for_config(&config);

        assert_eq!(endpoint.provider, CUSTOM);
        assert_eq!(endpoint.api_url, "https://example.com/v1/chat/completions");
        assert_eq!(endpoint.model, "foo-model");
    }

    #[test]
    fn invalid_active_provider_falls_back_to_latest_custom_provider() {
        let config = LlmProviderConfig {
            active: "custom_missing".to_string(),
            custom_base_url: None,
            custom_model: None,
            custom_providers: vec![
                crate::state::user_profile::CustomProvider {
                    id: "custom_a".to_string(),
                    name: "A".to_string(),
                    base_url: "https://a.example.com".to_string(),
                    model: "model-a".to_string(),
                    api_format: ApiFormat::OpenaiCompat,
                },
                crate::state::user_profile::CustomProvider {
                    id: "custom_b".to_string(),
                    name: "B".to_string(),
                    base_url: "https://b.example.com".to_string(),
                    model: "model-b".to_string(),
                    api_format: ApiFormat::OpenaiCompat,
                },
            ],
        };

        let endpoint = endpoint_for_config(&config);

        assert_eq!(endpoint.provider, "custom_b");
        assert_eq!(
            endpoint.api_url,
            "https://b.example.com/v1/chat/completions"
        );
        assert_eq!(endpoint.model, "model-b");
    }

    #[test]
    fn recognizes_image_unsupported_errors() {
        assert!(looks_like_image_input_unsupported_error(
            "API 返回错误 400: model does not support image input"
        ));
        assert!(looks_like_image_input_unsupported_error(
            "unsupported content type: input_image"
        ));
        assert!(!looks_like_image_input_unsupported_error(
            "API 返回错误 401: invalid api key"
        ));
    }
}
