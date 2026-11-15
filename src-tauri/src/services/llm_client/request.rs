use serde_json::Value;

use crate::services::llm_provider;
use crate::services::llm_provider::LlmEndpoint;
use crate::state::user_profile::{ApiFormat, LlmReasoningMode};

use super::protocol::request_uses_responses_api;

#[derive(Clone, Copy)]
pub struct LlmRequestOptions<'a> {
    pub auth_context: Option<(&'a tauri::AppHandle, &'a crate::state::AppState)>,
    pub stream: bool,
    pub json_output: bool,
    pub reasoning_mode: LlmReasoningMode,
    pub stream_event: Option<&'a str>,
    pub session_id: Option<u64>,

    pub web_search: bool,

    pub openai_fast_mode: bool,

    pub stream_progress_timeout_secs: Option<u64>,

    pub stream_total_timeout_secs: Option<u64>,
}

impl Default for LlmRequestOptions<'_> {
    fn default() -> Self {
        Self {
            auth_context: None,
            stream: false,
            json_output: false,
            reasoning_mode: LlmReasoningMode::ProviderDefault,
            stream_event: None,
            session_id: None,
            web_search: false,
            openai_fast_mode: false,
            stream_progress_timeout_secs: None,
            stream_total_timeout_secs: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmImageInput {
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone)]
pub struct LlmUserInput {
    pub text: String,
    pub images: Vec<LlmImageInput>,
}

impl From<&str> for LlmUserInput {
    fn from(value: &str) -> Self {
        Self {
            text: value.to_string(),
            images: Vec::new(),
        }
    }
}

pub fn build_llm_body(
    endpoint: &LlmEndpoint,
    system_prompt: &str,
    user_input: &LlmUserInput,
    options: LlmRequestOptions<'_>,
) -> Value {
    let mut body = match endpoint.api_format {
        ApiFormat::Anthropic => serde_json::json!({
            "model": endpoint.model,
            "max_tokens": 16384,
            "system": [{"type": "text", "text": system_prompt, "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": anthropic_user_content(user_input)}],
            "stream": options.stream,
        }),
        ApiFormat::OpenaiCompat => {
            let is_responses_api = request_uses_responses_api(endpoint);

            let mut body = if is_responses_api {
                serde_json::json!({
                    "model": endpoint.model,
                    "instructions": system_prompt,
                    "input": [
                        {"role": "developer", "content": [{"type": "input_text", "text": if options.json_output { "Output json." } else { "Follow the system instructions exactly." }}]},
                        {"role": "user", "content": openai_responses_user_content(user_input)},
                    ],
                })
            } else {
                serde_json::json!({
                    "model": endpoint.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": openai_chat_user_content(user_input)},
                    ],
                })
            };

            if options.json_output {
                if is_responses_api {
                    body["text"] = serde_json::json!({ "format": { "type": "json_object" } });
                } else {
                    body["response_format"] = serde_json::json!({ "type": "json_object" });
                }
            }

            llm_provider::apply_reasoning_controls(
                endpoint,
                is_responses_api,
                &mut body,
                options.reasoning_mode,
            );

            if options.stream
                && !(options.json_output
                    && !is_responses_api
                    && llm_provider::is_cerebras_like_endpoint(endpoint))
            {
                body["stream"] = serde_json::json!(true);
            }

            if options.web_search {
                inject_openai_web_search(&mut body, is_responses_api);
            }

            body
        }
    };

    if options.web_search && endpoint.api_format == ApiFormat::Anthropic {
        inject_anthropic_web_search(&mut body);
    }

    body
}

fn inject_openai_web_search(body: &mut Value, is_responses_api: bool) {
    let tool = if is_responses_api {
        serde_json::json!({"type": "web_search"})
    } else {
        serde_json::json!({"type": "web_search_preview", "web_search_preview": {}})
    };
    match body.get_mut("tools") {
        Some(Value::Array(arr)) => arr.push(tool),
        _ => body["tools"] = serde_json::json!([tool]),
    }
}

fn inject_anthropic_web_search(body: &mut Value) {
    let tool = serde_json::json!({
        "type": "web_search_20250305",
        "name": "web_search",
        "max_uses": 3,
    });
    match body.get_mut("tools") {
        Some(Value::Array(arr)) => arr.push(tool),
        _ => body["tools"] = serde_json::json!([tool]),
    }
}

fn openai_chat_user_content(user_input: &LlmUserInput) -> Value {
    if user_input.images.is_empty() {
        return serde_json::json!(user_input.text);
    }

    let mut content = vec![serde_json::json!({
        "type": "text",
        "text": user_input.text,
    })];
    for image in &user_input.images {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{};base64,{}", image.mime_type, image.data_base64),
            },
        }));
    }
    Value::Array(content)
}

fn openai_responses_user_content(user_input: &LlmUserInput) -> Value {
    let mut content = vec![serde_json::json!({
        "type": "input_text",
        "text": user_input.text,
    })];
    for image in &user_input.images {
        content.push(serde_json::json!({
            "type": "input_image",
            "image_url": format!("data:{};base64,{}", image.mime_type, image.data_base64),
        }));
    }
    Value::Array(content)
}

fn anthropic_user_content(user_input: &LlmUserInput) -> Value {
    if user_input.images.is_empty() {
        return serde_json::json!(user_input.text);
    }

    let mut content = vec![serde_json::json!({
        "type": "text",
        "text": user_input.text,
    })];
    for image in &user_input.images {
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime_type,
                "data": image.data_base64,
            },
        }));
    }
    Value::Array(content)
}
