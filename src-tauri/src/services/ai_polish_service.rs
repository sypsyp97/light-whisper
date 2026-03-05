use std::sync::atomic::Ordering;

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
2. 人称代词：根据语境修正混淆的你/您、他/她/它
3. 标点断句：修正断句不合理、语气符号缺失的问题
4. 口语化表达：轻度书面化，去除口头禅和重复词（嗯、啊、然后然后、就是就是），但保留说话者的原始语气和风格
5. 数字格式：根据语境使用合适的数字格式
6. 结构化格式：当用户明显在列举（「第一、第二」或「首先、其次」），用换行或编号格式化
7. 语气适配：如果输入带有 [当前应用] 上下文，根据应用场景自适应语气（如聊天软件保持口语化，邮件客户端用正式书面语，代码编辑器保留技术术语等）
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

/// 构建动态 system prompt，注入用户画像中的热词和纠错模式
fn build_system_prompt(state: &AppState, input_text: &str) -> String {
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

    log::debug!(
        "AI 润色 API 响应: {}",
        serde_json::to_string(&json).unwrap_or_default()
    );

    let raw_content = if is_responses_api {
        // Responses API 的 output 可能包含 reasoning 块，需要找到 type=="message" 的项
        json["output"]
            .as_array()
            .and_then(|outputs| {
                outputs.iter().find_map(|item| {
                    if item["type"].as_str() == Some("message") {
                        item["content"][0]["text"].as_str()
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(text)
    } else {
        json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or(text)
    }
    .trim();

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
        emit_polish_status(app_handle, "applied", text, &polished, "");
    } else {
        log::info!("AI 润色完成 ({}ms): 文本无变化", elapsed_ms);
        emit_polish_status(app_handle, "unchanged", text, &polished, "");
    }

    // 学习纠错模式和术语（无论文本是否变化，都记录 key_terms）
    let has_learnable = corrections.as_ref().is_some_and(|c| !c.is_empty())
        || key_terms.as_ref().is_some_and(|t| !t.is_empty())
        || changed;

    if has_learnable {
        if let Ok(mut profile) = state.user_profile.lock() {
            match corrections {
                Some(corrs) => profile_service::learn_from_structured(
                    &mut profile,
                    &corrs,
                    key_terms.as_deref().unwrap_or(&[]),
                    CorrectionSource::Ai,
                ),
                None if changed => profile_service::learn_from_correction(
                    &mut profile,
                    text,
                    &polished,
                    CorrectionSource::Ai,
                ),
                _ => {}
            }
            let profile_clone = profile.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = profile_service::save_profile_async(&profile_clone).await {
                    log::warn!("保存用户画像失败: {}", e);
                }
            });
        }
    }

    Ok(polished)
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
