use super::{asr_snapshot, endpoint_for_request, ModelConnectionRequest, ModelConnectionTarget};
use crate::state::user_profile::{
    ApiFormat, CustomProvider, LlmProviderConfig, LlmReasoningMode, OpenaiAuthMode, XaiAuthMode,
};
use serde_json::json;
use std::future::{poll_fn, Future};
use std::task::Poll;
use std::time::Duration;

#[tokio::test]
async fn saved_asr_snapshot_waits_for_the_lifecycle_lock_before_reading_the_cached_key() {
    let state = crate::state::AppState::new();
    let guard = state.engine.funasr_lifecycle_op.lock().await;
    state.set_online_asr_api_key("synthetic-before-transition");
    let request = request(json!({"target":"asr"}));
    let snapshot = asr_snapshot(&state, &request);
    tokio::pin!(snapshot);

    let pending = poll_fn(|cx| Poll::Ready(snapshot.as_mut().poll(cx).is_pending())).await;
    assert!(
        pending,
        "ASR snapshot must wait while a configuration transition owns the lifecycle lock"
    );
    state.set_online_asr_api_key("synthetic-after-transition");
    drop(guard);

    let captured = tokio::time::timeout(Duration::from_secs(1), snapshot)
        .await
        .expect("snapshot must finish after the transition releases its lock")
        .expect("saved ASR snapshot must resolve");
    assert_eq!(captured.api_key, "synthetic-after-transition");
}

fn request(value: serde_json::Value) -> ModelConnectionRequest {
    serde_json::from_value(value).unwrap()
}

fn config() -> LlmProviderConfig {
    LlmProviderConfig {
        active: "custom".into(),
        custom_base_url: Some("https://polish.example/v1".into()),
        custom_model: Some("stored-polish".into()),
        custom_providers: vec![CustomProvider {
            id: "registered-anthropic".into(),
            name: "Registered test provider".into(),
            base_url: "https://registered.example/v1".into(),
            model: "registered-default".into(),
            api_format: ApiFormat::Anthropic,
        }],
        assistant_use_separate_model: true,
        assistant_provider: Some("registered-anthropic".into()),
        assistant_model: Some("stored-assistant".into()),
        selection_use_separate_model: true,
        selection_provider: Some("registered-anthropic".into()),
        selection_model: Some("stored-selection".into()),
        validation_use_separate_model: true,
        validation_provider: Some("registered-anthropic".into()),
        validation_model: Some("stored-validation".into()),
        screen_vision_enabled: true,
        screen_vision_provider: Some("registered-anthropic".into()),
        screen_vision_model: Some("stored-vision".into()),
        ..Default::default()
    }
}

#[test]
fn request_deserializes_the_frontend_camel_case_contract() {
    let parsed = request(json!({
        "target":"selection", "provider":"custom", "baseUrl":"https://draft.example/v1",
        "model":"draft-model", "apiKey":"synthetic-key", "reasoningMode":"balanced",
        "openaiAuthMode":"oauth", "xaiAuthMode":"api_key", "engine":"glm-asr"
    }));
    assert!(matches!(parsed.target, ModelConnectionTarget::Selection));
    assert_eq!(parsed.provider.as_deref(), Some("custom"));
    assert_eq!(parsed.base_url.as_deref(), Some("https://draft.example/v1"));
    assert_eq!(parsed.model.as_deref(), Some("draft-model"));
    assert_eq!(parsed.api_key.as_deref(), Some("synthetic-key"));
    assert!(matches!(
        parsed.reasoning_mode,
        Some(LlmReasoningMode::Balanced)
    ));
    assert!(matches!(
        parsed.openai_auth_mode,
        Some(OpenaiAuthMode::Oauth)
    ));
    assert!(matches!(parsed.xai_auth_mode, Some(XaiAuthMode::ApiKey)));
    assert_eq!(parsed.engine.as_deref(), Some("glm-asr"));
}

#[test]
fn resolver_uses_explicit_drafts_without_requiring_or_changing_credentials() {
    let config = config();
    for key in [serde_json::Value::Null, json!("synthetic-draft-key")] {
        let request = request(
            json!({"target":"assistant", "provider":"custom", "baseUrl":"https://draft.example/v1", "model":"draft-model", "apiKey":key}),
        );
        let endpoint = endpoint_for_request(&config, &request).unwrap();
        assert_eq!(endpoint.provider, "custom");
        assert_eq!(
            endpoint.api_url,
            "https://draft.example/v1/chat/completions"
        );
        assert_eq!(endpoint.model, "draft-model");
        assert_eq!(endpoint.api_format, ApiFormat::OpenaiCompat);
        assert_eq!(request.api_key, key.as_str().map(str::to_owned));
    }
}

#[test]
fn resolver_uses_each_stored_role_when_no_draft_is_supplied() {
    let config = config();
    for (target, model) in [
        ("polish", "stored-polish"),
        ("assistant", "stored-assistant"),
        ("selection", "stored-selection"),
        ("validation", "stored-validation"),
        ("vision", "stored-vision"),
    ] {
        let endpoint = endpoint_for_request(&config, &request(json!({"target":target}))).unwrap();
        assert_eq!(endpoint.model, model, "{target}");
        if target == "polish" {
            assert_eq!(endpoint.provider, "custom");
            assert_eq!(
                endpoint.api_url,
                "https://polish.example/v1/chat/completions"
            );
        } else {
            assert_eq!(endpoint.provider, "registered-anthropic");
            assert_eq!(endpoint.api_url, "https://registered.example/v1/messages");
            assert_eq!(endpoint.api_format, ApiFormat::Anthropic);
        }
    }
}

#[test]
fn resolver_preserves_registered_anthropic_format_with_draft_endpoint_and_model() {
    let endpoint = endpoint_for_request(
        &config(),
        &request(json!({
            "target":"vision", "provider":"registered-anthropic",
            "baseUrl":"https://draft-anthropic.example/v1", "model":"draft-anthropic-model"
        })),
    )
    .unwrap();
    assert_eq!(endpoint.provider, "registered-anthropic");
    assert_eq!(
        endpoint.api_url,
        "https://draft-anthropic.example/v1/messages"
    );
    assert_eq!(endpoint.model, "draft-anthropic-model");
    assert_eq!(endpoint.api_format, ApiFormat::Anthropic);
}

#[test]
fn resolver_uses_registered_model_default_unless_the_role_has_a_matching_saved_model() {
    for (target, expected) in [
        ("polish", "registered-default"),
        ("assistant", "stored-assistant"),
    ] {
        let endpoint = endpoint_for_request(
            &config(),
            &request(json!({
                "target":target, "provider":"registered-anthropic"
            })),
        )
        .unwrap();
        assert_eq!(endpoint.model, expected, "{target}");
        assert_eq!(endpoint.provider, "registered-anthropic");
        assert_eq!(endpoint.api_format, ApiFormat::Anthropic);
    }
}

#[test]
fn resolver_rejects_unknown_provider_and_unsafe_or_invalid_draft_urls() {
    for value in [
        json!({"target":"polish", "provider":"removed-provider"}),
        json!({"target":"polish", "provider":"custom", "baseUrl":"file:///private/model"}),
        json!({"target":"polish", "provider":"custom", "baseUrl":"not a URL"}),
        json!({"target":"polish", "provider":"custom", "baseUrl":"https://user:private-password@example.test/v1"}),
    ] {
        let error = endpoint_for_request(&config(), &request(value))
            .err()
            .expect("invalid provider or URL must be rejected");
        assert!(!error.is_empty());
        assert!(!error.contains("private-password"));
    }
}
