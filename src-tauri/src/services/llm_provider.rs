use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;
use tauri_plugin_keyring::KeyringExt;

use crate::services::codex_oauth_service;
use crate::state::user_profile::{ApiFormat, CustomProvider, LlmProviderConfig, LlmReasoningMode};
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LlmReasoningSupport {
    pub supported: bool,
    pub strategy: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoReasoningStrategy {
    OpenaiResponsesReasoning,
    OpenaiChatReasoningEffort,
    TopLevelThinking,
    ChatTemplateThinking,
    NoControls,
}

impl AutoReasoningStrategy {
    pub fn strategy_name(self) -> &'static str {
        match self {
            Self::OpenaiResponsesReasoning => "auto_openai_responses_reasoning",
            Self::OpenaiChatReasoningEffort => "auto_openai_chat_reasoning_effort",
            Self::TopLevelThinking => "auto_top_level_thinking",
            Self::ChatTemplateThinking => "auto_chat_template_thinking",
            Self::NoControls => "auto_no_reasoning_controls",
        }
    }
}

static AUTO_REASONING_STRATEGY_CACHE: OnceLock<
    parking_lot::Mutex<HashMap<String, AutoReasoningStrategy>>,
> = OnceLock::new();

fn default_endpoint_parts(provider: &str) -> (&'static str, &'static str, u64) {
    match provider {
        OPENAI => ("https://api.openai.com", "gpt-4.1-mini", 10),
        DEEPSEEK => ("https://api.deepseek.com", "deepseek-v4-flash", 10),
        SILICONFLOW => ("https://api.siliconflow.cn", "Qwen/Qwen3-32B", 10),
        CUSTOM => ("http://127.0.0.1:8000", "gpt-4.1-mini", 10),
        _ => ("https://api.cerebras.ai", "gpt-oss-120b", 5),
    }
}

fn default_api_suffix(provider: &str) -> &'static str {
    match provider {
        OPENAI => "responses",
        _ => "chat/completions",
    }
}

fn normalize_api_url(input: Option<&str>, default_base_url: &str, api_suffix: &str) -> String {
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
        return format!("{trimmed}/{api_suffix}");
    }

    format!("{trimmed}/v1/{api_suffix}")
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
        let api_suffix = default_api_suffix(&active_provider);
        LlmEndpoint {
            provider: active_provider.clone(),
            api_url: if use_custom_endpoint {
                normalize_api_url(
                    config.custom_base_url.as_deref(),
                    default_base_url,
                    api_suffix,
                )
            } else {
                normalize_api_url(None, default_base_url, api_suffix)
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
            ApiFormat::OpenaiCompat => normalize_api_url(
                Some(&cp.base_url),
                "http://127.0.0.1:8000",
                "chat/completions",
            ),
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
            api_url: normalize_api_url(None, base, "chat/completions"),
            model: model.to_string(),
            timeout_secs: timeout,
            api_format: ApiFormat::OpenaiCompat,
        }
    }
}

pub fn assistant_endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    let assistant_provider = config.resolve_assistant_provider();
    let active_provider = config.resolve_active_provider();

    // 构建 base config：若助手 provider 与润色不同，临时切换 active
    let mut resolved = config.clone();
    if assistant_provider != active_provider {
        resolved.active = assistant_provider;
        // 对 preset provider，清掉 custom_model/custom_base_url（不能沿用润色的覆盖值）
        if is_preset(&resolved.active) {
            resolved.custom_model = None;
            resolved.custom_base_url = None;
        }
    }

    // 叠加 assistant_model 覆盖
    if let Some(assistant_model) = config.assistant_model() {
        let target_provider = resolved.resolve_active_provider();
        if is_preset(&target_provider) {
            resolved.custom_model = Some(assistant_model.to_string());
        } else if let Some(cp) = resolved
            .custom_providers
            .iter_mut()
            .find(|p| p.id == target_provider)
        {
            cp.model = assistant_model.to_string();
        } else {
            resolved.custom_model = Some(assistant_model.to_string());
        }
    }

    endpoint_for_config(&resolved)
}

pub fn validation_endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    let validation_provider = config.resolve_validation_provider();
    let active_provider = config.resolve_active_provider();

    let mut resolved = config.clone();
    if validation_provider != active_provider {
        resolved.active = validation_provider;
        if is_preset(&resolved.active) {
            resolved.custom_model = None;
            resolved.custom_base_url = None;
        }
    }

    if let Some(validation_model) = config.validation_model() {
        let target_provider = resolved.resolve_active_provider();
        if is_preset(&target_provider) {
            resolved.custom_model = Some(validation_model.to_string());
        } else if let Some(cp) = resolved
            .custom_providers
            .iter_mut()
            .find(|p| p.id == target_provider)
        {
            cp.model = validation_model.to_string();
        } else {
            resolved.custom_model = Some(validation_model.to_string());
        }
    }

    endpoint_for_config(&resolved)
}

pub fn image_support_probe_url(endpoint: &LlmEndpoint) -> Option<String> {
    if endpoint.api_format != ApiFormat::OpenaiCompat {
        return None;
    }

    let model = endpoint.model.trim();
    if model.is_empty() {
        return None;
    }

    let models_url = normalize_models_url(Some(&endpoint.api_url), &endpoint.api_url);
    let mut url = reqwest::Url::parse(&models_url).ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty();
        segments.push(model);
    }
    Some(url.into())
}

pub fn cerebras_public_model_probe_url(model: &str) -> Option<String> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }

    let mut url = reqwest::Url::parse("https://api.cerebras.ai/public/v1/models/").ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty();
        segments.push(model);
    }
    url.query_pairs_mut().append_pair("format", "openrouter");
    Some(url.into())
}

pub fn should_probe_cerebras_public_model_metadata(endpoint: &LlmEndpoint) -> bool {
    if is_cerebras_like_endpoint(endpoint) {
        return true;
    }

    endpoint
        .model
        .trim()
        .to_ascii_lowercase()
        .contains("gpt-oss")
}

pub fn parse_image_input_support_from_model_metadata(payload: &Value) -> Option<bool> {
    let model = payload
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .unwrap_or(payload);

    let modalities = model.get("input_modalities")?.as_array()?;
    let normalized = modalities
        .iter()
        .filter_map(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        return None;
    }

    if normalized
        .iter()
        .any(|value| value == "image" || value == "input_image")
    {
        return Some(true);
    }

    if normalized.iter().any(|value| value == "text") {
        return Some(false);
    }

    None
}

