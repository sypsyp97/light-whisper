use std::time::{SystemTime, UNIX_EPOCH};

use super::codex_oauth_service::{should_prewarm_runtime_session, OpenaiCodexOauthSession};
use crate::state::user_profile::{LlmProviderConfig, OpenaiAuthMode};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn config(active: &str, openai_auth_mode: Option<OpenaiAuthMode>) -> LlmProviderConfig {
    LlmProviderConfig {
        active: active.to_string(),
        openai_auth_mode,
        ..Default::default()
    }
}

fn session(
    access_token: &str,
    api_key: &str,
    expires_at_ms: Option<u64>,
) -> OpenaiCodexOauthSession {
    OpenaiCodexOauthSession {
        id_token: "id-token".to_string(),
        access_token: access_token.to_string(),
        refresh_token: "refresh-token".to_string(),
        api_key: api_key.to_string(),
        expires_at_ms,
        account_id: Some("account-id".to_string()),
        email: Some("user@example.com".to_string()),
        plan_type: Some("pro".to_string()),
    }
}

#[test]
fn refresh_token_only_openai_oauth_session_should_prewarm() {
    let config = config("openai", Some(OpenaiAuthMode::Oauth));
    let session = session("", "", Some(now_ms() + 3_600_000));

    assert!(should_prewarm_runtime_session(
        "openai",
        &config,
        Some(&session)
    ));
}

#[test]
fn warmed_unexpired_openai_oauth_session_should_skip_prewarm() {
    let config = config("openai", Some(OpenaiAuthMode::Oauth));
    let session = session(
        "already-warm-access-token",
        "sk-already-warm-api-key",
        Some(now_ms() + 3_600_000),
    );

    assert!(!should_prewarm_runtime_session(
        "openai",
        &config,
        Some(&session)
    ));
}

#[test]
fn api_key_mode_should_skip_oauth_prewarm() {
    let config = config("openai", Some(OpenaiAuthMode::ApiKey));
    let session = session("", "", Some(now_ms() + 3_600_000));

    assert!(!should_prewarm_runtime_session(
        "openai",
        &config,
        Some(&session)
    ));
}

#[test]
fn non_openai_provider_should_skip_oauth_prewarm() {
    let config = config("deepseek", Some(OpenaiAuthMode::Oauth));
    let session = session("", "", Some(now_ms() + 3_600_000));

    assert!(!should_prewarm_runtime_session(
        "deepseek",
        &config,
        Some(&session)
    ));
}

#[test]
fn expiring_openai_oauth_session_should_prewarm_even_when_tokens_exist() {
    let config = config("openai", Some(OpenaiAuthMode::Oauth));
    let session = session(
        "existing-access-token",
        "sk-existing-api-key",
        Some(now_ms().saturating_sub(1_000)),
    );

    assert!(should_prewarm_runtime_session(
        "openai",
        &config,
        Some(&session)
    ));
}
