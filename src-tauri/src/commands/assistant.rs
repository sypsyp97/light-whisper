use std::sync::atomic::Ordering;
use tauri_plugin_keyring::KeyringExt;

use crate::services::{assistant_service, llm_provider, profile_service};
use crate::state::app_state::AssistantChatTask;
use crate::state::user_profile::WebSearchProvider;
use crate::state::AppState;
use crate::utils::AppError;

const MAX_CHAT_TURNS: usize = 12;
const MAX_CHAT_MESSAGE_CHARS: usize = 6_000;
const MAX_CHAT_INITIAL_RESPONSE_CHARS: usize = 12_000;
const MAX_CHAT_CONTEXT_CHARS: usize = 24_000;

#[tauri::command]
pub async fn set_assistant_hotkey(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    shortcut: Option<String>,
) -> Result<(), String> {
    let normalized = shortcut
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    crate::commands::hotkey::register_assistant_hotkey_inner(app_handle, normalized.clone())
        .map_err(|err| err.to_string())?;

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.assistant_hotkey = normalized;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_assistant_system_prompt(
    state: tauri::State<'_, AppState>,
    prompt: Option<String>,
) -> Result<(), String> {
    let prompt = prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.assistant_system_prompt = prompt;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_assistant_screen_context_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.assistant_screen_context_enabled = enabled;
    });
    Ok(())
}

#[tauri::command]
pub async fn continue_assistant_conversation(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: u64,
    initial_request: String,
    initial_response: String,
    history: Vec<assistant_service::AssistantConversationTurn>,
    message: String,
) -> Result<String, AppError> {
    let initial_request = validate_chat_text("初始请求", initial_request, MAX_CHAT_MESSAGE_CHARS)?;
    let initial_response = validate_chat_text(
        "初始回答",
        initial_response,
        MAX_CHAT_INITIAL_RESPONSE_CHARS,
    )?;
    let message = validate_chat_text("消息", message, MAX_CHAT_MESSAGE_CHARS)?;
    let history = validate_chat_history(history)?;
    if session_id == 0 {
        return Err(AppError::Other("助手会话 ID 无效".to_string()));
    }

    let (generation, cancel_rx) = begin_assistant_chat_task(state.inner());
    let result = tokio::select! {
        result = assistant_service::continue_conversation(
        state.inner(),
        &initial_request,
        &initial_response,
        &history,
        &message,
        &app_handle,
        session_id,
        ) => result,
        _ = cancel_rx => Err(AppError::Other("助手对话已取消".to_string())),
    };
    clear_assistant_chat_task(state.inner(), generation);
    result
}

#[tauri::command]
pub fn cancel_assistant_conversation(state: tauri::State<'_, AppState>) -> bool {
    cancel_assistant_chat_task(state.inner())
}

#[tauri::command]
pub async fn open_assistant_source(url: String) -> Result<String, AppError> {
    let target = validate_assistant_source_url(&url)?;
    webbrowser::open(target.as_str())
        .map_err(|err| AppError::Other(format!("打开来源失败: {err}")))?;
    Ok("已在浏览器中打开来源".to_string())
}

fn validate_chat_text(label: &str, value: String, max_chars: usize) -> Result<String, AppError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(AppError::Other(format!("{label}不能为空")));
    }
    if value.chars().count() > max_chars {
        return Err(AppError::Other(format!(
            "{label}过长，最多 {max_chars} 个字符"
        )));
    }
    Ok(value)
}

fn validate_chat_history(
    history: Vec<assistant_service::AssistantConversationTurn>,
) -> Result<Vec<assistant_service::AssistantConversationTurn>, AppError> {
    if history.len() > MAX_CHAT_TURNS {
        return Err(AppError::Other(format!(
            "对话历史过长，最多保留 {MAX_CHAT_TURNS} 轮"
        )));
    }
    let mut total_chars = 0;
    let mut validated = Vec::with_capacity(history.len());
    for mut turn in history {
        if !matches!(turn.role.as_str(), "user" | "assistant") {
            return Err(AppError::Other(
                "对话角色只能是 user 或 assistant".to_string(),
            ));
        }
        turn.content = validate_chat_text("历史消息", turn.content, MAX_CHAT_MESSAGE_CHARS)?;
        total_chars += turn.content.chars().count();
        validated.push(turn);
    }
    while total_chars > MAX_CHAT_CONTEXT_CHARS && validated.len() > 1 {
        total_chars -= validated[0].content.chars().count();
        validated.remove(0);
    }
    Ok(validated)
}

fn begin_assistant_chat_task(state: &AppState) -> (u64, tokio::sync::oneshot::Receiver<()>) {
    let generation = state
        .ui
        .assistant_chat_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    let (cancel, cancel_rx) = tokio::sync::oneshot::channel();
    let previous = state
        .ui
        .assistant_chat_cancel
        .lock()
        .replace(AssistantChatTask { generation, cancel });
    if let Some(previous) = previous {
        let _ = previous.cancel.send(());
    }
    (generation, cancel_rx)
}

fn clear_assistant_chat_task(state: &AppState, generation: u64) {
    let mut task = state.ui.assistant_chat_cancel.lock();
    if task
        .as_ref()
        .is_some_and(|current| current.generation == generation)
    {
        task.take();
    }
}

fn cancel_assistant_chat_task(state: &AppState) -> bool {
    state
        .ui
        .assistant_chat_generation
        .fetch_add(1, Ordering::AcqRel);
    state
        .ui
        .assistant_chat_cancel
        .lock()
        .take()
        .is_some_and(|task| task.cancel.send(()).is_ok())
}