pub async fn probe_image_support_from_provider_metadata(
    http_client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
) -> Option<bool> {
    if codex_oauth_service::decode_chatgpt_bearer_token(api_key).is_some() {
        return None;
    }

    let mut probe_targets = Vec::new();
    if let Some(url) = image_support_probe_url(endpoint) {
        probe_targets.push((url, true));
    }
    if should_probe_cerebras_public_model_metadata(endpoint) {
        if let Some(url) = cerebras_public_model_probe_url(&endpoint.model) {
            if !probe_targets.iter().any(|(existing, _)| existing == &url) {
                probe_targets.push((url, false));
            }
        }
    }

    for (url, use_auth) in probe_targets {
        let mut request = http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(2));
        if use_auth {
            let Ok(headers) = build_auth_headers(&endpoint.api_format, api_key) else {
                continue;
            };
            request = request.headers(headers);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(err) => {
                log::debug!("探测模型图片能力失败: url={}, err={}", url, err);
                continue;
            }
        };

        if !response.status().is_success() {
            log::debug!(
                "模型图片能力探测返回非成功状态: status={}, url={}",
                response.status(),
                url
            );
            continue;
        }

        let payload = match response.json::<Value>().await {
            Ok(payload) => payload,
            Err(err) => {
                log::debug!("解析模型图片能力元数据失败: url={}, err={}", url, err);
                continue;
            }
        };

        if let Some(supported) = parse_image_input_support_from_model_metadata(&payload) {
            return Some(supported);
        }
    }

    None
}

pub fn endpoint_for_preview(
    provider: &str,
    base_url: Option<&str>,
    model: Option<&str>,
    api_format: ApiFormat,
) -> LlmEndpoint {
    let config = if is_preset(provider) {
        LlmProviderConfig {
            active: provider.to_string(),
            custom_base_url: base_url.map(str::to_string),
            custom_model: model.map(str::to_string),
            reasoning_mode: LlmReasoningMode::ProviderDefault,
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            assistant_provider: None,
            custom_providers: Vec::new(),
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
        }
    } else {
        LlmProviderConfig {
            active: provider.to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: LlmReasoningMode::ProviderDefault,
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            assistant_provider: None,
            custom_providers: vec![CustomProvider {
                id: provider.to_string(),
                name: provider.to_string(),
                base_url: base_url.unwrap_or_default().to_string(),
                model: model.unwrap_or_default().to_string(),
                api_format,
            }],
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
        }
    };

    endpoint_for_config(&config)
}

pub fn endpoint_uses_responses_api(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat
        && endpoint
            .api_url
            .trim_end_matches('/')
            .to_ascii_lowercase()
            .ends_with("/responses")
}

pub fn image_support_cache_key(endpoint: &LlmEndpoint) -> String {
    format!(
        "{:?}|{}|{}|{}",
        endpoint.api_format,
        endpoint.provider,
        endpoint.api_url,
        endpoint.model.trim().to_ascii_lowercase()
    )
}

fn indicates_unsupported(normalized: &str) -> bool {
    normalized.contains("not supported")
        || normalized.contains("unsupported")
        || normalized.contains("does not support")
        || normalized.contains("not permitted")
        || normalized.contains("are not valid")
        || normalized.contains("invalidparameter")
        || normalized.contains("invalid parameter")
        || normalized.contains("badrequest")
}

pub fn looks_like_image_input_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_image = normalized.contains("image")
        || normalized.contains("vision")
        || normalized.contains("multimodal")
        || normalized.contains("input_image")
        || normalized.contains("image_url");

    mentions_image
        && (indicates_unsupported(&normalized)
            || normalized.contains("invalid image")
            || normalized.contains("invalid content type")
            || normalized.contains("unsupported content type")
            || normalized.contains("unsupported modality")
            || normalized.contains("modalities are not supported")
            || (normalized.contains("unknown variant")
                && normalized.contains("expected")
                && normalized.contains("text"))
            || normalized.contains("invalid_value"))
}

pub fn looks_like_web_search_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_search = normalized.contains("web_search")
        || normalized.contains("web search")
        || normalized.contains("websearch")
        || normalized.contains("search_preview");

    mentions_search
        && (indicates_unsupported(&normalized)
            || normalized.contains("unknown")
            || normalized.contains("invalid"))
}

pub fn looks_like_json_output_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_json_output = normalized.contains("response_format")
        || normalized.contains("json_object")
        || normalized.contains("text.format")
        || normalized.contains("json schema")
        || normalized.contains("structured output");

    mentions_json_output && indicates_unsupported(&normalized)
}

pub fn looks_like_reasoning_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_reasoning = normalized.contains("reasoning")
        || normalized.contains("reasoning_effort")
        || normalized.contains("thinking")
        || normalized.contains("chat_template_kwargs")
        || normalized.contains("budget_tokens")
        || normalized.contains("reasoning_content");

    mentions_reasoning
        && (indicates_unsupported(&normalized) || normalized.contains("unknown parameter"))
}

fn endpoint_host(endpoint: &LlmEndpoint) -> Option<String> {
    reqwest::Url::parse(&endpoint.api_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
}

fn endpoint_host_matches(endpoint: &LlmEndpoint, domain: &str) -> bool {
    let Some(host) = endpoint_host(endpoint) else {
        return false;
    };
    host == domain || host.ends_with(&format!(".{domain}"))
}

pub fn is_volcengine_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    if endpoint.api_format != ApiFormat::OpenaiCompat {
        return false;
    }

    let model = endpoint.model.trim().to_ascii_lowercase();

    endpoint_host_matches(endpoint, "volces.com")
        || endpoint_host_matches(endpoint, "volcengine.com")
        || model.contains("doubao")
        || model.contains("seed-")
}

fn is_deepseek_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == DEEPSEEK || endpoint_host_matches(endpoint, "deepseek.com")
}

fn is_siliconflow_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == SILICONFLOW || endpoint_host_matches(endpoint, "siliconflow.com")
}

pub fn is_cerebras_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == CEREBRAS || endpoint_host_matches(endpoint, "cerebras.ai")
}

const GPT5_EFFORTS: &[&str] = &["minimal", "low", "medium", "high"];
const GPT5_1_EFFORTS: &[&str] = &["none", "low", "medium", "high"];
const GPT5_2_54_EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];
const GPT5_5_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];
// The public API also accepts `none`, while the Codex OAuth catalog starts at
// `low`. Use the common set so the same saved mode works with either credential.
const GPT5_6_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const GPT5_PRO_EFFORTS: &[&str] = &["high"];
const GPT5_2_54_PRO_EFFORTS: &[&str] = &["medium", "high", "xhigh"];
const GPT5_2_3_CODEX_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];
const GPT5_1_CODEX_MAX_EFFORTS: &[&str] = &["none", "medium", "high", "xhigh"];

