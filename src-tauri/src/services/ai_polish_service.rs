use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::Emitter;

use serde::Deserialize;

use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::services::profile_service;
use crate::state::user_profile::{ApiFormat, CorrectionSource};
use crate::state::AppState;

/// LLM 结构化输出
#[derive(Deserialize)]
struct StructuredResponse {
    polished: String,
    #[serde(default)]
    corrections: Vec<CorrectionItem>,
    #[serde(default)]
    key_terms: Vec<String>,
}

#[derive(Deserialize)]
struct CorrectionItem {
    original: String,
    corrected: String,
    /// homophone | term | pronoun | style
    #[serde(default)]
    r#type: String,
}

const BASE_SYSTEM_PROMPT: &str = "\
你是 ASR 文本校正器。以下文本来自语音识别，请修复识别错误。

【硬约束——不可违反】
- 不补充原文没有的信息
- 不改变说话人的立场、语气和措辞（除非该处明显是 ASR 识别错误）
- 不输出任何解释、注释或推理过程
- 拿不准时保持原文不变。宁可漏纠，不要过度修正

【允许修复的范围】
1. 同音字/近音字：根据上下文语义判断正确用字（例：「事业」vs「视野」、「期待」vs「奇袋」）
2. 人称代词：根据语境修正混淆的他/她/它。不要把「你」改成「您」——敬语升格不属于 ASR 纠错
3. 标点断句：修正断句不合理、语气符号缺失的问题
4. 明显无语义重复：去除口头禅和无意义重复（嗯、啊、然后然后、就是就是），但保留说话者的用词习惯，不要书面化改写
5. 数字格式：根据语境使用合适的数字格式
6. 口述符号转换：当用户明确口述符号名称时转为对应符号（如「大于」→>、「左括号」→(、「百分号」→%、「逗号」→，、「换行」→实际换行）。\
   注意：只在明确口述符号名称时转换，正常用作动词的词不转换

【条件性规则——仅在满足条件时执行】
- 自我修正：仅当出现明确修正信号词（如「不对」「我的意思是」「算了换个说法」「sorry I mean」「actually」）时，保留最终说法，丢弃被否定部分
- 列举格式：仅当检测到明确枚举结构（「第一、第二」或「首先、其次」）时，使用编号列表格式化
- 段落分隔：仅当用户口述分段内容时，用空行分隔段落
- 格式适配：如果输入带有 [当前应用] 上下文，仅调整格式（换行、段落、标点密度），不改写措辞：\
   聊天软件 → 紧凑，避免多余换行；邮件 → 段落分明；文档/笔记 → 结构化；代码编辑器/终端 → 保留原始格式

以 JSON 输出（无 markdown 代码块）：
{\"polished\":\"校正后文本\",\"corrections\":[{\"original\":\"原片段\",\"corrected\":\"纠正片段\",\"type\":\"homophone|term|pronoun|style\"}],\"key_terms\":[\"专有名词\"]}
corrections 只记录词/短语级替换（2-8字），不记录整句改写。key_terms 只列重要专有名词。
无需修改时 polished 与输入相同，corrections 和 key_terms 为空数组。";

/// 根据文本长度动态计算超时时间
fn dynamic_timeout(base_secs: u64, text_len: usize) -> Duration {
    // 每 200 字符额外 1 秒，封顶 120 秒
    let extra = (text_len / 200) as u64;
    Duration::from_secs(base_secs.saturating_add(extra).min(120))
}

/// 从 SSE 流中累积 Chat Completions delta 内容，并向前端推送流式进度
async fn read_sse_stream(
    response: reqwest::Response,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(30);

    let mut stream = response.bytes_stream().eventsource();

    loop {
        match tokio::time::timeout(event_timeout, stream.next()).await {
            Ok(Some(Ok(event))) => {
                let data = event.data.trim();
                if data == "[DONE]" {
                    return Ok(accumulated);
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                        accumulated.push_str(content);
                        token_count += 1;
                        let _ = app_handle.emit(
                            "ai-polish-status",
                            serde_json::json!({
                                "status": "streaming",
                                "tokens": token_count,
                            }),
                        );
                    }
                }
            }
            Ok(Some(Err(e))) => return Err(format!("流式读取失败: {}", e)),
            Ok(None) => return Ok(accumulated),
            Err(_) => return Err("流式读取超时（30 秒无数据）".into()),
        }
    }
}

/// 从 Anthropic SSE 流中累积文本内容
async fn read_anthropic_sse_stream(
    response: reqwest::Response,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    use eventsource_stream::Eventsource;
    use tokio_stream::StreamExt;

    let mut accumulated = String::new();
    let mut token_count: usize = 0;
    let event_timeout = Duration::from_secs(30);

    let mut stream = response.bytes_stream().eventsource();

    loop {
        match tokio::time::timeout(event_timeout, stream.next()).await {
            Ok(Some(Ok(event))) => {
                match event.event.as_str() {
                    "content_block_delta" => {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&event.data) {
                            if let Some(text) = json["delta"]["text"].as_str() {
                                accumulated.push_str(text);
                                token_count += 1;
                                let _ = app_handle.emit(
                                    "ai-polish-status",
                                    serde_json::json!({
                                        "status": "streaming",
                                        "tokens": token_count,
                                    }),
                                );
                            }
                        }
                    }
                    "message_stop" => return Ok(accumulated),
                    "error" => {
                        let msg = serde_json::from_str::<serde_json::Value>(&event.data)
                            .ok()
                            .and_then(|j| j["error"]["message"].as_str().map(String::from))
                            .unwrap_or_else(|| event.data.clone());
                        return Err(format!("Anthropic 流式错误: {}", msg));
                    }
                    // message_start, content_block_start, content_block_stop, message_delta, ping
                    _ => {}
                }
            }
            Ok(Some(Err(e))) => return Err(format!("流式读取失败: {}", e)),
            Ok(None) => return Ok(accumulated),
            Err(_) => return Err("流式读取超时（30 秒无数据）".into()),
        }
    }
}

