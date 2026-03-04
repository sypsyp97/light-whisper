use std::sync::atomic::Ordering;

use tauri::Emitter;

use crate::services::llm_provider;
use crate::services::profile_service;
use crate::state::AppState;

const BASE_SYSTEM_PROMPT: &str = "\
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

/// 构建动态 system prompt，注入用户画像中的热词和纠错模式
fn build_system_prompt(state: &AppState) -> String {
    let mut prompt = BASE_SYSTEM_PROMPT.to_string();

    let profile = match state.user_profile.lock() {
        Ok(p) => p.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    // 注入用户常用词汇（Top 50）
    let hot_words = profile.get_hot_word_texts(50);
    if !hot_words.is_empty() {
        prompt.push_str("\n\n用户常用专有名词（优先使用这些词汇）：\n");
        prompt.push_str(&hot_words.join("、"));
    }

    // 注入历史纠错模式（Top 10）
    let corrections = profile.get_top_corrections(10);
    if !corrections.is_empty() {
        prompt.push_str("\n\n历史纠错参考（ASR 常见错误模式）：\n");
        for c in &corrections {
            prompt.push_str(&format!("- \"{}\" → \"{}\"\n", c.original, c.corrected));
        }
    }

    prompt
}

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

    // 获取 LLM 端点配置
    let endpoint = {
        let profile = match state.user_profile.lock() {
            Ok(p) => p.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        llm_provider::get_endpoint(
            &profile.llm_provider.active,
            profile.llm_provider.custom_base_url.as_deref(),
            profile.llm_provider.custom_model.as_deref(),
        )
    };

    // 构建动态 prompt
    let system_prompt = build_system_prompt(state);

    let is_responses_api = endpoint.api_url.contains("/v1/responses");

    let mut body = if is_responses_api {
        // OpenAI Responses API 格式
        serde_json::json!({
            "model": endpoint.model,
            "instructions": system_prompt,
            "input": text,
            "max_output_tokens": 1024,
        })
    } else {
        // OpenAI Chat Completions 兼容格式
        let messages = vec![
            serde_json::json!({ "role": "system", "content": system_prompt }),
            serde_json::json!({ "role": "user", "content": text }),
        ];
        serde_json::json!({
            "model": endpoint.model,
            "messages": messages,
            "max_tokens": 1024,
            "temperature": 0.0,
        })
    };

    // Cerebras 特有参数
    if endpoint.api_url.contains("cerebras") {
        body["reasoning_effort"] = serde_json::json!("low");
    }

    let start = std::time::Instant::now();
    emit_polish_status(app_handle, "polishing", text, "", "");

    let response = state
        .http_client
        .post(&endpoint.api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(endpoint.timeout_secs))
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

    let polished = if is_responses_api {
        // Responses API: output[0].content[0].text
        json["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap_or(text)
    } else {
        // Chat Completions API: choices[0].message.content
        json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or(text)
    }
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

        // 从纠正中学习
        if let Ok(mut profile) = state.user_profile.lock() {
            profile_service::learn_from_correction(&mut profile, text, &polished);
            // 异步保存（不阻塞返回）
            let profile_clone = profile.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = profile_service::save_profile_async(&profile_clone).await {
                    log::warn!("保存用户画像失败: {}", e);
                }
            });
        }
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