fn openai_gpt5_reasoning_efforts(model: &str) -> Option<&'static [&'static str]> {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    match tail {
        "gpt-5-pro" => Some(GPT5_PRO_EFFORTS),
        _ if tail.starts_with("gpt-5-pro-") => Some(GPT5_PRO_EFFORTS),
        "gpt-5.2-pro" | "gpt-5.4-pro" | "gpt-5.5-pro" => Some(GPT5_2_54_PRO_EFFORTS),
        _ if tail.starts_with("gpt-5.2-pro-")
            || tail.starts_with("gpt-5.4-pro-")
            || tail.starts_with("gpt-5.5-pro-") =>
        {
            Some(GPT5_2_54_PRO_EFFORTS)
        }
        "gpt-5.2-codex" | "gpt-5.3-codex" => Some(GPT5_2_3_CODEX_EFFORTS),
        _ if tail.starts_with("gpt-5.2-codex-") || tail.starts_with("gpt-5.3-codex-") => {
            Some(GPT5_2_3_CODEX_EFFORTS)
        }
        "gpt-5.1-codex-max" => Some(GPT5_1_CODEX_MAX_EFFORTS),
        _ if tail.starts_with("gpt-5.1-codex-max-") => Some(GPT5_1_CODEX_MAX_EFFORTS),
        "gpt-5-mini" | "gpt-5-nano" => Some(GPT5_EFFORTS),
        _ if tail.starts_with("gpt-5-mini-") || tail.starts_with("gpt-5-nano-") => {
            Some(GPT5_EFFORTS)
        }
        "gpt-5.1" => Some(GPT5_1_EFFORTS),
        _ if tail.starts_with("gpt-5.1-") => Some(GPT5_1_EFFORTS),
        "gpt-5.2" | "gpt-5.4" => Some(GPT5_2_54_EFFORTS),
        _ if tail.starts_with("gpt-5.2-") || tail.starts_with("gpt-5.4-") => {
            Some(GPT5_2_54_EFFORTS)
        }
        "gpt-5.5" => Some(GPT5_5_EFFORTS),
        _ if tail.starts_with("gpt-5.5-") => Some(GPT5_5_EFFORTS),
        "gpt-5.6" => Some(GPT5_6_EFFORTS),
        _ if tail.starts_with("gpt-5.6-") => Some(GPT5_6_EFFORTS),
        "gpt-5" => Some(GPT5_EFFORTS),
        _ if tail.starts_with("gpt-5-") => Some(GPT5_EFFORTS),
        _ => None,
    }
}

fn openai_gpt5_effort_for_mode(model: &str, mode: LlmReasoningMode) -> Option<&'static str> {
    let efforts = openai_gpt5_reasoning_efforts(model)?;
    let index = match mode {
        LlmReasoningMode::Off => 0,
        LlmReasoningMode::Light => 1,
        LlmReasoningMode::Balanced => 2,
        LlmReasoningMode::Deep => 3,
        LlmReasoningMode::ProviderDefault => return None,
    }
    .min(efforts.len() - 1);

    Some(efforts[index])
}

fn supports_anthropic_thinking(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.contains("claude-3-7-sonnet")
        || model.contains("claude-sonnet-4")
        || model.contains("claude-opus-4")
}

fn supports_volcengine_thinking(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    (model.contains("doubao-seed-1-6-")
        || model.contains("doubao-seed-2-0-")
        || model.contains("doubao-1.5-thinking-pro")
        || model.contains("doubao"))
        && (model.contains("thinking")
            || model.contains("flash")
            || model.contains("seed-2-0-mini")
            || model.contains("seed-2-0-lite")
            || model.contains("seed-2-0-pro"))
}

fn supports_siliconflow_reasoning(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    normalized.contains("qwen/qwen3-")
        || normalized.contains("qwen/qwq-")
        || normalized.contains("thudm/glm-z1-")
        || normalized.contains("minimaxai/minimax-m2.1")
        || normalized.contains("tencent/hunyuan-a13b-instruct")
        || normalized.contains("deepseek-ai/deepseek-r1")
        || normalized.contains("glm-4.1v-9b-thinking")
        || tail.starts_with("qwen3-")
        || tail.starts_with("qwq-")
        || tail.starts_with("glm-z1-")
        || tail.contains("deepseek-r1")
        || tail.contains("thinking")
}

fn supports_cerebras_reasoning(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    tail == "gpt-oss-120b"
}

fn supports_deepseek_thinking(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    matches!(
        tail,
        "deepseek-v4-flash" | "deepseek-v4-pro" | "deepseek-chat" | "deepseek-reasoner"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReasoningControlKind {
    OpenaiEffort,
    AnthropicThinking,
    DeepSeekThinkingToggle,
    SiliconFlowThinkingBudget,
    CerebrasReasoningEffort,
    CerebrasGlmToggle,
    VolcengineThinkingType,
    AutoOpenaiCompat,
}

impl ReasoningControlKind {
    fn strategy_name(self) -> &'static str {
        match self {
            Self::OpenaiEffort => "openai_reasoning_effort",
            Self::AnthropicThinking => "anthropic_thinking",
            Self::DeepSeekThinkingToggle => "deepseek_thinking",
            Self::SiliconFlowThinkingBudget => "siliconflow_thinking_budget",
            Self::CerebrasReasoningEffort => "cerebras_reasoning_effort",
            Self::CerebrasGlmToggle => "cerebras_disable_reasoning",
            Self::VolcengineThinkingType => "volcengine_thinking_type",
            Self::AutoOpenaiCompat => "auto_openai_compat_probe",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::OpenaiEffort => {
                "当前模型支持 reasoning effort；关闭/轻量/标准/深度会映射为对应的推理强度。"
            }
            Self::AnthropicThinking => {
                "当前模型支持 extended thinking；会映射为 thinking + budget_tokens。"
            }
            Self::DeepSeekThinkingToggle => {
                "当前模型支持 thinking.type；关闭会下发 disabled，其余档位会启用 thinking。"
            }
            Self::SiliconFlowThinkingBudget => {
                "当前模型支持 thinking_budget；不同档位会映射为不同预算。"
            }
            Self::CerebrasReasoningEffort => {
                "当前模型支持 reasoning_effort；不同档位会映射为不同强度。"
            }
            Self::CerebrasGlmToggle => {
                "当前模型支持 disable_reasoning；关闭会禁用推理，其余档位会启用推理。"
            }
            Self::VolcengineThinkingType => {
                "当前模型支持 thinking.type；关闭=disabled，轻量/标准=auto，深度=enabled。"
            }
            Self::AutoOpenaiCompat => {
                "当前是 OpenAI-compatible 后端；会自动探测可用的思考控制参数并缓存成功策略。"
            }
        }
    }
}

fn reasoning_control_kind(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
) -> Option<ReasoningControlKind> {
    let model = endpoint.model.trim();

    if endpoint.api_format == ApiFormat::Anthropic {
        return supports_anthropic_thinking(model)
            .then_some(ReasoningControlKind::AnthropicThinking);
    }

    if is_volcengine_like_endpoint(endpoint)
        && !uses_responses_api
        && supports_volcengine_thinking(model)
    {
        return Some(ReasoningControlKind::VolcengineThinkingType);
    }

    if is_deepseek_like_endpoint(endpoint) && supports_deepseek_thinking(model) {
        return Some(ReasoningControlKind::DeepSeekThinkingToggle);
    }

    if is_siliconflow_like_endpoint(endpoint) && supports_siliconflow_reasoning(model) {
        return Some(ReasoningControlKind::SiliconFlowThinkingBudget);
    }

    if is_cerebras_like_endpoint(endpoint) && supports_cerebras_reasoning(model) {
        return Some(ReasoningControlKind::CerebrasReasoningEffort);
    }

    if is_cerebras_like_endpoint(endpoint) {
        let normalized = model.trim().to_ascii_lowercase();
        let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
        if tail == "zai-glm-4.7" {
            return Some(ReasoningControlKind::CerebrasGlmToggle);
        }
    }

    if is_auto_probe_openai_compatible_endpoint(endpoint) {
        return Some(ReasoningControlKind::AutoOpenaiCompat);
    }

    if openai_gpt5_reasoning_efforts(model).is_some() {
        return Some(ReasoningControlKind::OpenaiEffort);
    }

    None
}

fn is_auto_probe_openai_compatible_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat && !is_openai_like_endpoint(endpoint)
}

