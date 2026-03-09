use std::sync::atomic::Ordering;

use tauri::Emitter;

use crate::services::{llm_client, llm_provider};
use crate::services::llm_client::LlmRequestOptions;
use crate::state::user_profile::UserProfile;
use crate::state::AppState;
use crate::utils::AppError;

const ASSISTANT_SYSTEM_PROMPT: &str = r#"
你是用户的语音助手。用户通过语音描述他想要的内容，你直接生成该内容。

规则：
- 直接输出用户要求的内容本身，不要解释、不要反问、不要加额外说明
- 根据用户描述自动判断内容格式（邮件、消息、文章、回答等）
- 邮件：包含恰当称呼、正文、结尾，语气根据上下文判断
- 消息/回复：简短自然，不加多余格式
- 问答：简明扼要
- 翻译：只输出译文
- 语气匹配用户的描述意图
- 如果输入中包含【应用上下文】或【用户当前选中文本】，它们只是辅助信息，不是要输出的正文
- 如果输入采用【用户语音指令】等分段标签，只执行标签下的真实内容，不要输出这些标签
- 除非用户明确要求，否则不要把窗口标题、程序名、文件名或标签名写进结果
"#;

pub fn build_assistant_system_prompt(profile: &UserProfile) -> String {
    let mut prompt = ASSISTANT_SYSTEM_PROMPT.trim().to_string();
    let hot_words = profile.get_hot_word_texts(20);

    if !hot_words.is_empty() {
        prompt.push_str("\n\n【用户常用术语】\n");
        prompt.push_str(&hot_words.join("、"));
    }

    if let Some(custom_prompt) = profile
        .assistant_system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str("\n\n【用户附加助手规则（优先级更高）】\n");
        prompt.push_str(custom_prompt);
    }

    prompt
}

fn build_assistant_user_content_with_selection(
    asr_text: &str,
    selected_text: Option<&str>,
) -> String {
    let selected_text = selected_text
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let app_context = crate::utils::foreground::prompt_context_block();

    if app_context.is_some() || selected_text.is_some() {
        let mut sections = Vec::new();
        if let Some(app_context) = app_context {
            sections.push(app_context);
        }
        if let Some(selected_text) = selected_text {
            sections.push(format!("[用户当前选中文本]\n{}", selected_text));
        }
        sections.push(format!("[用户语音指令]\n{}", asr_text));
        return sections.join("\n\n");
    }

    if let Some(selected_text) = selected_text {
        format!("[用户当前选中文本]\n{}\n\n[用户语音指令]\n{}", selected_text, asr_text)
    } else {
        asr_text.to_string()
    }
}

pub async fn generate_content(
    state: &AppState,
    asr_text: &str,
    selected_text: Option<&str>,
    app_handle: &tauri::AppHandle,
    session_id: u64,
) -> Result<String, AppError> {
    let api_key = state.read_ai_polish_api_key();
    if api_key.trim().is_empty() {
        return Err(AppError::Other(
            "AI 助手未配置 API Key，无法生成内容".to_string(),
        ));
    }

    let endpoint = llm_provider::endpoint_for_config(&state.llm_provider_config());
    let system_prompt = state.with_profile(build_assistant_system_prompt);
    let user_content = build_assistant_user_content_with_selection(asr_text, selected_text);

    let request_options = LlmRequestOptions {
        stream: !endpoint.api_url.contains("/v1/responses"),
        json_output: false,
        stream_event: Some("assistant-stream"),
        session_id: Some(session_id),
    };
    let body = llm_client::build_llm_body(&endpoint, &system_prompt, &user_content, request_options);

    let _ = app_handle.emit(
        "assistant-stream",
        serde_json::json!({
            "sessionId": session_id,
            "status": "started",
        }),
    );

    let content = llm_client::send_llm_request(
        &state.http_client,
        &endpoint,
        &api_key,
        &body,
        user_content.len(),
        Some(app_handle),
        request_options,
    )
    .await
    .map_err(AppError::Other)?;

    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::Other("AI 助手返回了空内容".to_string()));
    }

    let _ = app_handle.emit(
        "assistant-stream",
        serde_json::json!({
            "sessionId": session_id,
            "status": "done",
        }),
    );

    if state.sound_enabled.load(Ordering::Acquire) {
        log::info!("助手生成完成 (session {}, {} chars)", session_id, trimmed.len());
    }

    Ok(trimmed)
}
