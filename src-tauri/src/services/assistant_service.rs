use std::sync::atomic::Ordering;

use chrono::{DateTime, FixedOffset, Local, SecondsFormat};
use tauri::Emitter;

use crate::services::llm_client::{LlmImageInput, LlmRequestOptions, LlmUserInput};
use crate::services::{
    codex_oauth_service, llm_client, llm_provider, screen_capture_service, web_search_service,
};
use crate::state::user_profile::{UserProfile, WebSearchConfig, WebSearchProvider};
use crate::state::AppState;
use crate::utils::AppError;

const ASSISTANT_SIMPLE_STREAM_TOTAL_TIMEOUT_SECS: u64 = 240;
const ASSISTANT_EXTENDED_STREAM_TOTAL_TIMEOUT_SECS: u64 = 600;

const ASSISTANT_SYSTEM_PROMPT: &str = r#"
<role>
你是用户的语音助手，负责理解用户的语音指令并生成对应的目标文本。用户通过语音告诉你要做什么，你直接输出完成后的结果。
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
9. 你的职责是理解指令、生成内容，不是转写或润色。绝不要仅对用户原话做标点修正或最小化编辑后原样输出——那是听写模式的工作。
</instructions>

<edge_cases>
- 如果用户说了”帮我回/写/发一句”后跟具体内容，只提取并输出该内容的最终版本。
- 如果用户的语音没有明确指令动词，应根据上下文推断最可能的意图（如起草消息、撰写回复、生成文案等），产出有增值的内容，而不是原样重复。
- 如果用户请求引用了已选中文本，但 <selected_text> 为空，就仅根据 <user_request> 做最小安全推断，不要编造额外事实。
- 如果用户请求是翻译，只输出译文，不附加说明。
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
    render_assistant_system_prompt_at(profile, Local::now().fixed_offset())
}

