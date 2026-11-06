use std::time::Duration;

use crate::services::llm_client::{self, LlmImageInput, LlmRequestOptions, LlmUserInput};
use crate::services::llm_provider::LlmEndpoint;
use crate::services::{jev_service, jev_tasks};
use crate::state::user_profile::JevConfig;

const LLM_TIMEOUT: Duration = Duration::from_secs(30);
// A deterministic 32 × 32 solid-red RGB PNG. Never capture the user's screen for a test.
const TEST_IMAGE: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKElEQVR4nO3NsQ0AAAzCMP5/un0CNkuZ41wybXsHAAAAAAAAAAAAxR4yw/wuPL6QkAAAAABJRU5ErkJggg==";
pub(crate) const TEST_AUDIO: &[u8] = include_bytes!("../../resources/model_connection.wav");

pub(crate) async fn probe_online_asr(
    client: &reqwest::Client,
    engine: &str,
    base_url: &str,
    model: &str,
    api_key: &str,
) -> Result<(), String> {
    use crate::services::{alibaba_asr_service, glm_asr_service};
    use crate::state::AppState;

    if api_key.trim().is_empty() {
        return Err("未配置语音模型 API Key。".into());
    }
    // Reuse the real ASR transports with an empty profile: never send private hot words.
    let state = AppState {
        http_client: client.clone(),
        ..Default::default()
    };
    let base_url = base_url.trim().trim_end_matches('/');
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        match engine {
            "glm-asr" => {
                glm_asr_service::transcribe_request(
                    &state,
                    TEST_AUDIO.to_vec(),
                    &format!("{base_url}/api/paas/v4/audio/transcriptions"),
                    api_key.trim(),
                )
                .await
            }
            "alibaba-asr" if crate::utils::paths::alibaba_model_uses_omni_chat(model) => {
                alibaba_asr_service::transcribe_via_omni_chat(
                    &state,
                    base_url,
                    model,
                    api_key.trim(),
                    TEST_AUDIO.to_vec(),
                )
                .await
            }
            "alibaba-asr" => {
                alibaba_asr_service::transcribe_via_dashscope_asr(
                    &state,
                    base_url,
                    model,
                    api_key.trim(),
                    TEST_AUDIO.to_vec(),
                )
                .await
            }
            _ => Err(crate::utils::AppError::Asr("未知的在线语音识别引擎".into())),
        }
    })
    .await
    .map_err(|_| "语音模型测试超时（60 秒），请检查网络后重试。".to_string())?
    .map_err(|error| connection_error(&error.to_string()))?;
    if !result.success || result.text.trim().is_empty() {
        return Err("语音模型未能识别内置测试语音，请检查模型配置。".into());
    }
    Ok(())
}

/// Keep actionable error categories without returning provider bodies, URLs or credentials.
pub(crate) fn connection_error(message: &str) -> String {
    for prefix in ["API 返回错误 ", "HTTP "] {
        if let Some((_, rest)) = message.split_once(prefix) {
            if let Some(code) = rest.split(|c: char| !c.is_ascii_digit()).next() {
                if code.len() == 3 {
                    let hint = match code {
                        "400" | "422" => "请求参数不被支持，请检查模型名称和接口协议。",
                        "401" => "认证失败，请检查 API Key 或重新登录。",
                        "402" => "账户余额或额度不足，请检查服务商账户。",
                        "403" => "无访问权限，请检查模型权限、地区和账户限制。",
                        "404" => "模型或接口不存在，请检查模型名称和 API 地址。",
                        "429" => "请求受限，请检查额度或稍后重试。",
                        _ => "服务请求失败，请检查服务状态后重试。",
                    };
                    return format!("HTTP {code}：{hint}");
                }
            }
        }
    }
    let lower = message.to_ascii_lowercase();
    if message.contains("语音识别正在进行") {
        "语音识别正在进行，请稍后测试。".into()
    } else if message.contains("语音引擎已切换") {
        "语音引擎已切换，请重新测试。".into()
    } else if lower.contains("timeout") || lower.contains("timed out") || message.contains("超时")
    {
        "模型响应超时，请检查网络或稍后重试。".into()
    } else if message.contains("API Key 包含非法字符") || message.contains("请求头") {
        "认证信息格式无效，请检查 API Key。".into()
    } else if message.contains("请求失败") || message.contains("连接失败") {
        "连接失败，请检查网络、代理、证书和 API 地址。".into()
    } else if message.contains("响应为空") {
        "模型未返回可用内容，请检查模型配置后重试。".into()
    } else {
        "模型响应无效或未完成，请检查模型和接口协议后重试。".into()
    }
}

pub(crate) async fn probe_llm(
    client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
    vision: bool,
    options: LlmRequestOptions<'_>,
) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("未配置模型认证，请填写 API Key 或登录后重试。".into());
    }
    let input = LlmUserInput {
        text: if vision {
            "Name the color in this test image."
        } else {
            "Reply with OK."
        }
        .into(),
        images: if vision {
            vec![LlmImageInput {
                mime_type: "image/png".into(),
                data_base64: TEST_IMAGE.into(),
            }]
        } else {
            Vec::new()
        },
    };
    let body = llm_client::build_llm_body(
        endpoint,
        "This is a connection test. Answer briefly.",
        &input,
        options,
    );
    tokio::time::timeout(LLM_TIMEOUT, async {
        let content = llm_client::send_llm_request(
            client,
            endpoint,
            api_key,
            &body,
            input.text.len(),
            None,
            options,
        )
        .await?;
        llm_client::ensure_non_empty_llm_content(content, endpoint, "connection_test").map(|_| ())
    })
    .await
    .map_err(|_| "模型测试超时（30 秒），请检查网络或稍后重试。".to_string())?
    .map_err(|error| connection_error(&error))
}

pub(crate) async fn probe_decision(
    client: &reqwest::Client,
    config: &JevConfig,
    api_key: &str,
) -> Result<(), String> {
    // A valid decision proves connectivity even when it asks for polishing or is uncertain.
    jev_tasks::evaluate_with_warning(
        client,
        config,
        api_key,
        serde_json::json!({"text": "Hello.", "polishing_policy": "Keep clear, grammatical text unchanged."}),
        jev_service::route_questions(),
        jev_service::REQUEST_TIMEOUT,
        None,
    )
    .await?
    .ok_or_else(|| "决策模型未返回有效结果。".to_string())
    .map(|_| ())
}

#[cfg(test)]
#[path = "model_connection_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "model_connection_asr_tests.rs"]
mod asr_tests;
