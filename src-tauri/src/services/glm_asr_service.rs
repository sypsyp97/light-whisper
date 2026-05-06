use std::io::Cursor;

use serde::Deserialize;

use crate::services::funasr_service::TranscriptionResult;
use crate::state::AppState;
use crate::utils::{paths, AppError};

const GLM_ASR_PATH: &str = "/api/paas/v4/audio/transcriptions";
const GLM_ASR_MODEL: &str = "glm-asr-2512";
const REQUEST_TIMEOUT_SECS: u64 = 30;
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const MAX_AUDIO_DURATION_SECS: f64 = 30.0;

#[derive(Deserialize)]
struct GlmResponse {
    text: Option<String>,
    #[serde(default)]
    code: Option<i64>,
    message: Option<String>,
}

fn build_form(audio_data: Vec<u8>, state: &AppState) -> Result<reqwest::multipart::Form, AppError> {
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

fn validate_audio_payload(audio_data: &[u8]) -> Result<(), AppError> {
    if audio_data.len() > MAX_AUDIO_BYTES {
        return Err(AppError::Asr(format!(
            "GLM-ASR 音频过大：{:.1} MiB，超过 25 MiB 上传上限",
            audio_data.len() as f64 / 1024.0 / 1024.0
        )));
    }

    match hound::WavReader::new(Cursor::new(audio_data)) {
        Ok(reader) => {
            let spec = reader.spec();
            if spec.sample_rate > 0 {
                let duration_sec = reader.duration() as f64 / spec.sample_rate as f64;
                if duration_sec > MAX_AUDIO_DURATION_SECS {
                    return Err(AppError::Asr(format!(
                        "GLM-ASR 音频时长过长：{:.1} 秒，超过 30 秒上限",
                        duration_sec
                    )));
                }
            }
        }
        Err(err) => {
            log::debug!("GLM-ASR WAV 时长预检跳过：{}", err);
        }
    }

    Ok(())
}

pub async fn transcribe(
    state: &AppState,
    audio_data: Vec<u8>,
) -> Result<TranscriptionResult, AppError> {
    validate_audio_payload(&audio_data)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 16_000;

    fn wav_with_duration_secs(seconds: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
            for _ in 0..(SAMPLE_RATE * seconds) {
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        }
        cursor.into_inner()
    }

    fn asr_error_message(err: AppError) -> String {
        match err {
            AppError::Asr(message) => message,
            other => format!("{:?}", other),
        }
    }

    #[test]
    fn accepts_small_valid_wav_payload() {
        let wav = wav_with_duration_secs(1);

        validate_audio_payload(&wav).expect("small valid WAV should pass GLM-ASR payload guard");
    }

    #[test]
    fn rejects_payload_larger_than_25_mib() {
        let payload = vec![0u8; 25 * 1024 * 1024 + 1];

        let err = validate_audio_payload(&payload).expect_err("oversized GLM-ASR payload failed");
        let message = asr_error_message(err);

        assert!(
            message.contains("GLM-ASR"),
            "error should identify GLM-ASR: {message}"
        );
        assert!(
            message.contains("25 MB") || message.contains("25 MiB"),
            "error should mention the 25 MB limit: {message}"
        );
    }

    #[test]
    fn rejects_wav_payload_longer_than_30_seconds() {
        let wav = wav_with_duration_secs(31);

        let err = validate_audio_payload(&wav).expect_err("long GLM-ASR WAV payload failed");
        let message = asr_error_message(err);

        assert!(
            message.contains("GLM-ASR"),
            "error should identify GLM-ASR: {message}"
        );
        assert!(
            message.contains("30")
                && (message.to_ascii_lowercase().contains("second") || message.contains('秒')),
            "error should mention the 30 second limit: {message}"
        );
    }
}