fn render_assistant_system_prompt_at(profile: &UserProfile, now: DateTime<FixedOffset>) -> String {
    let mut prompt = ASSISTANT_SYSTEM_PROMPT.trim().to_string();
    let hot_words = profile.get_hot_word_texts(20);

    prompt.push_str("\n\n");
    prompt.push_str(&render_assistant_runtime_context(now));

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

fn render_assistant_runtime_context(now: DateTime<FixedOffset>) -> String {
    let current_datetime = now.to_rfc3339_opts(SecondsFormat::Secs, false);
    let current_date = now.format("%Y-%m-%d").to_string();
    let current_time = now.format("%H:%M:%S").to_string();
    let utc_offset = now.format("%:z").to_string();

    format!(
        "<runtime_context>\n{}\n{}\n{}\n{}\n<instruction>这是当前设备本地时间。用户提到今天、明天、昨天、现在、当前、最近、本周、本月、今年、今晚等相对时间时，以这里的时间为准。搜索查询和搜索结果判断也以该时间为准；解释日期或时间范围时，同样以该时间为准。</instruction>\n</runtime_context>",
        crate::utils::foreground::wrap_xml_cdata("current_datetime", &current_datetime),
        crate::utils::foreground::wrap_xml_cdata("current_date", &current_date),
        crate::utils::foreground::wrap_xml_cdata("current_time", &current_time),
        crate::utils::foreground::wrap_xml_cdata("utc_offset", &utc_offset),
    )
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

fn build_assistant_request_options(
    reasoning_mode: crate::state::user_profile::LlmReasoningMode,
    session_id: u64,
    use_native_search: bool,
    has_image_context: bool,
) -> LlmRequestOptions<'static> {
    let stream_total_timeout_secs = if use_native_search || has_image_context {
        ASSISTANT_EXTENDED_STREAM_TOTAL_TIMEOUT_SECS
    } else {
        ASSISTANT_SIMPLE_STREAM_TOTAL_TIMEOUT_SECS
    };

    LlmRequestOptions {
        stream: true,
        json_output: false,
        reasoning_mode,
        stream_event: Some("assistant-stream"),
        session_id: Some(session_id),
        web_search: use_native_search,
        openai_fast_mode: false,
        stream_progress_timeout_secs: None,
        stream_total_timeout_secs: Some(stream_total_timeout_secs),
    }
}

#[derive(Debug, Clone, Copy)]
struct AssistantWebSearchDecision {
    should_search: bool,
    reason: &'static str,
}

fn decide_assistant_web_search(
    asr_text: &str,
    selected_text: Option<&str>,
) -> AssistantWebSearchDecision {
    let query = asr_text.trim().to_lowercase();
    let has_selection = selected_text
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    if query.is_empty() {
        return AssistantWebSearchDecision {
            should_search: false,
            reason: "empty_request",
        };
    }

    if contains_any(
        &query,
        &[
            "不要联网",
            "不用联网",
            "别联网",
            "不要搜索",
            "不用搜索",
            "别搜索",
            "不要查",
            "不用查",
            "别查",
            "no search",
            "without searching",
            "do not search",
            "don't search",
        ],
    ) {
        return AssistantWebSearchDecision {
            should_search: false,
            reason: "explicit_no_search",
        };
    }

    if contains_any(
        &query,
        &[
            "查一下",
            "查下",
            "帮我查",
            "搜一下",
            "搜下",
            "搜索",
            "联网查",
            "上网查",
            "网上查",
            "检索",
            "look up",
            "search",
            "google",
            "browse",
        ],
    ) {
        return AssistantWebSearchDecision {
            should_search: true,
            reason: "explicit_search",
        };
    }

    if is_generation_or_editing_request(&query, has_selection) {
        return AssistantWebSearchDecision {
            should_search: false,
            reason: "generation_or_editing",
        };
    }

    if contains_any(
        &query,
        &[
            "天气",
            "温度",
            "气温",
            "预报",
            "下雨",
            "实时",
            "当前",
            "现在",
            "今天",
            "今日",
            "明天",
            "昨天",
            "最近",
            "最新",
            "新闻",
            "价格",
            "股价",
            "汇率",
            "利率",
            "航班",
            "路况",
            "比赛",
            "赛程",
            "结果",
            "weather",
            "temperature",
            "forecast",
            "current",
            "today",
            "tomorrow",
            "yesterday",
            "recent",
            "latest",
            "news",
            "price",
            "stock",
            "exchange rate",
            "flight",
            "traffic",
            "score",
            "schedule",
        ],
    ) {
        return AssistantWebSearchDecision {
            should_search: true,
            reason: "realtime_or_freshness",
        };
    }

    AssistantWebSearchDecision {
        should_search: false,
        reason: "no_search_intent",
    }
}

fn is_generation_or_editing_request(query: &str, has_selection: bool) -> bool {
    contains_any(
        query,
        &[
            "帮我写",
            "写一",
            "写封",
            "写个",
            "写段",
            "起草",
            "回复",
            "回一句",
            "帮我回",
            "发一句",
            "翻译",
            "译成",
            "润色",
            "改写",
            "改得",
            "改成",
            "总结",
            "摘要",
            "提炼",
            "扩写",
            "缩短",
            "压缩",
            "整理",
            "语气",
            "grammar",
            "translate",
            "rewrite",
            "polish",
            "summarize",
            "summary",
            "draft",
            "write",
            "reply",
            "make it",
            "shorten",
            "expand",
        ],
    ) || (has_selection
        && contains_any(
            query,
            &[
                "这段",
                "这句话",
                "这个文本",
                "selected text",
                "this text",
                "this sentence",
            ],
        ))
}

fn contains_any(value: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| value.contains(pattern))
}

/// 执行第三方搜索（Exa / Tavily）
async fn run_third_party_search(
    state: &AppState,
    ws: &WebSearchConfig,
    query: &str,
) -> Result<Vec<web_search_service::SearchResult>, String> {
    let max = ws.max_results;
    match ws.provider {
        WebSearchProvider::Exa => {
            web_search_service::exa_search(&state.http_client, query, max).await
        }
        WebSearchProvider::Tavily => {
            let api_key = state.read_web_search_api_key();
            if api_key.trim().is_empty() {
                return Err("Tavily 搜索需要配置 API Key".to_string());
            }
            web_search_service::tavily_search(&state.http_client, &api_key, query, max).await
        }
        WebSearchProvider::ModelNative => unreachable!(),
    }
}