pub fn is_openai_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == OPENAI || endpoint_host(endpoint).as_deref() == Some("api.openai.com")
}

pub fn reasoning_support(endpoint: &LlmEndpoint, uses_responses_api: bool) -> LlmReasoningSupport {
    if let Some(kind) = reasoning_control_kind(endpoint, uses_responses_api) {
        return LlmReasoningSupport {
            supported: true,
            strategy: Some(kind.strategy_name().to_string()),
            summary: kind.summary().to_string(),
        };
    }

    let summary = if endpoint.api_format == ApiFormat::Anthropic {
        "当前 Anthropic 模型不在官方支持 extended thinking 的型号内，思考模式不可用。"
    } else if is_volcengine_like_endpoint(endpoint) {
        "当前火山方舟模型不在官方支持 thinking.type 的型号内，思考模式不可用。"
    } else if is_deepseek_like_endpoint(endpoint) {
        "当前 DeepSeek 模型未识别到官方 thinking 控制能力，思考模式不可用。"
    } else if is_siliconflow_like_endpoint(endpoint) {
        "当前 SiliconFlow 模型不在官方支持 thinking_budget 的推理模型范围内，思考模式不可用。"
    } else if is_cerebras_like_endpoint(endpoint) {
        "当前 Cerebras 模型未识别到官方 reasoning_effort 支持，思考模式不可用。"
    } else if openai_gpt5_reasoning_efforts(&endpoint.model).is_some() {
        "当前模型名看起来属于 GPT-5，但当前接口路径不支持对应的思考控制参数。"
    } else {
        "当前模型未识别到官方思考控制参数，思考模式不可用。"
    };

    LlmReasoningSupport {
        supported: false,
        strategy: None,
        summary: summary.to_string(),
    }
}

pub fn reasoning_support_for_mode(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> LlmReasoningSupport {
    let support = reasoning_support(endpoint, uses_responses_api);
    if mode == LlmReasoningMode::ProviderDefault || !support.supported {
        return support;
    }

    if is_auto_reasoning_endpoint(endpoint, uses_responses_api)
        && cached_auto_reasoning_strategy(endpoint, uses_responses_api, mode)
            == Some(AutoReasoningStrategy::NoControls)
    {
        return LlmReasoningSupport {
            supported: false,
            strategy: Some(
                AutoReasoningStrategy::NoControls
                    .strategy_name()
                    .to_string(),
            ),
            summary: "当前后端已拒绝该档位的思考控制参数，应用会按模型默认行为发送。".to_string(),
        };
    }

    support
}

fn auto_reasoning_intent(mode: LlmReasoningMode) -> &'static str {
    match mode {
        LlmReasoningMode::Off => "off",
        LlmReasoningMode::Light => "light",
        LlmReasoningMode::Balanced => "balanced",
        LlmReasoningMode::Deep => "deep",
        LlmReasoningMode::ProviderDefault => "provider_default",
    }
}

fn auto_reasoning_strategy_cache_key(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    intent: &str,
) -> String {
    format!(
        "{:?}|{}|{}|{}|{}",
        endpoint.api_format,
        endpoint.api_url,
        endpoint.model.trim().to_ascii_lowercase(),
        uses_responses_api,
        intent
    )
}

fn auto_reasoning_strategy_cache(
) -> &'static parking_lot::Mutex<HashMap<String, AutoReasoningStrategy>> {
    AUTO_REASONING_STRATEGY_CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

