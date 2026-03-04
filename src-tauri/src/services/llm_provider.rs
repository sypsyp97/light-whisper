/// LLM 提供商配置
pub struct LlmEndpoint {
    pub api_url: String,
    pub model: String,
    pub timeout_secs: u64,
}

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

/// 获取密钥环用户名（每个后端独立存储 API Key）
pub fn keyring_user_for_provider(provider: &str) -> &str {
    match provider {
        DEEPSEEK => "deepseek-api-key",
        CUSTOM => "custom-api-key",
        _ => "cerebras-api-key",
    }
}
