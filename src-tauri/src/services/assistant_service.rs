use std::sync::atomic::Ordering;

use tauri::Emitter;

use crate::services::llm_client::{LlmImageInput, LlmRequestOptions, LlmUserInput};
use crate::services::{llm_client, llm_provider, screen_capture_service};
use crate::state::user_profile::UserProfile;
use crate::state::AppState;
use crate::utils::AppError;

const ASSISTANT_SYSTEM_PROMPT: &str = r#"
<role>
你是用户的语音助手，负责把用户口述的意图直接变成可直接使用的最终文本。
</role>

<instructions>
1. 只输出最终内容本身，不要解释、不要反问、不要加标题、不要加前后说明。
2. 只根据 <user_request> 生成与其真实意图对应的内容。
3. 把 <app_context>、<selected_text> 和屏幕截图都只当作辅助上下文；除非用户明确要求，否则不要复述其中的信息。
4. 如果存在 <selected_text> 且用户请求是改写、续写、总结、翻译、解释、润色、提炼、扩写、压缩或调整语气，默认只处理 <selected_text>。
5. 如果不存在 <selected_text>，就仅根据 <user_request> 生成目标内容，不要假设还有隐藏上下文。
6. 根据用户意图自动匹配格式：
   - 邮件：包含合适称呼、正文、结尾。
   - 消息/回复：简短自然，不加多余格式。
   - 问答：直接回答，简明扼要。
   - 翻译：只输出译文。
   - 清单/标题/摘要：按用户要求输出对应形式。
7. 语气、详略、语言和格式优先匹配用户请求，而不是匹配你的默认风格。
8. 除非用户明确要求，否则不要把窗口标题、程序名、文件名、标签名或示例内容写进结果。
</instructions>

<edge_cases>
- 如果用户请求本身就是一段要直接使用的内容，直接输出它的最终版本。
- 如果用户请求引用了已选中文本，但 <selected_text> 为空，就仅根据 <user_request> 做最小安全推断，不要编造额外事实。
- 如果用户请求是翻译，只输出译文，不附加说明。
- 如果用户请求是“回复一句”“写一句”“帮我发一句”，默认输出一段可直接使用的自然文本。
</edge_cases>

<examples>
  <example>
    <input>
      <user_request><![CDATA[帮我回一句：我今天晚点到，大概七点半。]]></user_request>
    </input>
    <output><![CDATA[我今天会晚点到，大概七点半。]]></output>
  </example>
  <example>
    <input>
      <selected_text><![CDATA[这个方案不太行，你再想想。]]></selected_text>
      <user_request><![CDATA[改得更礼貌一些]]></user_request>
    </input>
    <output><![CDATA[这个方案目前还不够理想，麻烦你再想想。]]></output>
  </example>
  <example>
    <input>
      <user_request><![CDATA[把“我们明天下午两点开会”翻成英文]]></user_request>
    </input>
    <output><![CDATA[We have a meeting tomorrow at 2 PM.]]></output>
  </example>
  <example>
    <input>
      <app_context>
        <process_name><![CDATA[Code.exe]]></process_name>
        <window_title><![CDATA[RELEASE_GUIDE.md]]></window_title>
      </app_context>
      <user_request><![CDATA[写一句提交版本说明的备注：补充发版步骤和注意事项]]></user_request>
    </input>
    <output><![CDATA[补充了发版步骤和注意事项。]]></output>
  </example>
</examples>
"#;

pub fn build_assistant_system_prompt(profile: &UserProfile) -> String {
    let mut prompt = ASSISTANT_SYSTEM_PROMPT.trim().to_string();
    let hot_words = profile.get_hot_word_texts(20);

    if !hot_words.is_empty() {
        prompt.push_str("\n\n<user_terms>\n");
        for hot_word in hot_words {
            prompt.push_str(&crate::utils::foreground::wrap_xml_cdata("term", &hot_word));
            prompt.push('\n');
        }
        prompt.push_str("</user_terms>");
    }

    if let Some(custom_prompt) = profile
        .assistant_system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str("\n\n<user_overrides priority=\"high\">\n");
        prompt.push_str(&crate::utils::foreground::wrap_xml_cdata(
            "override",
            custom_prompt,
        ));
        prompt.push_str("\n</user_overrides>");
    }

    prompt
}

fn build_assistant_user_content_with_selection(
    asr_text: &str,
    selected_text: Option<&str>,
    has_screen_context: bool,
) -> String {
    let app_context = crate::utils::foreground::prompt_context_block();
    render_assistant_user_content(
        app_context.as_deref(),
        asr_text,
        selected_text,
        has_screen_context,
    )
}

fn render_assistant_user_content(
    app_context: Option<&str>,
    asr_text: &str,
    selected_text: Option<&str>,
    has_screen_context: bool,
) -> String {
    let selected_text = selected_text
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if app_context.is_some() || selected_text.is_some() || has_screen_context {
        let mut sections = Vec::new();
        if let Some(app_context) = app_context {
            sections.push(app_context.to_string());
        }
        if let Some(selected_text) = selected_text {
            sections.push(crate::utils::foreground::wrap_xml_cdata(
                "selected_text",
                selected_text,
            ));
        }
        if has_screen_context {
            sections.push(crate::utils::foreground::wrap_xml_cdata(
                "screen_context",
                "已附带当前整屏截图，仅在与用户请求相关时参考其中信息。",
            ));
        }
        sections.push(crate::utils::foreground::wrap_xml_cdata(
            "user_request",
            asr_text,
        ));
        return sections.join("\n\n");
    }

    if let Some(selected_text) = selected_text {
        format!(
            "{}\n\n{}",
            crate::utils::foreground::wrap_xml_cdata("selected_text", selected_text),
            crate::utils::foreground::wrap_xml_cdata("user_request", asr_text)
        )
    } else {
        crate::utils::foreground::wrap_xml_cdata("user_request", asr_text)
    }
}