pub async fn generate_content(
    state: &AppState,
    asr_text: &str,
    selected_text: Option<&str>,
    app_handle: &tauri::AppHandle,
    session_id: u64,
) -> Result<String, AppError> {
    let assistant_provider =
        state.with_profile(|profile| profile.llm_provider.resolve_assistant_provider());
    let assistant_manual_api_key = {
        let assistant_api_key = state.read_assistant_api_key();
        if !assistant_api_key.trim().is_empty() {
            assistant_api_key
        } else {
            let active_provider =
                state.with_profile(|profile| profile.llm_provider.resolve_active_provider());
            if assistant_provider == active_provider {
                state.read_ai_polish_api_key()
            } else {
                String::new()
            }
        }
    };
    let api_key = codex_oauth_service::resolve_api_key_for_provider(
        app_handle,
        state,
        &assistant_provider,
        &assistant_manual_api_key,
    )
    .await
    .map_err(AppError::Other)?;
    if api_key.trim().is_empty() {
        return Err(AppError::Other(
            "AI 助手未配置 API Key，且未完成 OpenAI Codex 登录，无法生成内容".to_string(),
        ));
    }

    let config = state.llm_provider_config();
    let endpoint = llm_provider::assistant_endpoint_for_config(&config);
    let ws = state.with_profile(|p| p.web_search.clone());
    let is_codex_chatgpt_bearer =
        codex_oauth_service::decode_chatgpt_bearer_token(&api_key).is_some();
    let effective_ws =
        if ws.enabled && ws.provider == WebSearchProvider::ModelNative && is_codex_chatgpt_bearer {
            log::info!(
            "OpenAI Codex OAuth bearer 模式下，助手联网搜索从模型内置搜索自动切换到 Exa: model={}",
            endpoint.model
        );
            WebSearchConfig {
                provider: WebSearchProvider::Exa,
                ..ws.clone()
            }
        } else {
            ws.clone()
        };

    // 构建 system prompt，可能追加搜索结果
    let mut system_prompt = state.with_profile(build_assistant_system_prompt);

    // ── 联网搜索 ──
    // 先用本地意图判断避免无关搜索；实时/事实/显式查询再进入搜索路径。
    // 原生模式：注入 tool，模型在需要时调用；不支持的模型会在下方 retry 时去掉 web_search。
    // 第三方模式：先搜索，结果注入 prompt，模型基于搜索结果作答。
    let web_search_decision = if effective_ws.enabled {
        decide_assistant_web_search(asr_text, selected_text)
    } else {
        AssistantWebSearchDecision {
            should_search: false,
            reason: "web_search_disabled",
        }
    };
    let use_native_search = effective_ws.enabled
        && effective_ws.provider == WebSearchProvider::ModelNative
        && web_search_decision.should_search;

    if effective_ws.enabled
        && effective_ws.provider != WebSearchProvider::ModelNative
        && web_search_decision.should_search
    {
        // 通知前端：正在搜索
        let _ = app_handle.emit(
            "assistant-stream",
            serde_json::json!({
                "sessionId": session_id,
                "status": "searching",
            }),
        );

        match run_third_party_search(state, &effective_ws, asr_text).await {
            Ok(results) => {
                log::info!(
                    "联网搜索({:?})返回 {} 条结果",
                    effective_ws.provider,
                    results.len()
                );
                system_prompt.push_str(&format!(
                    "\n\n{}",
                    web_search_service::render_search_context(&results)
                ));
            }
            Err(err) => log::warn!(
                "联网搜索({:?})失败，继续无搜索上下文: {err}",
                effective_ws.provider
            ),
        }
    } else if effective_ws.enabled && !web_search_decision.should_search {
        log::info!(
            "助手联网搜索跳过: provider={:?}, reason={}",
            effective_ws.provider,
            web_search_decision.reason
        );
    }

    let screen_context_enabled =
        state.with_profile(|profile| profile.assistant_screen_context_enabled);
    let image_support_cache_key = llm_provider::image_support_cache_key(&endpoint);
    let cached_image_support = state.assistant_image_support(&image_support_cache_key);
    let probed_image_support = if screen_context_enabled && cached_image_support.is_none() {
        let support = llm_provider::probe_image_support_from_provider_metadata(
            &state.http_client,
            &endpoint,
            &api_key,
        )
        .await;
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
        match screen_capture_service::capture_full_screen_context_async().await {
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
    let has_image_context = !user_input.images.is_empty();

    let request_options = LlmRequestOptions {
        openai_fast_mode: config.openai_fast_mode,
        ..build_assistant_request_options(
            config.assistant_reasoning_mode(),
            session_id,
            use_native_search,
            has_image_context,
        )
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
        Err(err)
            if use_native_search && llm_provider::looks_like_web_search_unsupported_error(&err) =>
        {
            log::warn!(
                "当前模型不支持联网搜索工具，去掉 web_search 重试: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                err
            );
            let fallback_options = LlmRequestOptions {
                web_search: false,
                ..request_options
            };
            let fallback_body = llm_client::build_llm_body(
                &endpoint,
                &system_prompt,
                &user_input,
                fallback_options,
            );
            llm_client::send_llm_request(
                &state.http_client,
                &endpoint,
                &api_key,
                &fallback_body,
                user_content.len(),
                Some(app_handle),
                fallback_options,
            )
            .await
            .map_err(AppError::Other)?
        }
        Err(err) => return Err(AppError::Other(err)),
    };

    // `send_llm_request` 已经在内部把空响应统一映射成 Err（带 provider/
    // model 诊断信息），上面的 match 把所有 Err 分支都 return 了，所以走到
    // 这里 content trim 后必然非空。旧的 "AI 助手返回了空内容" 兜底已
    // 不可达，移除。
    let trimmed = content.trim().to_string();

    let _ = app_handle.emit(
        "assistant-stream",
        serde_json::json!({
            "sessionId": session_id,
            "status": "done",
        }),
    );

    if state.ui.sound_enabled.load(Ordering::Acquire) {
        log::info!(
            "助手生成完成 (session {}, {} chars)",
            session_id,
            trimmed.len()
        );
    }

    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::{
        build_assistant_request_options, decide_assistant_web_search,
        render_assistant_system_prompt_at, render_assistant_user_content,
    };
    use crate::state::user_profile::LlmReasoningMode;
    use crate::state::user_profile::UserProfile;

    #[test]
    fn assistant_system_prompt_includes_current_runtime_time_context() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-15T09:08:07+02:00")
            .expect("fixed test datetime");

        let prompt = render_assistant_system_prompt_at(&UserProfile::default(), now);

        assert!(prompt.contains("<runtime_context>"));
        assert!(prompt.contains("2026-06-15T09:08:07+02:00"));
        assert!(prompt.contains("<current_date><![CDATA[2026-06-15]]></current_date>"));
        assert!(prompt.contains("<current_time><![CDATA[09:08:07]]></current_time>"));
        assert!(prompt.contains("<utc_offset><![CDATA[+02:00]]></utc_offset>"));
        assert!(prompt.contains("相对时间"));
        assert!(prompt.contains("搜索查询和搜索结果判断也以该时间为准"));
    }

    #[test]
    fn assistant_web_search_intent_requires_search_for_realtime_questions() {
        for request in [
            "纽伦堡今天的天气怎么样？",
            "查一下 OpenAI 今天有什么新闻",
            "英伟达现在股价多少？",
            "what is the latest OpenAI API pricing?",
        ] {
            let decision = decide_assistant_web_search(request, None);
            assert!(
                decision.should_search,
                "real-time query should trigger search: {request} ({})",
                decision.reason
            );
        }
    }

    #[test]
    fn assistant_web_search_intent_skips_generation_and_editing_tasks() {
        for request in [
            "现在帮我写一封邮件，说我明天下午到",
            "把这句话翻译成英文",
            "改得更礼貌一点",
            "帮我总结一下这段文字",
            "write a short reply saying I will be late",
        ] {
            let decision = decide_assistant_web_search(request, Some("需要处理的文本"));
            assert!(
                !decision.should_search,
                "generation/editing task should skip search: {request} ({})",
                decision.reason
            );
        }
    }

    #[test]
    fn assistant_web_search_intent_honors_explicit_no_search() {
        let decision = decide_assistant_web_search("不要联网，直接帮我写一段回复", None);

        assert!(!decision.should_search);
        assert_eq!(decision.reason, "explicit_no_search");
    }

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

    #[test]
    fn assistant_stream_uses_bounded_total_budget() {
        let options =
            build_assistant_request_options(LlmReasoningMode::ProviderDefault, 42, false, false);

        let timeout = options
            .stream_total_timeout_secs
            .expect("assistant streaming requests must carry a total timeout");
        assert!(
            (240..=600).contains(&timeout),
            "assistant stream total timeout should be bounded for normal tasks, got {timeout}s"
        );
    }

    #[test]
    fn native_web_search_assistant_stream_may_use_upper_budget() {
        let options =
            build_assistant_request_options(LlmReasoningMode::ProviderDefault, 42, true, false);

        let timeout = options
            .stream_total_timeout_secs
            .expect("web-search assistant streaming requests must carry a total timeout");
        assert!(
            (480..=600).contains(&timeout),
            "web-search assistant stream budget should be near the upper bounded range, got {timeout}s"
        );
    }
}
