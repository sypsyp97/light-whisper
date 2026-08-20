use std::time::{SystemTime, UNIX_EPOCH};

use super::grok_build_oauth_service::{
    decode_grok_build_oauth_access_token, effective_xai_auth_mode,
    encode_grok_build_oauth_access_token, is_grok_build_oauth_origin_auth,
    resolve_image_support_for_request, should_prewarm_runtime_session,
    GrokBuildOauthDeviceCodeChallenge, GrokBuildOauthSession, GrokBuildOauthStatus,
    GROK_BUILD_INFERENCE_BASE_URL, GROK_BUILD_OAUTH_PREFIX, XAI_API_KEY_INFERENCE_BASE_URL,
};
use super::llm_provider::{self, build_auth_headers};
use crate::state::user_profile::{ApiFormat, LlmProviderConfig, XaiAuthMode};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn config(active: &str, xai_auth_mode: Option<XaiAuthMode>) -> LlmProviderConfig {
    LlmProviderConfig {
        active: active.to_string(),
        xai_auth_mode,
        ..Default::default()
    }
}

fn session(access_token: &str, expires_at_ms: Option<u64>) -> GrokBuildOauthSession {
    GrokBuildOauthSession {
        access_token: access_token.to_string(),
        expires_at_ms,
        ..Default::default()
    }
}

fn header_value(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

#[test]
fn grok_build_prefix_and_inference_urls_match_the_spec() {
    assert_eq!(GROK_BUILD_OAUTH_PREFIX, "grok-build-oauth:");
    assert_eq!(
        GROK_BUILD_INFERENCE_BASE_URL,
        "https://cli-chat-proxy.grok.com"
    );
    assert_eq!(XAI_API_KEY_INFERENCE_BASE_URL, "https://api.x.ai");
    assert_eq!(
        format!("{GROK_BUILD_INFERENCE_BASE_URL}/v1/responses"),
        "https://cli-chat-proxy.grok.com/v1/responses"
    );
    assert_eq!(
        format!("{GROK_BUILD_INFERENCE_BASE_URL}/v1/models"),
        "https://cli-chat-proxy.grok.com/v1/models"
    );
    assert_eq!(
        format!("{XAI_API_KEY_INFERENCE_BASE_URL}/v1/responses"),
        "https://api.x.ai/v1/responses"
    );
    assert_eq!(
        format!("{XAI_API_KEY_INFERENCE_BASE_URL}/v1/models"),
        "https://api.x.ai/v1/models"
    );
}

#[test]
fn encode_grok_build_oauth_access_token_trims_and_rejects_empty() {
    assert_eq!(
        encode_grok_build_oauth_access_token("access-token").as_deref(),
        Some("grok-build-oauth:access-token")
    );
    assert_eq!(
        encode_grok_build_oauth_access_token("  access-token  ").as_deref(),
        Some("grok-build-oauth:access-token")
    );
    assert_eq!(encode_grok_build_oauth_access_token(""), None);
    assert_eq!(encode_grok_build_oauth_access_token("   "), None);
    assert_eq!(encode_grok_build_oauth_access_token("\n\t"), None);
}

#[test]
fn decode_grok_build_oauth_access_token_requires_prefix_and_payload() {
    assert_eq!(
        decode_grok_build_oauth_access_token("grok-build-oauth:access-token").as_deref(),
        Some("access-token")
    );
    assert_eq!(
        decode_grok_build_oauth_access_token("  grok-build-oauth:access-token  ").as_deref(),
        Some("access-token")
    );
    assert_eq!(decode_grok_build_oauth_access_token("access-token"), None);
    assert_eq!(
        decode_grok_build_oauth_access_token("openai-codex-oauth-api-key:sk-test"),
        None
    );
    assert_eq!(
        decode_grok_build_oauth_access_token("grok-build-oauth:"),
        None
    );
    assert_eq!(
        decode_grok_build_oauth_access_token("grok-build-oauth:   "),
        None
    );
}

#[test]
fn encode_then_decode_round_trips_a_runtime_access_token() {
    let encoded =
        encode_grok_build_oauth_access_token("  runtime-token  ").expect("non-empty tokens encode");
    assert_eq!(
        decode_grok_build_oauth_access_token(&encoded).as_deref(),
        Some("runtime-token")
    );
}

#[test]
fn grok_build_oauth_treats_unknown_image_support_as_text_only() {
    let wrapped =
        encode_grok_build_oauth_access_token("access-token").expect("non-empty tokens encode");
    assert_eq!(
        resolve_image_support_for_request(&wrapped, None),
        Some(false)
    );
    assert_eq!(
        resolve_image_support_for_request(&wrapped, Some(true)),
        Some(true)
    );
    assert_eq!(
        resolve_image_support_for_request("xai-plain-api-key", None),
        None
    );
}

#[test]
fn is_grok_build_oauth_origin_auth_accepts_only_decodable_prefixed_tokens() {
    assert!(is_grok_build_oauth_origin_auth(
        "grok-build-oauth:access-token"
    ));
    assert!(!is_grok_build_oauth_origin_auth(""));
    assert!(!is_grok_build_oauth_origin_auth("xai-plain-api-key"));
    assert!(!is_grok_build_oauth_origin_auth(
        "openai-codex-oauth-api-key:sk-test"
    ));
    assert!(!is_grok_build_oauth_origin_auth("grok-build-oauth:"));
}

#[test]
fn effective_xai_auth_mode_prefers_stored_choice_then_login_state() {
    assert_eq!(
        effective_xai_auth_mode(Some(XaiAuthMode::ApiKey), true),
        XaiAuthMode::ApiKey
    );
    assert_eq!(
        effective_xai_auth_mode(Some(XaiAuthMode::Oauth), false),
        XaiAuthMode::Oauth
    );
    assert_eq!(effective_xai_auth_mode(None, true), XaiAuthMode::Oauth);
    assert_eq!(effective_xai_auth_mode(None, false), XaiAuthMode::ApiKey);
}

#[test]
fn xai_auth_mode_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_value(XaiAuthMode::ApiKey).expect("serialize api_key"),
        serde_json::json!("api_key")
    );
    assert_eq!(
        serde_json::to_value(XaiAuthMode::Oauth).expect("serialize oauth"),
        serde_json::json!("oauth")
    );
}

