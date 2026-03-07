use serde::Deserialize;

use crate::state::AppState;
use crate::services::funasr_service::TranscriptionResult;
use crate::utils::{paths, AppError};

const GLM_ASR_PATH: &str = "/api/paas/v4/audio/transcriptions";
const GLM_ASR_MODEL: &str = "glm-asr-2512";
const REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Deserialize)]
struct GlmResponse {
    text: Option<String>,
    #[serde(default)]
    code: Option<i64>,
    message: Option<String>,
}

fn build_form(
    audio_data: Vec<u8>,
    state: &AppState,
) -> Result<reqwest::multipart::Form, AppError> {
    let file_part = reqwest::multipart::Part::bytes(audio_data)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| AppError::Asr(format!("构建 multipart 失败: {}", e)))?;

    let mut form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", GLM_ASR_MODEL)
        .text("stream", "false");

    let words = state.with_profile(|p| p.get_hot_word_texts(100));
    if !words.is_empty() {
        let json = serde_json::to_string(&words)
            .map_err(|e| AppError::Asr(format!("序列化热词失败: {}", e)))?;
        form = form.text("hotwords", json);
    }

    Ok(form)
}

pub async fn transcribe(
    state: &AppState,
    audio_data: Vec<u8>,
) -> Result<TranscriptionResult, AppError> {
    let api_key = state.read_online_asr_api_key();
    if api_key.is_empty() {
        return Err(AppError::Asr("GLM-ASR API Key 未配置".into()));
    }

    let url = format!("{}{}", paths::read_online_asr_endpoint(), GLM_ASR_PATH);
    let form = build_form(audio_data, state)?;

    let resp = state
        .http_client
        .post(&url)
        .bearer_auth(&api_key)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| AppError::Asr(format!("GLM-ASR 请求失败: {}", e)))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Asr(format!("读取 GLM-ASR 响应失败: {}", e)))?;

    if !status.is_success() {
        return Err(AppError::Asr(format!("GLM-ASR HTTP {}: {}", status, body)));
    }

    let parsed: GlmResponse = serde_json::from_str(&body)
        .map_err(|e| AppError::Asr(format!("解析 GLM-ASR 响应失败: {}", e)))?;

    if let Some(code) = parsed.code {
        if code != 0 {
            return Ok(TranscriptionResult {
                text: String::new(),
                duration: None,
                success: false,
                error: parsed.message.or(Some(format!("GLM-ASR 错误码: {}", code))),
                language: None,
            });
        }
    }

    Ok(TranscriptionResult {
        text: parsed.text.unwrap_or_default(),
        duration: None,
        success: true,
        error: None,
        language: None,
    })
}