/// 构建 LLM 请求 body，返回 (body, use_streaming)
fn build_llm_body(
    endpoint: &LlmEndpoint,
    system_prompt: &str,
    user_content: &str,
) -> (serde_json::Value, bool) {
    match endpoint.api_format {
        ApiFormat::Anthropic => {
            let body = serde_json::json!({
                "model": endpoint.model,
                "max_tokens": 4096,
                "system": system_prompt,
                "messages": [{"role": "user", "content": user_content}],
                "stream": true,
            });
            (body, true)
        }
        ApiFormat::OpenaiCompat => {
            let is_responses_api = endpoint.api_url.contains("/v1/responses");
            let use_streaming = !is_responses_api;

            let mut body = if is_responses_api {
                serde_json::json!({
                    "model": endpoint.model,
                    "instructions": system_prompt,
                    "input": [
                        {"role": "developer", "content": "Output json."},
                        {"role": "user", "content": user_content},
                    ],
                    "text": { "format": { "type": "json_object" } }
                })
            } else {
                serde_json::json!({
                    "model": endpoint.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_content},
                    ],
                    "response_format": { "type": "json_object" }
                })
            };

            if is_responses_api {
                body["reasoning"] = serde_json::json!({"effort": "medium"});
            } else if endpoint.api_url.contains("cerebras") {
                body["reasoning_effort"] = serde_json::json!("low");
            }
            if use_streaming {
                body["stream"] = serde_json::json!(true);
            }

            (body, use_streaming)
        }
    }
}

/// 从非流式响应中提取文本内容
fn extract_content(endpoint: &LlmEndpoint, json: &serde_json::Value) -> Option<String> {
    match endpoint.api_format {
        ApiFormat::Anthropic => json["content"]
            .as_array()
            .and_then(|arr| arr.iter().find_map(|b| b["text"].as_str().map(String::from))),
        ApiFormat::OpenaiCompat => {
            if endpoint.api_url.contains("/v1/responses") {
                json["output"].as_array().and_then(|outputs| {
                    outputs.iter().find_map(|item| {
                        if item["type"].as_str() == Some("message") {
                            item["content"][0]["text"].as_str().map(String::from)
                        } else {
                            None
                        }
                    })
                })
            } else {
                json["choices"][0]["message"]["content"]
                    .as_str()
                    .map(String::from)
            }
        }
    }
}