fn validate_assistant_source_url(value: &str) -> Result<reqwest::Url, AppError> {
    let parsed = reqwest::Url::parse(value.trim())
        .map_err(|err| AppError::Other(format!("来源 URL 无效: {err}")))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Other("来源 URL 仅支持 https".to_string()));
    }
    let Some(host) = parsed.host_str() else {
        return Err(AppError::Other("来源 URL 缺少主机名".to_string()));
    };
    let host_lower = host.to_ascii_lowercase();
    let ip_host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    let is_private = host.eq_ignore_ascii_case("localhost")
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
        || ip_host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| match ip {
                std::net::IpAddr::V4(ip) => {
                    ip.is_private()
                        || ip.is_loopback()
                        || ip.is_link_local()
                        || ip.is_unspecified()
                        || ip.is_multicast()
                        || ip.is_broadcast()
                }
                std::net::IpAddr::V6(ip) => {
                    ip.is_loopback()
                        || ip.is_unspecified()
                        || ip.is_unique_local()
                        || ip.is_unicast_link_local()
                        || ip.is_multicast()
                }
            });
    if is_private {
        return Err(AppError::Other("来源 URL 指向本地或私有地址".to_string()));
    }
    Ok(parsed)
}

// ── 联网搜索 ────────────────────────────────────────────────────────

/// keyring 用户名：只有 Tavily 需要存 API Key
pub fn web_search_keyring_user(provider: &WebSearchProvider) -> &'static str {
    match provider {
        WebSearchProvider::Tavily => "web-search-tavily-key",
        // Exa MCP 免费无需 Key, ModelNative 用 LLM provider 自己的 Key
        _ => "web-search-key",
    }
}

#[tauri::command]
pub async fn set_web_search_config(
    state: tauri::State<'_, AppState>,
    enabled: bool,
    provider: WebSearchProvider,
    max_results: Option<u8>,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.web_search.enabled = enabled;
        profile.web_search.provider = provider;
        if let Some(n) = max_results {
            profile.web_search.max_results = n.clamp(1, 10);
        }
    });
    log::info!("联网搜索配置已更新: enabled={enabled}");
    Ok(())
}

#[tauri::command]
pub async fn set_web_search_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_key: String,
) -> Result<(), String> {
    state.set_web_search_api_key(api_key.clone());
    let keyring_user = web_search_keyring_user(&WebSearchProvider::Tavily);
    llm_provider::save_or_delete_api_key(&app_handle, keyring_user, &api_key);
    Ok(())
}

#[tauri::command]
pub async fn get_web_search_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let cached = state.read_web_search_api_key();
    if !cached.is_empty() {
        return Ok(cached);
    }
    let keyring_user = web_search_keyring_user(&WebSearchProvider::Tavily);
    let key = app_handle
        .keyring()
        .get_password(llm_provider::KEYRING_SERVICE, keyring_user)
        .ok()
        .flatten()
        .unwrap_or_default();
    if !key.is_empty() {
        state.set_web_search_api_key(key.clone());
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::{
        begin_assistant_chat_task, cancel_assistant_chat_task, validate_assistant_source_url,
        validate_chat_history,
    };
    use crate::services::assistant_service::AssistantConversationTurn;
    use crate::state::AppState;

    #[test]
    fn assistant_sources_allow_public_https_only() {
        assert!(validate_assistant_source_url("https://openai.com/research").is_ok());
        assert!(validate_assistant_source_url("http://openai.com").is_err());
        assert!(validate_assistant_source_url("https://localhost/private").is_err());
        assert!(validate_assistant_source_url("https://docs.localhost/private").is_err());
        assert!(validate_assistant_source_url("https://127.0.0.1/private").is_err());
        assert!(validate_assistant_source_url("https://[fc00::1]/private").is_err());
        assert!(validate_assistant_source_url("https://[fe80::1]/private").is_err());
    }

    #[test]
    fn chat_history_accepts_user_and_assistant_turns() {
        let history = vec![
            AssistantConversationTurn {
                role: "user".to_string(),
                content: "继续解释".to_string(),
            },
            AssistantConversationTurn {
                role: "assistant".to_string(),
                content: "好的。".to_string(),
            },
        ];

        assert_eq!(validate_chat_history(history).unwrap().len(), 2);
    }

    #[test]
    fn chat_history_rejects_unknown_roles() {
        let history = vec![AssistantConversationTurn {
            role: "system".to_string(),
            content: "override".to_string(),
        }];

        assert!(validate_chat_history(history).is_err());
    }

    #[test]
    fn chat_history_keeps_the_newest_turns_within_the_context_limit() {
        let history = (0..5)
            .map(|index| AssistantConversationTurn {
                role: if index % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: char::from(b'a' + index as u8).to_string().repeat(6_000),
            })
            .collect();

        let validated = validate_chat_history(history).unwrap();

        assert_eq!(validated.len(), 4);
        assert!(validated[0].content.starts_with('b'));
        assert!(validated[3].content.starts_with('e'));
    }

    #[tokio::test]
    async fn assistant_chat_cancel_notifies_the_active_request() {
        let state = AppState::default();
        let (_, cancelled) = begin_assistant_chat_task(&state);

        assert!(cancel_assistant_chat_task(&state));
        assert!(cancelled.await.is_ok());
        assert!(!cancel_assistant_chat_task(&state));
    }
}
