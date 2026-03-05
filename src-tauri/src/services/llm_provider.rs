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

/// 已知的 LLM 后端
const DEEPSEEK: &str = "deepseek";
const CUSTOM: &str = "custom";

/// 根据后端名称和配置获取 LLM 端点
pub fn get_endpoint(
    provider: &str,
    custom_base_url: Option<&str>,
    custom_model: Option<&str>,
) -> LlmEndpoint {
    match provider {
        DEEPSEEK => LlmEndpoint {
            api_url: "https://api.deepseek.com/v1/chat/completions".to_string(),
            model: "deepseek-chat".to_string(),
            timeout_secs: 10,
        },
        CUSTOM => LlmEndpoint {
            api_url: custom_base_url
                .unwrap_or("https://api.openai.com/v1/responses")
                .to_string(),
            model: custom_model.unwrap_or("gpt-3.5-turbo").to_string(),
            timeout_secs: 10,
        },
        // 默认 Cerebras
        _ => LlmEndpoint {
            api_url: "https://api.cerebras.ai/v1/chat/completions".to_string(),
            model: "gpt-oss-120b".to_string(),
            timeout_secs: 5,
        },
    }
}

pub fn endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    get_endpoint(
        &config.active,
        config.custom_base_url.as_deref(),
        config.custom_model.as_deref(),
    )
}

/// 获取密钥环用户名（每个后端独立存储 API Key）
pub fn keyring_user_for_provider(provider: &str) -> &str {
    match provider {
        DEEPSEEK => "deepseek-api-key",
        CUSTOM => "custom-api-key",
        _ => "cerebras-api-key",
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