#[test]
fn llm_provider_config_persists_xai_auth_mode_and_omits_unset() {
    let stored = LlmProviderConfig {
        xai_auth_mode: Some(XaiAuthMode::Oauth),
        ..Default::default()
    };
    let stored_json = serde_json::to_value(&stored).expect("serialize stored mode");
    assert_eq!(stored_json["xai_auth_mode"], "oauth");

    let unset = LlmProviderConfig::default();
    let unset_json = serde_json::to_value(&unset).expect("serialize default");
    assert!(unset_json.get("xai_auth_mode").is_none());
}

#[test]
fn grok_build_status_and_device_code_challenge_use_camel_case_wire_shape() {
    let status = GrokBuildOauthStatus {
        logged_in: true,
        email: Some("user@example.com".to_string()),
        plan_type: Some("super".to_string()),
        account_id: Some("acc-1".to_string()),
        expires_at_ms: Some(1_700_000_000_000),
    };
    let status_json = serde_json::to_value(&status).expect("serialize status");
    assert_eq!(status_json["loggedIn"], true);
    assert_eq!(status_json["email"], "user@example.com");
    assert_eq!(status_json["planType"], "super");
    assert_eq!(status_json["accountId"], "acc-1");
    assert_eq!(status_json["expiresAtMs"].as_u64(), Some(1_700_000_000_000));

    let challenge = GrokBuildOauthDeviceCodeChallenge {
        verification_url: "https://auth.x.ai/device".to_string(),
        user_code: "ABCD-1234".to_string(),
        device_code: "device-code".to_string(),
        interval_secs: 5,
    };
    let challenge_json = serde_json::to_value(&challenge).expect("serialize challenge");
    assert_eq!(
        challenge_json["verificationUrl"],
        "https://auth.x.ai/device"
    );
    assert_eq!(challenge_json["userCode"], "ABCD-1234");
    assert_eq!(challenge_json["deviceCode"], "device-code");
    assert_eq!(challenge_json["intervalSecs"], 5);
    assert!(challenge_json.get("deviceAuthId").is_none());
}

#[test]
fn logged_out_status_reports_logged_in_false() {
    let status = GrokBuildOauthStatus {
        logged_in: false,
        email: None,
        plan_type: None,
        account_id: None,
        expires_at_ms: None,
    };
    let json = serde_json::to_value(&status).expect("serialize logged-out status");
    assert_eq!(json["loggedIn"], false);
}

#[test]
fn missing_access_token_xai_oauth_session_should_prewarm() {
    let config = config("xai", Some(XaiAuthMode::Oauth));
    let session = session("", Some(now_ms() + 3_600_000));

    assert!(should_prewarm_runtime_session(
        "xai",
        &config,
        Some(&session)
    ));
}

#[test]
fn whitespace_only_access_token_xai_oauth_session_should_prewarm() {
    let config = config("xai", Some(XaiAuthMode::Oauth));
    let session = session("   ", Some(now_ms() + 3_600_000));

    assert!(should_prewarm_runtime_session(
        "xai",
        &config,
        Some(&session)
    ));
}

#[test]
fn warmed_unexpired_xai_oauth_session_should_skip_prewarm() {
    let config = config("xai", Some(XaiAuthMode::Oauth));
    let session = session("already-warm-access-token", Some(now_ms() + 3_600_000));

    assert!(!should_prewarm_runtime_session(
        "xai",
        &config,
        Some(&session)
    ));
}