pub fn cached_auto_reasoning_strategy(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> Option<AutoReasoningStrategy> {
    auto_reasoning_strategy_cache()
        .lock()
        .get(&auto_reasoning_strategy_cache_key(
            endpoint,
            uses_responses_api,
            auto_reasoning_intent(mode),
        ))
        .copied()
}

pub fn remember_auto_reasoning_strategy(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    mode: LlmReasoningMode,
    strategy: AutoReasoningStrategy,
) {
    auto_reasoning_strategy_cache().lock().insert(
        auto_reasoning_strategy_cache_key(
            endpoint,
            uses_responses_api,
            auto_reasoning_intent(mode),
        ),
        strategy,
    );
}

pub fn is_auto_reasoning_endpoint(endpoint: &LlmEndpoint, uses_responses_api: bool) -> bool {
    reasoning_control_kind(endpoint, uses_responses_api)
        == Some(ReasoningControlKind::AutoOpenaiCompat)
}

fn preferred_auto_reasoning_strategy(
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> AutoReasoningStrategy {
    if mode == LlmReasoningMode::Off {
        AutoReasoningStrategy::TopLevelThinking
    } else if uses_responses_api {
        AutoReasoningStrategy::OpenaiResponsesReasoning
    } else {
        AutoReasoningStrategy::OpenaiChatReasoningEffort
    }
}

fn auto_reasoning_fallback_strategies(
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> &'static [AutoReasoningStrategy] {
    const RESPONSES: &[AutoReasoningStrategy] = &[AutoReasoningStrategy::OpenaiChatReasoningEffort];
    const CHAT_OFF: &[AutoReasoningStrategy] = &[AutoReasoningStrategy::ChatTemplateThinking];
    const CHAT_EFFORT: &[AutoReasoningStrategy] = &[AutoReasoningStrategy::TopLevelThinking];

    if mode == LlmReasoningMode::Off {
        CHAT_OFF
    } else if uses_responses_api {
        RESPONSES
    } else {
        CHAT_EFFORT
    }
}

fn auto_effort_for_mode(mode: LlmReasoningMode) -> Option<&'static str> {
    match mode {
        LlmReasoningMode::Off => None,
        LlmReasoningMode::Light => Some("low"),
        LlmReasoningMode::Balanced => Some("medium"),
        LlmReasoningMode::Deep => Some("high"),
        LlmReasoningMode::ProviderDefault => None,
    }
}

fn apply_auto_reasoning_strategy(
    body: &mut serde_json::Value,
    strategy: AutoReasoningStrategy,
    mode: LlmReasoningMode,
) {
    match strategy {
        AutoReasoningStrategy::OpenaiResponsesReasoning => {
            let Some(effort) = auto_effort_for_mode(mode) else {
                return;
            };
            body["reasoning"] = serde_json::json!({ "effort": effort });
        }
        AutoReasoningStrategy::OpenaiChatReasoningEffort => {
            let Some(effort) = auto_effort_for_mode(mode) else {
                return;
            };
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        AutoReasoningStrategy::TopLevelThinking => {
            let thinking_type = if mode == LlmReasoningMode::Off {
                "disabled"
            } else {
                "enabled"
            };
            body["thinking"] = serde_json::json!({ "type": thinking_type });
        }
        AutoReasoningStrategy::ChatTemplateThinking => {
            body["chat_template_kwargs"] =
                serde_json::json!({ "thinking": mode != LlmReasoningMode::Off });
        }
        AutoReasoningStrategy::NoControls => {}
    }
}

pub fn applied_auto_reasoning_strategy(body: &serde_json::Value) -> Option<AutoReasoningStrategy> {
    let map = body.as_object()?;
    if map.contains_key("thinking") {
        return Some(AutoReasoningStrategy::TopLevelThinking);
    }
    if map.contains_key("chat_template_kwargs") {
        return Some(AutoReasoningStrategy::ChatTemplateThinking);
    }
    if map.contains_key("reasoning") {
        return Some(AutoReasoningStrategy::OpenaiResponsesReasoning);
    }
    if map.contains_key("reasoning_effort") {
        return Some(AutoReasoningStrategy::OpenaiChatReasoningEffort);
    }
    None
}

pub fn auto_reasoning_fallback_bodies(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    request_body: &serde_json::Value,
    mode: LlmReasoningMode,
) -> Vec<(AutoReasoningStrategy, serde_json::Value)> {
    if !is_auto_reasoning_endpoint(endpoint, uses_responses_api)
        || mode == LlmReasoningMode::ProviderDefault
    {
        return Vec::new();
    }

    let cached = cached_auto_reasoning_strategy(endpoint, uses_responses_api, mode);
    if cached == Some(AutoReasoningStrategy::NoControls) {
        return Vec::new();
    }

    let current = applied_auto_reasoning_strategy(request_body).or(cached);
    let mut base = request_body.clone();
    strip_reasoning_controls(&mut base);

    auto_reasoning_fallback_strategies(uses_responses_api, mode)
        .iter()
        .copied()
        .filter(|strategy| Some(*strategy) != current)
        .map(|strategy| {
            let mut body = base.clone();
            apply_auto_reasoning_strategy(&mut body, strategy, mode);
            (strategy, body)
        })
        .collect()
}

pub fn apply_reasoning_controls(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    body: &mut serde_json::Value,
    mode: LlmReasoningMode,
) {
    let Some(kind) = reasoning_control_kind(endpoint, uses_responses_api) else {
        return;
    };

    match (kind, mode) {
        // Cerebras 推理模型：ProviderDefault 映射到 low（恢复重构前的硬编码行为，
        // 避免服务端默认推理强度过高导致 TTFT 和生成时间显著增加）
        (ReasoningControlKind::CerebrasReasoningEffort, LlmReasoningMode::ProviderDefault) => {
            body["reasoning_effort"] = serde_json::json!("low");
        }
        (_, LlmReasoningMode::ProviderDefault) => {}
        (ReasoningControlKind::AnthropicThinking, _) => {
            if mode == LlmReasoningMode::Off {
                return;
            }

            let budget_tokens = match mode {
                LlmReasoningMode::Light => 1_024,
                LlmReasoningMode::Balanced => 2_048,
                LlmReasoningMode::Deep => 4_096,
                _ => 1_024,
            };
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget_tokens,
            });
        }
        (ReasoningControlKind::VolcengineThinkingType, _) => {
            let thinking_type = match mode {
                LlmReasoningMode::Off => "disabled",
                LlmReasoningMode::Light | LlmReasoningMode::Balanced => "auto",
                LlmReasoningMode::Deep => "enabled",
                LlmReasoningMode::ProviderDefault => return,
            };
            body["thinking"] = serde_json::json!({ "type": thinking_type });
        }
        (ReasoningControlKind::DeepSeekThinkingToggle, _) => {
            let thinking_type = if mode == LlmReasoningMode::Off {
                "disabled"
            } else {
                "enabled"
            };
            body["thinking"] = serde_json::json!({ "type": thinking_type });
        }
        (ReasoningControlKind::SiliconFlowThinkingBudget, _) => {
            let thinking_budget = match mode {
                LlmReasoningMode::Off => {
                    body["enable_thinking"] = serde_json::json!(false);
                    return;
                }
                LlmReasoningMode::Light => 1024,
                LlmReasoningMode::Balanced => 4096,
                LlmReasoningMode::Deep => 8192,
                LlmReasoningMode::ProviderDefault => return,
            };
            body["enable_thinking"] = serde_json::json!(true);
            body["thinking_budget"] = serde_json::json!(thinking_budget);
        }
        (ReasoningControlKind::CerebrasReasoningEffort, _) => {
            let effort = match mode {
                LlmReasoningMode::Off => return,
                LlmReasoningMode::Light => "low",
                LlmReasoningMode::Balanced => "medium",
                LlmReasoningMode::Deep => "high",
                LlmReasoningMode::ProviderDefault => return,
            };
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        (ReasoningControlKind::CerebrasGlmToggle, _) => {
            body["disable_reasoning"] = serde_json::json!(mode == LlmReasoningMode::Off);
        }
        (ReasoningControlKind::AutoOpenaiCompat, _) => {
            let Some(strategy) = cached_auto_reasoning_strategy(endpoint, uses_responses_api, mode)
                .or_else(|| Some(preferred_auto_reasoning_strategy(uses_responses_api, mode)))
            else {
                return;
            };
            apply_auto_reasoning_strategy(body, strategy, mode);
        }
        (ReasoningControlKind::OpenaiEffort, _) => {
            let Some(effort) = openai_gpt5_effort_for_mode(&endpoint.model, mode) else {
                return;
            };

            if uses_responses_api {
                body["reasoning"] = serde_json::json!({ "effort": effort });
            } else {
                body["reasoning_effort"] = serde_json::json!(effort);
            }
        }
    }
}

pub fn strip_reasoning_controls(body: &mut serde_json::Value) {
    if let Some(map) = body.as_object_mut() {
        map.remove("reasoning");
        map.remove("reasoning_effort");
        map.remove("thinking");
        map.remove("chat_template_kwargs");
        map.remove("thinking_budget");
        map.remove("enable_thinking");
        map.remove("disable_reasoning");
    }
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
            .map_err(|_| "API Key 包含非法字符，无法作为 HTTP header 使用".to_string())
    };
    match api_format {
        ApiFormat::Anthropic => {
            headers.insert("x-api-key", parse(api_key)?);
            headers.insert("anthropic-version", parse("2023-06-01")?);
            headers.insert("content-type", parse("application/json")?);
        }
        ApiFormat::OpenaiCompat => {
            if let Some(token) = codex_oauth_service::decode_chatgpt_bearer_token(api_key) {
                headers.insert(
                    "Authorization",
                    parse(&format!("Bearer {}", token.access_token))?,
                );
                if let Some(account_id) = token
                    .account_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                {
                    headers.insert("ChatGPT-Account-ID", parse(account_id)?);
                }
                headers.insert("originator", parse(codex_oauth_service::ORIGINATOR)?);
                headers.insert(
                    "User-Agent",
                    parse(codex_oauth_service::CHATGPT_BEARER_USER_AGENT)?,
                );
            } else {
                let bearer_api_key = codex_oauth_service::decode_oauth_api_key(api_key)
                    .unwrap_or_else(|| api_key.to_string());
                headers.insert("Authorization", parse(&format!("Bearer {bearer_api_key}"))?);
            }
            headers.insert("Content-Type", parse("application/json")?);
        }
    }
    Ok(headers)
}

