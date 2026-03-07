use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::Emitter;

use serde::Deserialize;

use crate::services::llm_provider;
use crate::services::profile_service;
use crate::state::user_profile::CorrectionSource;
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
你是资深的语音识别文本校对专家。以下文本来自ASR语音识别，请根据语境推断用户的真实意图，校正文本。

校正规则：
1. 同音字/近音字：语音识别最常见的错误。根据上下文语义判断正确用字（例：「事业」vs「视野」、「期待」vs「奇袋」）
2. 人称代词：根据语境修正混淆的他/她/它。注意：不要把「你」改成「您」，除非用户原话就说的「您」——敬语升格不属于 ASR 纠错
3. 标点断句：修正断句不合理、语气符号缺失的问题
4. 口语化表达：去除口头禅和重复词（嗯、啊、然后然后、就是就是），但保留说话者的原始语气、风格和用词习惯，不要书面化改写。\
   当用户中途改口（如「不对」「我的意思是」「算了换个说法」「sorry I mean」「actually」），只保留最终意图，丢弃被否定的部分
5. 数字格式：根据语境使用合适的数字格式
6. 结构化格式：当用户明显在列举（「第一、第二」或「首先、其次」），使用编号列表格式化（如 1. xxx\n2. xxx）；\
   当用户口述分段内容时，用空行分隔段落
7. 语气适配：如果输入带有 [当前应用] 上下文，根据应用场景自适应语气和格式：\
   聊天软件（微信/钉钉/Telegram）→ 紧凑口语化，避免多余换行；\
   邮件客户端 → 正式书面语，段落分明；\
   文档/笔记 → 完整结构化格式，列表和段落都保留；\
   代码编辑器/终端 → 保留技术术语和原始格式
8. 口述符号转换：当用户明确口述符号名称时，转换为对应符号。常见映射：\
   大于→>、小于→<、等于→=、不等于→≠、大于等于→>=、小于等于→<=\
   加/加上→+、减/减去→-、乘/乘以→×、除/除以→/\
   左括号→(、右括号→)、左方括号→[、右方括号→]、左花括号→{、右花括号→}\
   百分号→%、井号→#、at→@、下划线→_、斜杠→/、反斜杠→\\、竖线→|、波浪号→~\
   句号→。、逗号→，、问号→？、感叹号→！、冒号→：、分号→；、省略号→……\
   换行/新行→实际换行\
   注意：只在用户明确口述符号名称时转换，不要把正常用作动词的词（如「大于」表示比较）也转换

禁止事项：
- 禁止添加原文没有的信息
- 禁止改变原文的意思和意图
- 禁止输出任何解释、注释或推理过程

以 JSON 格式输出（不要 markdown 代码块）：
{\"polished\":\"校正后的完整文本\",\"corrections\":[{\"original\":\"ASR原文片段\",\"corrected\":\"纠正后片段\",\"type\":\"类型\"}],\"key_terms\":[\"重要专有名词或术语\"]}

corrections 的 type 取值：
- homophone: 同音字/近音字错误（如「视野」→「事业」）—— ASR 听错了字
- term: 专有名词/术语识别错误（如「赛博」→「Cerebras」）—— ASR 不认识这个词
- pronoun: 人称代词错误（如「他」→「它」）
- style: 口语化改写、去口头禅、标点修正等风格调整

重要：corrections 只记录词/短语级别的替换（2-8字），不要把整句改写拆成 correction。
key_terms 只列出值得记录的专有名词（2字以上的名词、术语、人名、品牌等）。
如文本无需修改，polished 与输入相同，corrections 和 key_terms 为空数组。";

/// 根据文本长度动态计算超时时间
fn dynamic_timeout(base_secs: u64, text_len: usize) -> Duration {
    // 每 200 字符额外 1 秒，封顶 120 秒
    let extra = (text_len / 200) as u64;
    Duration::from_secs(base_secs.saturating_add(extra).min(120))
}

/// 从 SSE 流中累积 Chat Completions delta 内容，并向前端推送流式进度
async fn read_sse_stream(
    mut response: reqwest::Response,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    let mut accumulated = String::new();
    let mut buffer = String::new();
    let chunk_timeout = Duration::from_secs(30);
    let mut token_count: usize = 0;

    loop {
        match tokio::time::timeout(chunk_timeout, response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim_end_matches('\r').to_string();
                    buffer = buffer[pos + 1..].to_string();

                    let Some(data) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    let data = data.trim();
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
            }
            Ok(Ok(None)) => return Ok(accumulated),
            Ok(Err(e)) => return Err(format!("流式读取失败: {}", e)),
            Err(_) => return Err("流式读取超时（30 秒无数据）".into()),
        }
    }
}

