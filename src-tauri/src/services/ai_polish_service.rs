use std::sync::atomic::Ordering;

use tauri::Emitter;

use crate::state::AppState;

const CEREBRAS_API_URL: &str = "https://api.cerebras.ai/v1/chat/completions";
const MODEL: &str = "gpt-oss-120b";
const TIMEOUT_SECS: u64 = 5;

const SYSTEM_PROMPT: &str = "\
你是资深的语音识别文本校对专家。以下文本来自ASR语音识别，请根据语境推断用户的真实意图，校正文本。

校正规则：
1. 同音字/近音字：语音识别最常见的错误。根据上下文语义判断正确用字（例：「事业」vs「视野」、「期待」vs「奇袋」）
2. 人称代词：根据语境修正混淆的你/您、他/她/它
3. 标点断句：修正断句不合理、语气符号缺失的问题
4. 口语化表达：轻度书面化，去除口头禅和重复词（嗯、啊、然后然后、就是就是），但保留说话者的原始语气和风格
5. 数字格式：根据语境使用合适的数字格式

禁止事项：
- 禁止添加原文没有的信息
- 禁止改变原文的意思和意图
- 禁止输出任何解释、注释或推理过程

直接输出校正后的文本。";

pub async fn polish_text(
    state: &AppState,
    text: &str,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    if !state.ai_polish_enabled.load(Ordering::Acquire) {
        return Ok(text.to_string());
    }

    let api_key = match state.ai_polish_api_key.lock() {
        Ok(key) => key.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    if api_key.is_empty() {
        log::warn!("AI 润色已启用但未配置 API Key，跳过润色");
        return Ok(text.to_string());
    }

    let messages = vec![
        serde_json::json!({ "role": "system", "content": SYSTEM_PROMPT }),
        serde_json::json!({ "role": "user", "content": text }),
    ];

    let body = serde_json::json!({
        "model": MODEL,
        "messages": messages,
        "max_tokens": 1024,
        "temperature": 0.0,
        "reasoning_effort": "low",
    });

    let start = std::time::Instant::now();

    let response = state
        .http_client
        .post(CEREBRAS_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("AI 润色请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let err = format!("AI 润色 API 返回错误 {}: {}", status, body_text);
        emit_polish_status(app_handle, "error", text, text, &err);
        return Err(err);
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("AI 润色响应解析失败: {}", e))?;

    let polished = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or(text)
        .trim()
        .to_string();

    if polished.is_empty() {
        return Ok(text.to_string());
    }

    let elapsed_ms = start.elapsed().as_millis();
    let changed = polished != text;

    if changed {
        log::info!(
            "AI 润色完成 ({}ms): \"{}\" -> \"{}\"",
            elapsed_ms,
            text,
            polished
        );
        emit_polish_status(app_handle, "applied", text, &polished, "");
    } else {
        log::info!("AI 润色完成 ({}ms): 文本无变化", elapsed_ms);
        emit_polish_status(app_handle, "unchanged", text, &polished, "");
    }

    Ok(polished)
}

fn emit_polish_status(
    app_handle: &tauri::AppHandle,
    status: &str,
    original: &str,
    polished: &str,
    error: &str,
) {
    let _ = app_handle.emit(
        "ai-polish-status",
        serde_json::json!({
            "status": status,
            "original": original,
            "polished": polished,
            "error": error,
        }),
    );
}