/// 发送请求并获取响应文本
async fn send_llm_request(
    state: &AppState,
    endpoint: &LlmEndpoint,
    api_key: &str,
    system_prompt: &str,
    user_content: &str,
    text_len: usize,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    let (body, use_streaming) = build_llm_body(endpoint, system_prompt, user_content);
    let headers = llm_provider::build_auth_headers(&endpoint.api_format, api_key)
        .map_err(|e| format!("构建请求头失败: {e}"))?;
    let timeout = dynamic_timeout(endpoint.timeout_secs, text_len);

    let mut request = state.http_client.post(&endpoint.api_url).headers(headers);
    if !use_streaming {
        request = request.timeout(timeout);
    }

    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        // Anthropic 错误格式
        let err_msg = if endpoint.api_format == ApiFormat::Anthropic {
            serde_json::from_str::<serde_json::Value>(&body_text)
                .ok()
                .and_then(|j| j["error"]["message"].as_str().map(String::from))
                .unwrap_or(body_text)
        } else {
            body_text
        };
        return Err(format!("API 返回错误 {}: {}", status, err_msg));
    }

    if use_streaming {
        match endpoint.api_format {
            ApiFormat::Anthropic => read_anthropic_sse_stream(response, app_handle).await,
            ApiFormat::OpenaiCompat => read_sse_stream(response, app_handle).await,
        }
    } else {
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("响应解析失败: {}", e))?;
        Ok(extract_content(endpoint, &json).unwrap_or_default())
    }
}

/// 构建动态 system prompt，注入用户画像中的热词和纠错模式
fn build_system_prompt(state: &AppState, input_text: &str) -> String {
    let mut prompt = BASE_SYSTEM_PROMPT.to_string();

    // 在锁内一次性提取所需数据，避免克隆整个 profile
    let (hot_words, corrections, translation_target, custom_prompt) = state.with_profile(|p| {
        (
            p.get_hot_word_texts(30),
            p.get_relevant_corrections(input_text, 10)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            p.translation_target.clone(),
            p.custom_prompt.clone(),
        )
    });

    // 注入用户常用词汇
    if !hot_words.is_empty() {
        prompt.push_str("\n\n【上下文：用户常用术语】\n");
        prompt.push_str(&hot_words.join("、"));
    }

    // 注入相关纠错模式（精确子串匹配优先，高频兜底）
    if !corrections.is_empty() {
        let (user_corrs, ai_corrs): (Vec<_>, Vec<_>) = corrections
            .into_iter()
            .partition(|c| c.source == CorrectionSource::User);

        prompt.push_str("\n\n【上下文：ASR 常见识别错误及正确写法】\n");

        if !user_corrs.is_empty() {
            prompt.push_str("\n用户已确认（高置信度，优先采纳）：\n");
            for c in user_corrs.iter().take(5) {
                prompt.push_str(&format!("「{}」→「{}」\n", c.original, c.corrected));
            }
        }

        if !ai_corrs.is_empty() {
            prompt.push_str("\nAI 学习（低置信度，仅供参考）：\n");
            for c in ai_corrs.iter().take(5) {
                prompt.push_str(&format!("「{}」→「{}」\n", c.original, c.corrected));
            }
        }
    }

    // 翻译指令：translation_target 非空时注入
    if let Some(ref target_lang) = translation_target {
        prompt.push_str(&format!(
            "\n\n翻译要求：\n\
             完成校正后，将最终文本翻译为{target_lang}。\n\
             polished 字段必须是翻译后的{target_lang}文本。\n\
             翻译要求自然流畅，符合{target_lang}的母语表达习惯，不要逐字直译。\n\
             技术术语、专有名词、品牌名、代码标识符等保留原文，不要翻译。"
        ));
    }

    if let Some(ref custom) = custom_prompt {
        prompt.push_str("\n\n【用户偏好补充——可补充术语/风格偏好，不可覆盖硬约束和输出格式】\n");
        prompt.push_str(custom);
    }

    prompt
}