#[test]
fn api_key_mode_should_skip_grok_build_oauth_prewarm() {
    let config = config("xai", Some(XaiAuthMode::ApiKey));
    let session = session("", Some(now_ms() + 3_600_000));

    assert!(!should_prewarm_runtime_session(
        "xai",
        &config,
        Some(&session)
    ));
}

#[test]
fn non_xai_provider_should_skip_grok_build_oauth_prewarm() {
    let config = config("openai", Some(XaiAuthMode::Oauth));
    let session = session("", Some(now_ms() + 3_600_000));

    assert!(!should_prewarm_runtime_session(
        "openai",
        &config,
        Some(&session)
    ));
    assert!(!should_prewarm_runtime_session(
        "deepseek",
        &config,
        Some(&session)
    ));
}

#[test]
fn missing_session_should_skip_grok_build_oauth_prewarm() {
    let config = config("xai", Some(XaiAuthMode::Oauth));

    assert!(!should_prewarm_runtime_session("xai", &config, None));
}

#[test]
fn expiring_xai_oauth_session_should_prewarm_even_when_access_token_exists() {
    let config = config("xai", Some(XaiAuthMode::Oauth));
    let session = session(
        "existing-access-token",
        Some(now_ms().saturating_sub(1_000)),
    );

    assert!(should_prewarm_runtime_session(
        "xai",
        &config,
        Some(&session)
    ));
}

#[test]
fn unset_stored_mode_with_a_session_defaults_to_oauth_prewarm_rules() {
    let config = config("xai", None);
    let session = session("", Some(now_ms() + 3_600_000));

    assert!(should_prewarm_runtime_session(
        "xai",
        &config,
        Some(&session)
    ));
}

#[test]
fn xai_is_a_builtin_responses_provider() {
    let config = config("xai", None);
    assert_eq!(config.resolve_active_provider(), "xai");

    let endpoint = llm_provider::endpoint_for_config(&config);
    assert_eq!(endpoint.provider, "xai");
    assert_eq!(endpoint.api_url, "https://api.x.ai/v1/responses");
    assert_eq!(endpoint.model, "grok-4.6");
    assert!(llm_provider::endpoint_uses_responses_api(&endpoint));
}

#[test]
fn openai_and_deepseek_presets_remain_unchanged() {
    let openai = llm_provider::endpoint_for_config(&config("openai", None));
    assert_eq!(openai.provider, "openai");
    assert_eq!(openai.api_url, "https://api.openai.com/v1/responses");

    let deepseek = llm_provider::endpoint_for_config(&config("deepseek", None));
    assert_eq!(deepseek.provider, "deepseek");
    assert_eq!(
        deepseek.api_url,
        "https://api.deepseek.com/v1/chat/completions"
    );
}

#[test]
fn grok_build_oauth_origin_auth_decodes_bearer_and_sets_cli_headers() {
    let wrapped =
        encode_grok_build_oauth_access_token("grok-access-token").expect("non-empty tokens encode");

    let headers = build_auth_headers(&ApiFormat::OpenaiCompat, &wrapped)
        .expect("Grok Build origin auth should build headers");

    assert_eq!(
        header_value(&headers, "Authorization").as_deref(),
        Some("Bearer grok-access-token")
    );
    assert_eq!(
        header_value(&headers, "User-Agent").as_deref(),
        Some("xai-grok-cli")
    );
    assert_eq!(
        header_value(&headers, "x-grok-client-identifier").as_deref(),
        Some("grok-shell")
    );
    assert_eq!(
        header_value(&headers, "X-XAI-Token-Auth").as_deref(),
        Some("xai-grok-cli")
    );
    assert_eq!(
        header_value(&headers, "x-grok-client-version").as_deref(),
        Some("0.2.114")
    );
}

#[test]
fn xai_api_key_auth_does_not_set_cli_headers() {
    let headers = build_auth_headers(&ApiFormat::OpenaiCompat, "xai-plain-api-key")
        .expect("plain API keys should still build headers");

    assert_eq!(
        header_value(&headers, "Authorization").as_deref(),
        Some("Bearer xai-plain-api-key")
    );
    assert!(header_value(&headers, "x-grok-client-identifier").is_none());
    assert!(header_value(&headers, "X-XAI-Token-Auth").is_none());
    assert!(header_value(&headers, "x-grok-client-version").is_none());
}

#[test]
fn openai_oauth_derived_api_keys_do_not_gain_grok_cli_headers() {
    let wrapped = super::codex_oauth_service::encode_oauth_api_key("sk-oauth-session")
        .expect("openai oauth keys encode");
    let headers = build_auth_headers(&ApiFormat::OpenaiCompat, &wrapped)
        .expect("OpenAI OAuth-derived keys should still build headers");

    assert_eq!(
        header_value(&headers, "Authorization").as_deref(),
        Some("Bearer sk-oauth-session")
    );
    assert!(header_value(&headers, "x-grok-client-identifier").is_none());
    assert!(header_value(&headers, "X-XAI-Token-Auth").is_none());
}