/// 保存或删除 API Key：非空则写入密钥环，空则删除
pub fn save_or_delete_api_key(app_handle: &tauri::AppHandle, keyring_user: &str, api_key: &str) {
    if !api_key.is_empty() {
        if let Err(e) = app_handle
            .keyring()
            .set_password(KEYRING_SERVICE, keyring_user, api_key)
        {
            log::warn!("保存 API Key 到密钥环失败: {e}");
        }
    } else {
        let _ = app_handle
            .keyring()
            .delete_password(KEYRING_SERVICE, keyring_user);
    }
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

pub fn sync_assistant_api_key(app_handle: &tauri::AppHandle, state: &AppState) {
    let provider = state.with_profile(|p| p.llm_provider.resolve_assistant_provider());
    let key = load_api_key_for_provider(app_handle, &provider);
    state.set_assistant_api_key(key);
}

pub fn sync_runtime_api_key(app_handle: &tauri::AppHandle, state: &AppState) -> String {
    let api_key = load_api_key_for_active_provider(app_handle, state);
    state.set_ai_polish_api_key(api_key.clone());
    sync_assistant_api_key(app_handle, state);
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
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            assistant_provider: None,
            custom_providers: Vec::new(),
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
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
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            assistant_provider: None,
            custom_providers: Vec::new(),
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
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
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            assistant_provider: None,
            custom_providers: Vec::new(),
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
        };

        let endpoint = endpoint_for_config(&config);

        assert_eq!(endpoint.provider, CUSTOM);
        assert_eq!(endpoint.api_url, "https://example.com/v1/chat/completions");
        assert_eq!(endpoint.model, "foo-model");
    }

    #[test]
    fn wrapped_oauth_openai_api_key_builds_plain_bearer_header() {
        let wrapped =
            codex_oauth_service::encode_oauth_api_key("sk-oauth-session").expect("wrapped key");

        let headers = build_auth_headers(&ApiFormat::OpenaiCompat, &wrapped)
            .expect("headers should build for wrapped OAuth key");

        assert_eq!(
            headers
                .get("Authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-oauth-session")
        );
        assert!(headers.get("ChatGPT-Account-ID").is_none());
        assert!(headers.get("originator").is_none());
        assert!(headers.get("User-Agent").is_none());
    }

    #[test]
    fn invalid_active_provider_falls_back_to_latest_custom_provider() {
        let config = LlmProviderConfig {
            active: "custom_missing".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            assistant_provider: None,
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
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
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
    fn assistant_endpoint_uses_separate_model_for_builtin_provider() {
        let config = LlmProviderConfig {
            active: CEREBRAS.to_string(),
            custom_base_url: None,
            custom_model: Some("gpt-oss-120b".to_string()),
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: true,
            assistant_model: Some("gpt-oss-20b".to_string()),
            assistant_provider: None,
            custom_providers: Vec::new(),
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
        };

        let endpoint = assistant_endpoint_for_config(&config);

        assert_eq!(endpoint.provider, CEREBRAS);
        assert_eq!(endpoint.model, "gpt-oss-20b");
        assert_eq!(
            endpoint.api_url,
            "https://api.cerebras.ai/v1/chat/completions"
        );
    }

    #[test]
    fn assistant_endpoint_uses_separate_model_for_custom_provider() {
        let config = LlmProviderConfig {
            active: "custom_a".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: Default::default(),
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: true,
            assistant_model: Some("assistant-model".to_string()),
            assistant_provider: None,
            custom_providers: vec![crate::state::user_profile::CustomProvider {
                id: "custom_a".to_string(),
                name: "Custom A".to_string(),
                base_url: "https://example.com".to_string(),
                model: "polish-model".to_string(),
                api_format: ApiFormat::OpenaiCompat,
            }],
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
        };

        let endpoint = assistant_endpoint_for_config(&config);

        assert_eq!(endpoint.provider, "custom_a");
        assert_eq!(endpoint.model, "assistant-model");
        assert_eq!(endpoint.api_url, "https://example.com/v1/chat/completions");
    }

    #[test]
    fn recognizes_image_unsupported_errors() {
        assert!(looks_like_image_input_unsupported_error(
            "API 返回错误 400: model does not support image input"
        ));
        assert!(looks_like_image_input_unsupported_error(
            "unsupported content type: input_image"
        ));
        assert!(looks_like_image_input_unsupported_error(
            "API 返回错误 400 Bad Request: Failed to deserialize the JSON body into the target type: messages[1]: unknown variant `image_url`, expected `text` at line 1 column 127501 (code: invalid_request_error)"
        ));
        assert!(!looks_like_image_input_unsupported_error(
            "API 返回错误 401: invalid api key"
        ));
    }

    #[test]
    fn builds_cerebras_image_support_probe_url() {
        let endpoint = endpoint_for_preview(
            CUSTOM,
            Some("https://gateway.ai.cloudflare.com/v1/account/openai/compat"),
            Some("Qwen/Qwen3-32B"),
            ApiFormat::OpenaiCompat,
        );

        assert_eq!(
            image_support_probe_url(&endpoint).as_deref(),
            Some(
                "https://gateway.ai.cloudflare.com/v1/account/openai/compat/v1/models/Qwen%2FQwen3-32B"
            )
        );
    }

    #[test]
    fn builds_cerebras_public_model_probe_url() {
        assert_eq!(
            cerebras_public_model_probe_url("Qwen/Qwen3-32B").as_deref(),
            Some("https://api.cerebras.ai/public/v1/models/Qwen%2FQwen3-32B?format=openrouter")
        );
    }

    #[test]
    fn only_cerebras_like_endpoints_use_public_model_probe() {
        let openai_endpoint =
            endpoint_for_preview(OPENAI, None, Some("gpt-4.1"), ApiFormat::OpenaiCompat);
        let cerebras_endpoint = endpoint_for_preview(
            CEREBRAS,
            None,
            Some("gpt-oss-120b"),
            ApiFormat::OpenaiCompat,
        );

        assert!(!should_probe_cerebras_public_model_metadata(
            &openai_endpoint
        ));
        assert!(should_probe_cerebras_public_model_metadata(
            &cerebras_endpoint
        ));
    }

    #[test]
    fn parses_text_only_model_metadata_as_no_image_support() {
        let payload = serde_json::json!({
            "id": "gpt-oss-120b",
            "input_modalities": ["text"]
        });

        assert_eq!(
            parse_image_input_support_from_model_metadata(&payload),
            Some(false)
        );
    }

    #[test]
    fn parses_multimodal_model_metadata_as_image_supported() {
        let payload = serde_json::json!({
            "id": "vision-model",
            "input_modalities": ["text", "image"]
        });

        assert_eq!(
            parse_image_input_support_from_model_metadata(&payload),
            Some(true)
        );
    }

    #[test]
    fn recognizes_json_output_unsupported_errors() {
        assert!(looks_like_json_output_unsupported_error(
            "API 返回错误 400 Bad Request: {\"error\":{\"code\":\"InvalidParameter\",\"message\":\"The parameter `response_format.type` specified in the request are not valid: `json_object` is not supported by this model.\",\"param\":\"response_format.type\",\"type\":\"BadRequest\"}}"
        ));
        assert!(looks_like_json_output_unsupported_error(
            "unsupported structured output: json schema is not supported"
        ));
        assert!(!looks_like_json_output_unsupported_error(
            "API 返回错误 401: invalid api key"
        ));
    }

    #[test]
    fn recognizes_reasoning_unsupported_errors() {
        assert!(looks_like_reasoning_unsupported_error(
            "API 返回错误 400: The parameter `thinking.type` is not supported by this model"
        ));
        assert!(looks_like_reasoning_unsupported_error(
            "unknown parameter: reasoning_effort"
        ));
        assert!(looks_like_reasoning_unsupported_error(
            "Extra inputs are not permitted (param: chat_template_kwargs)"
        ));
        assert!(!looks_like_reasoning_unsupported_error(
            "API 返回错误 401: invalid api key"
        ));
    }

    #[test]
    fn preview_endpoint_preserves_custom_provider_format() {
        let endpoint = endpoint_for_preview(
            "foo",
            Some("https://api.anthropic.com"),
            Some("claude-3-7-sonnet-latest"),
            ApiFormat::Anthropic,
        );

        assert_eq!(endpoint.api_format, ApiFormat::Anthropic);
        assert_eq!(endpoint.api_url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn responses_api_detection_is_case_insensitive_and_exact() {
        let responses = endpoint_for_preview(
            "responses-case",
            Some("https://example.com/V1/RESPONSES"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );
        let responses_suffix = endpoint_for_preview(
            "responses-suffix",
            Some("https://example.com/v1/responses-extra"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );

        assert!(endpoint_uses_responses_api(&responses));
        assert!(!endpoint_uses_responses_api(&responses_suffix));
    }

    #[test]
    fn volcan_seed_2_models_report_reasoning_support() {
        let endpoint = endpoint_for_preview(
            CUSTOM,
            Some("https://ark.cn-beijing.volces.com/api/v3"),
            Some("doubao-seed-2-0-mini-260215"),
            ApiFormat::OpenaiCompat,
        );

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported);
        assert_eq!(
            support.strategy.as_deref(),
            Some("volcengine_thinking_type")
        );
    }

    #[test]
    fn volcengine_like_unknown_model_reports_auto_reasoning_support() {
        let endpoint = endpoint_for_preview(
            CUSTOM,
            Some("https://ark.cn-beijing.volces.com/api/v3"),
            Some("doubao-future-reasoner"),
            ApiFormat::OpenaiCompat,
        );

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported);
        assert_eq!(
            support.strategy.as_deref(),
            Some("auto_openai_compat_probe")
        );
    }

    #[test]
    fn openai_non_reasoning_models_report_unsupported() {
        let endpoint =
            endpoint_for_preview(OPENAI, None, Some("gpt-4.1-mini"), ApiFormat::OpenaiCompat);

        let support = reasoning_support(&endpoint, false);

        assert!(!support.supported);
        assert!(support.summary.contains("不可用"));
    }

    #[test]
    fn unknown_openai_compatible_reports_auto_reasoning_support() {
        let endpoint = endpoint_for_preview(
            "fau",
            Some("https://hub.nhr.fau.de/api/llmgw/v1"),
            Some("moonshotai/Kimi-K2.6"),
            ApiFormat::OpenaiCompat,
        );

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported);
        assert_eq!(
            support.strategy.as_deref(),
            Some("auto_openai_compat_probe")
        );
    }

    #[test]
    fn custom_gateway_path_containing_openai_host_still_uses_auto_reasoning() {
        let endpoint = endpoint_for_preview(
            "custom-gateway",
            Some("https://gateway.example/api.openai.com/v1"),
            Some("gpt-5.2"),
            ApiFormat::OpenaiCompat,
        );

        let support = reasoning_support(&endpoint, false);

        assert_eq!(
            support.strategy.as_deref(),
            Some("auto_openai_compat_probe"),
            "only the actual URL host should identify the official OpenAI API"
        );
    }

    #[test]
    fn auto_openai_compatible_starts_with_chat_reasoning_effort() {
        let endpoint = endpoint_for_preview(
            "auto_reasoning_chat_start",
            Some("https://auto-reasoning-start.example/v1"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );
        let mut body = serde_json::json!({});

        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Deep);

        assert_eq!(body["reasoning_effort"], serde_json::json!("high"));
        assert!(body.get("thinking").is_none());
        assert!(body.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn auto_openai_compatible_off_starts_with_thinking_disabled() {
        let endpoint = endpoint_for_preview(
            "auto_reasoning_chat_off",
            Some("https://auto-reasoning-off.example/v1"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );
        let mut body = serde_json::json!({});

        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

        assert_eq!(body["thinking"], serde_json::json!({ "type": "disabled" }));
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn auto_openai_compatible_fallbacks_try_one_standard_alternate() {
        let endpoint = endpoint_for_preview(
            "auto_reasoning_fallback",
            Some("https://auto-reasoning-fallback.example/v1"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );
        let request_body = serde_json::json!({
            "model": "future-model",
            "messages": [],
            "reasoning_effort": "low",
        });

        let fallbacks =
            auto_reasoning_fallback_bodies(&endpoint, false, &request_body, LlmReasoningMode::Deep);

        assert_eq!(fallbacks.len(), 1);
        assert_eq!(fallbacks[0].0, AutoReasoningStrategy::TopLevelThinking);
        assert_eq!(
            fallbacks[0].1["thinking"],
            serde_json::json!({ "type": "enabled" })
        );
        assert!(fallbacks
            .iter()
            .all(|(_, body)| body.get("reasoning_effort").is_none()));
    }

    #[test]
    fn remembered_auto_strategy_is_reused_for_endpoint() {
        let endpoint = endpoint_for_preview(
            "auto_reasoning_cached",
            Some("https://auto-reasoning-cache.example/v1"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );

        remember_auto_reasoning_strategy(
            &endpoint,
            false,
            LlmReasoningMode::Balanced,
            AutoReasoningStrategy::OpenaiResponsesReasoning,
        );

        let mut body = serde_json::json!({});
        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Balanced);

        assert_eq!(body["reasoning"], serde_json::json!({ "effort": "medium" }));
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn remembered_no_controls_strategy_skips_future_auto_params() {
        let endpoint = endpoint_for_preview(
            "auto_reasoning_no_controls",
            Some("https://auto-reasoning-no-controls.example/v1"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );

        remember_auto_reasoning_strategy(
            &endpoint,
            false,
            LlmReasoningMode::Deep,
            AutoReasoningStrategy::NoControls,
        );

        let mut body = serde_json::json!({});
        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Deep);

        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn auto_support_for_mode_reports_no_controls_cache_as_unsupported() {
        let endpoint = endpoint_for_preview(
            "auto_reasoning_no_controls_support",
            Some("https://auto-reasoning-no-controls-support.example/v1"),
            Some("future-model"),
            ApiFormat::OpenaiCompat,
        );

        remember_auto_reasoning_strategy(
            &endpoint,
            false,
            LlmReasoningMode::Deep,
            AutoReasoningStrategy::NoControls,
        );

        let default_support =
            reasoning_support_for_mode(&endpoint, false, LlmReasoningMode::ProviderDefault);
        let deep_support = reasoning_support_for_mode(&endpoint, false, LlmReasoningMode::Deep);

        assert!(default_support.supported);
        assert!(!deep_support.supported);
        assert_eq!(
            deep_support.strategy.as_deref(),
            Some("auto_no_reasoning_controls")
        );
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn reasoning_efforts_for_modes(endpoint: &LlmEndpoint) -> Vec<String> {
        [
            LlmReasoningMode::Off,
            LlmReasoningMode::Light,
            LlmReasoningMode::Balanced,
            LlmReasoningMode::Deep,
        ]
        .into_iter()
        .map(|mode| {
            let mut body = serde_json::json!({});
            apply_reasoning_controls(endpoint, true, &mut body, mode);
            body["reasoning"]["effort"]
                .as_str()
                .expect("reasoning.effort should be set")
                .to_string()
        })
        .collect()
    }

    #[test]
    fn openai_gpt5_4_off_maps_reasoning_effort_to_none() {
        let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5.4"), ApiFormat::OpenaiCompat);
        let mut body = serde_json::json!({});

        apply_reasoning_controls(&endpoint, true, &mut body, LlmReasoningMode::Off);

        assert_eq!(body["reasoning"]["effort"], serde_json::json!("none"));
    }

    #[test]
    fn openai_gpt5_mini_keeps_minimal_family_mapping() {
        let endpoint =
            endpoint_for_preview(OPENAI, None, Some("gpt-5-mini"), ApiFormat::OpenaiCompat);

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["minimal", "low", "medium", "high"])
        );
    }

    #[test]
    fn openai_gpt5_2_off_maps_reasoning_effort_to_none() {
        let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5.2"), ApiFormat::OpenaiCompat);

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["none", "low", "medium", "high"])
        );
    }

    #[test]
    fn openai_gpt5_pro_uses_high_only() {
        let endpoint =
            endpoint_for_preview(OPENAI, None, Some("gpt-5-pro"), ApiFormat::OpenaiCompat);

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["high", "high", "high", "high"])
        );
    }

    #[test]
    fn openai_gpt5_2_codex_clamps_to_low_then_rises() {
        let endpoint =
            endpoint_for_preview(OPENAI, None, Some("gpt-5.2-codex"), ApiFormat::OpenaiCompat);

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["low", "medium", "high", "xhigh"])
        );
    }

    #[test]
    fn openai_gpt5_5_reaches_xhigh() {
        let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5.5"), ApiFormat::OpenaiCompat);

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["low", "medium", "high", "xhigh"])
        );
        assert_eq!(
            reasoning_support(&endpoint, true).strategy.as_deref(),
            Some("openai_reasoning_effort")
        );
    }

    #[test]
    fn openai_gpt5_6_family_supports_off_through_deep() {
        for model in ["gpt-5.6", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
            let endpoint = endpoint_for_preview(OPENAI, None, Some(model), ApiFormat::OpenaiCompat);

            assert_eq!(
                reasoning_efforts_for_modes(&endpoint),
                strings(&["low", "medium", "high", "xhigh"]),
                "unexpected reasoning mapping for {model}"
            );
            assert_eq!(
                reasoning_support(&endpoint, true).strategy.as_deref(),
                Some("openai_reasoning_effort")
            );
        }
    }

    #[test]
    fn openai_gpt5_1_codex_max_reaches_xhigh() {
        let endpoint = endpoint_for_preview(
            OPENAI,
            None,
            Some("gpt-5.1-codex-max"),
            ApiFormat::OpenaiCompat,
        );

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["none", "medium", "high", "xhigh"])
        );
    }

    #[test]
    fn deepseek_reasoner_reports_reasoning_support() {
        let endpoint = endpoint_for_preview(
            DEEPSEEK,
            None,
            Some("deepseek-reasoner"),
            ApiFormat::OpenaiCompat,
        );

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported);
        assert_eq!(support.strategy.as_deref(), Some("deepseek_thinking"));
    }

    #[test]
    fn deepseek_v4_flash_reports_reasoning_support() {
        let endpoint = endpoint_for_preview(
            DEEPSEEK,
            None,
            Some("deepseek-v4-flash"),
            ApiFormat::OpenaiCompat,
        );

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported);
        assert_eq!(support.strategy.as_deref(), Some("deepseek_thinking"));
    }

    #[test]
    fn known_openai_compatible_provider_unknown_model_reports_auto_reasoning_support() {
        let endpoint = endpoint_for_preview(
            DEEPSEEK,
            None,
            Some("deepseek-future-reasoner"),
            ApiFormat::OpenaiCompat,
        );

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported);
        assert_eq!(
            support.strategy.as_deref(),
            Some("auto_openai_compat_probe")
        );
    }

    #[test]
    fn known_openai_compatible_provider_unknown_model_starts_with_auto_probe() {
        let endpoint = endpoint_for_preview(
            SILICONFLOW,
            None,
            Some("Future/Reasoner-Next"),
            ApiFormat::OpenaiCompat,
        );
        let mut body = serde_json::json!({});

        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Deep);

        assert_eq!(body["reasoning_effort"], serde_json::json!("high"));
        assert!(body.get("enable_thinking").is_none());
        assert!(body.get("thinking_budget").is_none());
    }

    #[test]
    fn deepseek_v4_flash_off_disables_thinking() {
        let endpoint = endpoint_for_preview(
            DEEPSEEK,
            None,
            Some("deepseek-v4-flash"),
            ApiFormat::OpenaiCompat,
        );
        let mut body = serde_json::json!({});

        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

        assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn siliconflow_off_sends_enable_thinking_false() {
        let endpoint = endpoint_for_preview(
            SILICONFLOW,
            None,
            Some("Qwen/Qwen3-32B"),
            ApiFormat::OpenaiCompat,
        );
        let mut body = serde_json::json!({});

        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

        assert_eq!(body["enable_thinking"], serde_json::json!(false));
        assert!(body.get("thinking_budget").is_none());
    }

    #[test]
    fn siliconflow_reasoning_budget_enables_thinking() {
        let endpoint = endpoint_for_preview(
            SILICONFLOW,
            None,
            Some("Qwen/Qwen3-32B"),
            ApiFormat::OpenaiCompat,
        );
        let mut body = serde_json::json!({});

        apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Balanced);

        assert_eq!(body["enable_thinking"], serde_json::json!(true));
        assert_eq!(body["thinking_budget"], serde_json::json!(4096));
    }

    #[test]
    fn cerebras_glm_reports_reasoning_support() {
        let endpoint =
            endpoint_for_preview(CEREBRAS, None, Some("zai-glm-4.7"), ApiFormat::OpenaiCompat);

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported);
        assert_eq!(
            support.strategy.as_deref(),
            Some("cerebras_disable_reasoning")
        );
    }
}