#[cfg(test)]
mod tests {
    use super::render_assistant_user_content;

    #[test]
    fn assistant_input_preserves_symbols_and_splits_cdata() {
        let content = render_assistant_user_content(
            Some("<app_context><process_name><![CDATA[Code.exe]]></process_name></app_context>"),
            "如果 a > b 并且文本里有 ]]> 这个片段",
            Some("原文里有 <tag> 和 >"),
            true,
        );

        assert!(content.contains(
            "<app_context><process_name><![CDATA[Code.exe]]></process_name></app_context>"
        ));
        assert!(content.contains("<selected_text><![CDATA[原文里有 <tag> 和 >]]></selected_text>"));
        assert!(content.contains("<screen_context><![CDATA[已附带当前整屏截图，仅在与用户请求相关时参考其中信息。]]></screen_context>"));
        assert!(content.contains(
            "<user_request><![CDATA[如果 a > b 并且文本里有 ]]]]><![CDATA[> 这个片段]]></user_request>"
        ));
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

    let config = state.llm_provider_config();
    let endpoint = llm_provider::assistant_endpoint_for_config(&config);
    let system_prompt = state.with_profile(build_assistant_system_prompt);
    let screen_context_enabled =
        state.with_profile(|profile| profile.assistant_screen_context_enabled);
    let image_support_cache_key = llm_provider::image_support_cache_key(&endpoint);
    let cached_image_support = state.assistant_image_support(&image_support_cache_key);
    let probed_image_support = if screen_context_enabled && cached_image_support.is_none() {
        let support = llm_provider::probe_image_support_from_provider_metadata(&state.http_client, &endpoint, &api_key).await;
        if let Some(supported) = support {
            state.set_assistant_image_support(image_support_cache_key.clone(), supported);
            log::info!(
                "根据模型元数据识别图片输入支持: provider={}, model={}, supported={}",
                endpoint.provider,
                endpoint.model,
                supported
            );
        }
        support
    } else {
        None
    };
    let effective_image_support = cached_image_support.or(probed_image_support);

    let images = if screen_context_enabled && effective_image_support != Some(false) {
        match screen_capture_service::capture_full_screen_context() {
            Ok(captured) => {
                if !captured.is_empty() {
                    let labels = captured
                        .iter()
                        .map(|image| image.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    log::info!(
                        "助手模式已附带屏幕截图上下文: {} 张 ({})",
                        captured.len(),
                        labels
                    );
                }
                captured
                    .into_iter()
                    .map(|image| LlmImageInput {
                        mime_type: image.mime_type,
                        data_base64: image.data_base64,
                    })
                    .collect::<Vec<_>>()
            }
            Err(err) => {
                log::warn!("截取屏幕上下文失败，继续纯文本助手请求: {}", err);
                Vec::new()
            }
        }
    } else {
        if screen_context_enabled && effective_image_support == Some(false) {
            log::info!(
                "当前助手模型已缓存为不支持图片输入，跳过屏幕截图上下文: provider={}, model={}",
                endpoint.provider,
                endpoint.model
            );
        }
        Vec::new()
    };

    let user_content =
        build_assistant_user_content_with_selection(asr_text, selected_text, !images.is_empty());
    let user_input = LlmUserInput {
        text: user_content.clone(),
        images,
    };

    let request_options = LlmRequestOptions {
        stream: true,
        json_output: false,
        reasoning_mode: config.assistant_reasoning_mode(),
        stream_event: Some("assistant-stream"),
        session_id: Some(session_id),
    };
    let body = llm_client::build_llm_body(&endpoint, &system_prompt, &user_input, request_options);

    let _ = app_handle.emit(
        "assistant-stream",
        serde_json::json!({
            "sessionId": session_id,
            "status": "started",
        }),
    );

    let content = match llm_client::send_llm_request(
        &state.http_client,
        &endpoint,
        &api_key,
        &body,
        user_content.len(),
        Some(app_handle),
        request_options,
    )
    .await
    {
        Ok(content) => {
            if !user_input.images.is_empty() {
                state.set_assistant_image_support(image_support_cache_key.clone(), true);
            }
            content
        }
        Err(err)
            if !user_input.images.is_empty()
                && llm_provider::looks_like_image_input_unsupported_error(&err) =>
        {
            log::warn!(
                "当前模型不支持图片输入，回退到纯文本助手请求: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                err
            );
            state.set_assistant_image_support(image_support_cache_key.clone(), false);
            let fallback_input = LlmUserInput {
                text: user_content.clone(),
                images: Vec::new(),
            };
            let fallback_body = llm_client::build_llm_body(
                &endpoint,
                &system_prompt,
                &fallback_input,
                request_options,
            );
            llm_client::send_llm_request(
                &state.http_client,
                &endpoint,
                &api_key,
                &fallback_body,
                user_content.len(),
                Some(app_handle),
                request_options,
            )
            .await
            .map_err(AppError::Other)?
        }
        Err(err) => return Err(AppError::Other(err)),
    };

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
        log::info!(
            "助手生成完成 (session {}, {} chars)",
            session_id,
            trimmed.len()
        );
    }

    Ok(trimmed)
}