/// 构建动态 system prompt，注入用户画像中的热词和纠错模式
fn build_system_prompt(state: &AppState, input_text: &str) -> String {
    let mut prompt = BASE_SYSTEM_PROMPT.to_string();

    let profile = state.snapshot_profile();

    // 注入用户常用词汇（Top 50）
    let hot_words = profile.get_hot_word_texts(50);
    if !hot_words.is_empty() {
        prompt.push_str("\n\n用户常用专有名词（优先使用这些词汇）：\n");
        prompt.push_str(&hot_words.join("、"));
    }

    // 注入相关纠错模式（精确子串匹配优先，高频兜底）
    let corrections = profile.get_relevant_corrections(input_text, 10);
    if !corrections.is_empty() {
        let (user_corrs, ai_corrs): (Vec<_>, Vec<_>) = corrections
            .into_iter()
            .partition(|c| c.source == CorrectionSource::User);

        prompt.push_str("\n\n以下是 ASR 常见识别错误及其正确写法，请在校正时优先参考：\n");

        if !user_corrs.is_empty() {
            prompt.push_str("\n[用户已确认的纠错]\n");
            for c in user_corrs.iter().take(5) {
                prompt.push_str(&format!(
                    "输入：……{}……\n输出：……{}……\n\n",
                    c.original, c.corrected
                ));
            }
        }

        if !ai_corrs.is_empty() {
            prompt.push_str("[AI 学习的纠错模式]\n");
            for c in ai_corrs.iter().take(5) {
                prompt.push_str(&format!(
                    "错误「{}」→ 正确「{}」\n",
                    c.original, c.corrected
                ));
            }
        }
    }

    // 翻译指令：translation_target 非空时注入
    if let Some(ref target_lang) = profile.translation_target {
        prompt.push_str(&format!(
            "\n\n翻译要求：\n\
             完成校正后，将最终文本翻译为{target_lang}。\n\
             polished 字段必须是翻译后的{target_lang}文本。\n\
             翻译要求自然流畅，符合{target_lang}的母语表达习惯，不要逐字直译。\n\
             技术术语、专有名词、品牌名、代码标识符等保留原文，不要翻译。"
        ));
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

    // 获取 LLM 端点配置
    let endpoint = llm_provider::endpoint_for_config(&state.llm_provider_config());

    // 构建动态 prompt
    let system_prompt = build_system_prompt(state, text);

    // 注入前台应用上下文到 user message
    let user_content = build_user_content(text);

    let is_responses_api = endpoint.api_url.contains("/v1/responses");

    let mut body = if is_responses_api {
        // OpenAI Responses API 格式，强制 JSON 输出
        // input 必须包含 "json" 一词才能启用 json_object 格式，
        // 用 developer message 注入以避免污染用户文本
        serde_json::json!({
            "model": endpoint.model,
            "instructions": system_prompt,
            "input": [
                {"role": "developer", "content": "Output json."},
                {"role": "user", "content": user_content},
            ],
            "text": {
                "format": {
                    "type": "json_object"
                }
            }
        })
    } else {
        // OpenAI Chat Completions 兼容格式
        let messages = vec![
            serde_json::json!({ "role": "system", "content": system_prompt }),
            serde_json::json!({ "role": "user", "content": user_content }),
        ];
        serde_json::json!({
            "model": endpoint.model,
            "messages": messages,
            "response_format": { "type": "json_object" }
        })
    };

    // 文本校对不需要深度推理，降低 reasoning effort 节省时间和 token
    if is_responses_api {
        body["reasoning"] = serde_json::json!({"effort": "medium"});
    } else if endpoint.api_url.contains("cerebras") {
        body["reasoning_effort"] = serde_json::json!("low");
    }

    // Chat Completions API 使用流式输出，避免长文本超时
    let use_streaming = !is_responses_api;
    if use_streaming {
        body["stream"] = serde_json::json!(true);
    }

    // 流式请求不设置总超时——由 read_sse_stream 的 chunk_timeout 保护；
    // 非流式请求使用动态总超时。
    let timeout = dynamic_timeout(endpoint.timeout_secs, text.len());
    log::info!(
        "AI 润色请求: 文本长度={}, 超时={}s, 流式={}",
        text.len(),
        timeout.as_secs(),
        use_streaming
    );

    let start = std::time::Instant::now();
    emit_polish_status(app_handle, "polishing", text, "", "", session_id);

    let mut request = state
        .http_client
        .post(&endpoint.api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");
    if !use_streaming {
        request = request.timeout(timeout);
    }
    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("AI 润色请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let err = format!("AI 润色 API 返回错误 {}: {}", status, body_text);
        emit_polish_status(app_handle, "error", text, text, &err, session_id);
        return Err(err);
    }

    let raw_content = if use_streaming {
        let content = read_sse_stream(response, app_handle).await.map_err(|e| {
            emit_polish_status(app_handle, "error", text, text, &e, session_id);
            e
        })?;
        content
    } else {
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("AI 润色响应解析失败: {}", e))?;

        log::debug!(
            "AI 润色 API 响应: {}",
            serde_json::to_string(&json).unwrap_or_default()
        );

        // Responses API 的 output 可能包含 reasoning 块，需要找到 type=="message" 的项
        json["output"]
            .as_array()
            .and_then(|outputs| {
                outputs.iter().find_map(|item| {
                    if item["type"].as_str() == Some("message") {
                        item["content"][0]["text"].as_str().map(String::from)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| text.to_string())
    };

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

    let is_responses_api = endpoint.api_url.contains("/v1/responses");

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
    }

    let use_streaming = !is_responses_api;
    if use_streaming {
        body["stream"] = serde_json::json!(true);
    }

    let timeout = dynamic_timeout(endpoint.timeout_secs, selected_text.len());

    let start = std::time::Instant::now();
    emit_polish_status(app_handle, "polishing", selected_text, "", "", session_id);

    let mut request = state
        .http_client
        .post(&endpoint.api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");
    if !use_streaming {
        request = request.timeout(timeout);
    }
    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("编辑请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let err = format!("编辑 API 返回错误 {}: {}", status, body_text);
        emit_polish_status(app_handle, "error", selected_text, selected_text, &err, session_id);
        return Err(err);
    }

    let raw_content = if use_streaming {
        read_sse_stream(response, app_handle).await.map_err(|e| {
            emit_polish_status(app_handle, "error", selected_text, selected_text, &e, session_id);
            e
        })?
    } else {
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("编辑响应解析失败: {}", e))?;

        json["output"]
            .as_array()
            .and_then(|outputs| {
                outputs.iter().find_map(|item| {
                    if item["type"].as_str() == Some("message") {
                        item["content"][0]["text"].as_str().map(String::from)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| selected_text.to_string())
    };

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