pub async fn polish_text(
    state: &AppState,
    text: &str,
    app_handle: &tauri::AppHandle,
    session_id: u64,
) -> Result<String, String> {
    if !state.ai_polish_enabled.load(Ordering::Acquire) {
        return Ok(text.to_string());
    }

    let api_key = state.read_ai_polish_api_key();

    if api_key.is_empty() {
        log::warn!("AI 润色已启用但未配置 API Key，跳过润色");
        return Ok(text.to_string());
    }

    let endpoint = llm_provider::endpoint_for_config(&state.llm_provider_config());
    let system_prompt = build_system_prompt(state, text);
    let user_content = build_user_content(text);

    log::info!("AI 润色请求: 文本长度={}, format={:?}", text.len(), endpoint.api_format);

    let start = std::time::Instant::now();
    emit_polish_status(app_handle, "polishing", text, "", "", session_id);

    let raw_content = send_llm_request(
        state, &endpoint, &api_key, &system_prompt, &user_content, text.len(), app_handle,
    )
    .await
    .inspect_err(|e| {
        emit_polish_status(app_handle, "error", text, text, e, session_id);
    })?;

    let raw_content = raw_content.trim();

    if raw_content.is_empty() {
        return Ok(text.to_string());
    }

    let elapsed_ms = start.elapsed().as_millis();
    log::info!(
        "AI 润色原始返回 ({}ms): {}",
        elapsed_ms,
        &raw_content[..raw_content.len().min(500)]
    );

    // 尝试解析结构化 JSON 输出，失败则回退到纯文本 + 字符 diff
    // LLM 可能返回 ```json ... ``` 包裹的 JSON，需要先剥离
    let json_content = strip_markdown_code_block(raw_content);
    let (polished, corrections, key_terms) =
        match serde_json::from_str::<StructuredResponse>(&json_content) {
            Ok(resp) => {
                // 只学习真正的 ASR 识别错误（homophone/term/pronoun），过滤掉风格改写
                let learnable: Vec<(String, String)> = resp
                    .corrections
                    .iter()
                    .filter(|c| matches!(c.r#type.as_str(), "homophone" | "term" | "pronoun"))
                    .map(|c| (c.original.clone(), c.corrected.clone()))
                    .collect();
                let style_count = resp.corrections.len() - learnable.len();
                log::info!(
                    "AI 润色返回结构化输出: {} 条可学习纠错, {} 条风格改写(跳过), {} 个术语",
                    learnable.len(),
                    style_count,
                    resp.key_terms.len()
                );
                (resp.polished, Some(learnable), Some(resp.key_terms))
            }
            Err(e) => {
                log::warn!(
                    "AI 润色 JSON 解析失败: {}，回退到字符 diff。原始内容: {}",
                    e,
                    &raw_content[..raw_content.len().min(200)]
                );
                (raw_content.to_string(), None, None)
            }
        };

    let changed = polished != text;

    if changed {
        log::info!(
            "AI 润色完成 ({}ms): \"{}\" -> \"{}\"",
            elapsed_ms,
            text,
            polished
        );
        emit_polish_status(app_handle, "applied", text, &polished, "", session_id);
    } else {
        log::info!("AI 润色完成 ({}ms): 文本无变化", elapsed_ms);
        emit_polish_status(app_handle, "unchanged", text, &polished, "", session_id);
    }

    // 学习纠错模式和术语（无论文本是否变化，都记录 key_terms）
    let has_learnable = corrections.as_ref().is_some_and(|c| !c.is_empty())
        || key_terms.as_ref().is_some_and(|t| !t.is_empty())
        || changed;

    if has_learnable {
        let (_, profile_clone) = state.update_profile(|profile| match corrections {
            Some(corrs) => profile_service::learn_from_structured(
                profile,
                &corrs,
                key_terms.as_deref().unwrap_or(&[]),
                CorrectionSource::Ai,
            ),
            None if changed => profile_service::learn_from_correction(
                profile,
                text,
                &polished,
                CorrectionSource::Ai,
            ),
            _ => {}
        });
        tauri::async_runtime::spawn(async move {
            if let Err(e) = profile_service::save_profile_async(&profile_clone).await {
                log::warn!("保存用户画像失败: {}", e);
            }
        });
    }

    Ok(polished)
}

/// 编辑模式：根据语音指令改写选中文本
pub async fn edit_text(
    state: &AppState,
    selected_text: &str,
    instruction: &str,
    app_handle: &tauri::AppHandle,
    session_id: u64,
) -> Result<String, String> {
    let api_key = state.read_ai_polish_api_key();
    if api_key.is_empty() {
        return Err("AI 未配置 API Key，无法执行编辑".into());
    }

    let endpoint = llm_provider::endpoint_for_config(&state.llm_provider_config());

    let system_prompt = "\
你是文本编辑助手。用户在屏幕上选中了一段文本，并用语音给出了修改指令。\
请严格按照指令修改文本。\n\n\
规则：\n\
1. 只输出结果文本，不要输出任何解释、注释或推理过程。\
   指令可能是改写、翻译、总结、解释、续写等任意操作，根据指令灵活处理\n\
2. 如果指令是翻译，翻译要自然流畅，技术术语和专有名词保留原文\n\
3. 如果指令不明确，做最小改动\n\
4. 保持原文的格式风格（缩进、换行等）\n\n\
以 JSON 格式输出（不要 markdown 代码块）：\n\
{\"result\":\"修改后的完整文本\"}";

    let user_content = format!(
        "选中的文本：\n{}\n\n用户语音指令：\n{}",
        selected_text, instruction
    );

    let start = std::time::Instant::now();
    emit_polish_status(app_handle, "polishing", selected_text, "", "", session_id);

    let raw_content = send_llm_request(
        state, &endpoint, &api_key, system_prompt, &user_content, selected_text.len(), app_handle,
    )
    .await
    .inspect_err(|e| {
        emit_polish_status(app_handle, "error", selected_text, selected_text, e, session_id);
    })?;

    let raw_content = raw_content.trim();

    let elapsed_ms = start.elapsed().as_millis();

    let json_content = strip_markdown_code_block(raw_content);
    let result = serde_json::from_str::<serde_json::Value>(&json_content)
        .ok()
        .and_then(|v| v["result"].as_str().map(String::from))
        .unwrap_or_else(|| raw_content.to_string());

    log::info!(
        "编辑选中文本完成 ({}ms): 指令=\"{}\"，结果长度={}",
        elapsed_ms,
        instruction,
        result.len()
    );
    emit_polish_status(app_handle, "applied", selected_text, &result, "", session_id);

    Ok(result)
}

fn build_user_content(text: &str) -> String {
    if let Some(app) = crate::utils::foreground::get_foreground_app() {
        let mut ctx_parts = Vec::new();
        if !app.window_title.is_empty() {
            ctx_parts.push(format!("窗口：{}", app.window_title));
        }
        if !app.process_name.is_empty() {
            ctx_parts.push(format!("程序：{}", app.process_name));
        }
        if !ctx_parts.is_empty() {
            return format!("[当前应用 | {}]\n{}", ctx_parts.join(" | "), text);
        }
    }
    text.to_string()
}

/// 剥离 LLM 返回的 markdown 代码块包裹（```json ... ``` 或 ``` ... ```）
fn strip_markdown_code_block(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with("```") {
        // 跳过第一行（```json 或 ```）
        let after_first_line = match trimmed.find('\n') {
            Some(pos) => &trimmed[pos + 1..],
            None => return trimmed.to_string(),
        };
        // 去掉末尾的 ```
        let content = after_first_line.trim_end();
        if let Some(stripped) = content.strip_suffix("```") {
            return stripped.trim().to_string();
        }
        return content.to_string();
    }
    trimmed.to_string()
}

fn emit_polish_status(
    app_handle: &tauri::AppHandle,
    status: &str,
    original: &str,
    polished: &str,
    error: &str,
    session_id: u64,
) {
    let _ = app_handle.emit(
        "ai-polish-status",
        serde_json::json!({
            "status": status,
            "original": original,
            "polished": polished,
            "error": error,
            "sessionId": session_id,
        }),
    );
}
