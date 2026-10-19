use super::llm_client::{build_llm_body, send_llm_request, LlmRequestOptions, LlmUserInput};
use super::llm_provider::{self, LlmEndpoint};
use super::llm_reasoning_negotiation_tests::spawn_scripted_server;
use crate::state::user_profile::{ApiFormat, LlmProviderConfig, LlmReasoningMode};
use serde_json::{json, Value};

fn endpoint(provider: &str, api_url: &str, model: &str) -> LlmEndpoint {
    LlmEndpoint {
        provider: provider.to_string(),
        api_url: api_url.to_string(),
        model: model.to_string(),
        timeout_secs: 10,
        api_format: ApiFormat::OpenaiCompat,
    }
}

fn modes() -> [(LlmReasoningMode, Option<Value>); 5] {
    [
        (LlmReasoningMode::ProviderDefault, None),
        (LlmReasoningMode::Off, Some(json!({"enabled": false}))),
        (LlmReasoningMode::Light, Some(json!({"effort": "low"}))),
        (
            LlmReasoningMode::Balanced,
            Some(json!({"effort": "medium"})),
        ),
        (LlmReasoningMode::Deep, Some(json!({"effort": "high"}))),
    ]
}

fn success_response(suffix: &str) -> Value {
    if suffix == "responses" {
        json!({"output": [{"type": "message", "content": [{"type": "output_text", "text": "ok"}]}]})
    } else {
        json!({"choices": [{"message": {"content": "ok"}}]})
    }
}

fn assert_no_vendor_reasoning_controls(body: &Value) {
    for key in [
        "reasoning_effort",
        "thinking",
        "thinking_budget",
        "enable_thinking",
        "disable_reasoning",
        "chat_template_kwargs",
    ] {
        assert!(
            body.get(key).is_none(),
            "unexpected vendor control {key}: {body}"
        );
    }
}

#[test]
fn builtin_openrouter_resolves_defaults_catalog_keyring_and_saved_model() {
    let mut config = LlmProviderConfig {
        active: "openrouter".to_string(),
        ..LlmProviderConfig::default()
    };
    assert!(LlmProviderConfig::is_builtin_provider("openrouter"));
    assert_eq!(config.resolve_active_provider(), "openrouter");
    let resolved = llm_provider::endpoint_for_config(&config);
    let preview =
        llm_provider::endpoint_for_preview("openrouter", None, None, ApiFormat::OpenaiCompat);
    assert_eq!(resolved.provider, "openrouter");
    assert_eq!(
        resolved.api_url,
        "https://openrouter.ai/api/v1/chat/completions"
    );
    assert_eq!(resolved.model, "~deepseek/deepseek-flash-latest");
    assert_eq!(resolved.api_format, ApiFormat::OpenaiCompat);
    assert_eq!(preview.api_url, resolved.api_url);
    assert_eq!(preview.model, resolved.model);
    assert_eq!(
        llm_provider::models_url(&config, "openrouter", None),
        "https://openrouter.ai/api/v1/models"
    );
    assert_eq!(
        llm_provider::keyring_user_for_provider("openrouter"),
        "openrouter-api-key"
    );

    config.custom_model = Some("anthropic/claude-sonnet-4-5".to_string());
    assert_eq!(
        llm_provider::endpoint_for_config(&config).model,
        "anthropic/claude-sonnet-4-5"
    );
}

#[test]
fn builtin_and_custom_openrouter_hosts_use_unified_reasoning_for_every_vendor() {
    for provider in ["openrouter", "saved-custom-openrouter"] {
        for suffix in ["chat/completions", "responses"] {
            for model in [
                "~deepseek/deepseek-flash-latest",
                "deepseek/deepseek-reasoner",
                "doubao-seed-2-0-pro",
                "anthropic/claude-sonnet-4-5",
                "openai/gpt-5-mini",
                "Qwen/Qwen3-32B",
            ] {
                let endpoint = endpoint(
                    provider,
                    &format!("https://openrouter.ai/api/v1/{suffix}"),
                    model,
                );
                assert!(llm_provider::is_openrouter_like_endpoint(&endpoint));
                assert_eq!(
                    llm_provider::reasoning_support(&endpoint, suffix == "responses")
                        .strategy
                        .as_deref(),
                    Some("openrouter_reasoning")
                );
                for (mode, expected) in modes() {
                    let body = build_llm_body(
                        &endpoint,
                        "system",
                        &LlmUserInput::from("hello"),
                        LlmRequestOptions {
                            reasoning_mode: mode,
                            ..LlmRequestOptions::default()
                        },
                    );
                    assert_eq!(
                        body.get("reasoning"),
                        expected.as_ref(),
                        "{provider}/{suffix}/{model}: {body}"
                    );
                    assert_no_vendor_reasoning_controls(&body);
                }
            }
        }
    }
}

#[test]
fn openrouter_hostname_in_path_query_userinfo_or_suffix_cannot_select_openrouter_controls() {
    for api_url in [
        "https://gateway.example/openrouter.ai/api/v1/chat/completions",
        "https://gateway.example/v1/chat/completions?upstream=https://openrouter.ai",
        "https://openrouter.ai@gateway.example/v1/chat/completions",
        "https://openrouter.ai.evil.test/api/v1/chat/completions",
    ] {
        let endpoint = endpoint("custom-gateway", api_url, "future-model");
        assert!(
            !llm_provider::is_openrouter_like_endpoint(&endpoint),
            "{api_url}"
        );
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                reasoning_mode: LlmReasoningMode::Off,
                ..LlmRequestOptions::default()
            },
        );
        assert!(body.get("reasoning").is_none(), "{api_url}: {body}");
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
    }
}

#[test]
fn anthropic_api_format_keeps_native_controls_even_with_openrouter_provider_and_host() {
    let mut endpoint = endpoint(
        "openrouter",
        "https://openrouter.ai/api/v1/messages",
        "anthropic/claude-sonnet-4-5",
    );
    endpoint.api_format = ApiFormat::Anthropic;
    assert!(!llm_provider::is_openrouter_like_endpoint(&endpoint));
    let mut body = build_llm_body(
        &endpoint,
        "system",
        &LlmUserInput::from("hello"),
        LlmRequestOptions {
            reasoning_mode: LlmReasoningMode::Balanced,
            ..LlmRequestOptions::default()
        },
    );
    llm_provider::apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Balanced);
    assert_eq!(
        body["thinking"],
        json!({"type": "enabled", "budget_tokens": 2048})
    );
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("system").is_some());
}

#[tokio::test]
async fn openrouter_transport_sends_unified_reasoning_without_optional_output_caps() {
    for suffix in ["chat/completions", "responses"] {
        for (mode, expected) in modes() {
            let (api_url, server) = spawn_scripted_server(
                &format!("/api/v1/{suffix}"),
                vec![("200 OK", success_response(suffix))],
            )
            .await;
            let endpoint = endpoint("openrouter", &api_url, "doubao-seed-2-0-pro");
            let options = LlmRequestOptions {
                json_output: true,
                web_search: true,
                reasoning_mode: mode,
                ..LlmRequestOptions::default()
            };
            let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
            let response = send_llm_request(
                &reqwest::Client::builder().no_proxy().build().unwrap(),
                &endpoint,
                "test-key",
                &body,
                5,
                None,
                options,
            )
            .await
            .expect("OpenRouter request should reach the local capture server");
            assert_eq!(response, "ok");
            let bodies = server.await.unwrap();
            assert_eq!(bodies, vec![body]);
            assert_eq!(bodies[0].get("reasoning"), expected.as_ref());
            assert_no_vendor_reasoning_controls(&bodies[0]);
            for key in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
                assert!(
                    bodies[0].get(key).is_none(),
                    "unexpected {key}: {}",
                    bodies[0]
                );
            }
        }
    }
}

#[tokio::test]
async fn openrouter_required_reasoning_rejection_retries_once_without_controls() {
    for suffix in ["chat/completions", "responses"] {
        for message in [
            "reasoning is unsupported for this model",
            "reasoning cannot be disabled",
        ] {
            let (api_url, server) = spawn_scripted_server(
                &format!("/api/v1/{suffix}"),
                vec![
                    ("400 Bad Request", json!({"error": {"message": message}})),
                    ("200 OK", success_response(suffix)),
                ],
            )
            .await;
            let endpoint = endpoint("openrouter", &api_url, "required-reasoning-model");
            let options = LlmRequestOptions {
                reasoning_mode: LlmReasoningMode::Off,
                json_output: true,
                ..LlmRequestOptions::default()
            };
            let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
            let response = send_llm_request(
                &reqwest::Client::builder().no_proxy().build().unwrap(),
                &endpoint,
                "test-key",
                &body,
                5,
                None,
                options,
            )
            .await
            .expect("a mandatory-reasoning model should accept the one no-controls fallback");
            assert_eq!(response, "ok");
            let bodies = server.await.unwrap();
            assert_eq!(bodies.len(), 2);
            assert_eq!(bodies[0]["reasoning"], json!({"enabled": false}));
            assert_no_vendor_reasoning_controls(&bodies[1]);
            let mut without_controls = bodies[0].clone();
            without_controls
                .as_object_mut()
                .unwrap()
                .remove("reasoning");
            assert_eq!(bodies[1], without_controls);
        }
    }
}

#[tokio::test]
async fn openrouter_required_reasoning_errors_stop_after_auth_or_second_rejection() {
    for suffix in ["chat/completions", "responses"] {
        for statuses in [
            vec!["401 Unauthorized"],
            vec!["403 Forbidden"],
            vec!["400 Bad Request", "400 Bad Request"],
        ] {
            let rejection = json!({"error": {"message": "reasoning cannot be disabled"}});
            let responses = statuses
                .iter()
                .map(|&status| (status, rejection.clone()))
                .collect();
            let (api_url, server) =
                spawn_scripted_server(&format!("/api/v1/{suffix}"), responses).await;
            let endpoint = endpoint("openrouter", &api_url, "required-reasoning-error-model");
            let options = LlmRequestOptions {
                reasoning_mode: LlmReasoningMode::Off,
                ..LlmRequestOptions::default()
            };
            let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
            let error = send_llm_request(
                &reqwest::Client::builder().no_proxy().build().unwrap(),
                &endpoint,
                "test-key",
                &body,
                5,
                None,
                options,
            )
            .await
            .expect_err("auth failures and a rejected fallback must stop compatibility retries");
            assert!(
                error.contains("reasoning cannot be disabled"),
                "the final provider rejection must survive: {error}"
            );
            let bodies = server.await.unwrap();
            assert_eq!(bodies.len(), statuses.len());
            assert_eq!(bodies[0], body);
            if bodies.len() == 2 {
                let mut without_controls = body.clone();
                without_controls
                    .as_object_mut()
                    .unwrap()
                    .remove("reasoning");
                assert_eq!(bodies[1], without_controls);
            }
        }
    }
}

#[tokio::test]
async fn required_reasoning_message_does_not_change_other_providers_retry_behavior() {
    let (api_url, server) = spawn_scripted_server(
        "/v1/chat/completions",
        vec![(
            "400 Bad Request",
            json!({"error": {"message": "reasoning cannot be disabled"}}),
        )],
    )
    .await;
    let endpoint = endpoint("custom-gateway", &api_url, "future-model");
    let options = LlmRequestOptions {
        reasoning_mode: LlmReasoningMode::Off,
        ..LlmRequestOptions::default()
    };
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("hello"), options);
    let error = send_llm_request(
        &reqwest::Client::builder().no_proxy().build().unwrap(),
        &endpoint,
        "test-key",
        &body,
        5,
        None,
        options,
    )
    .await
    .expect_err("the OpenRouter-specific rejection must not broaden generic compatibility retries");
    assert!(error.contains("reasoning cannot be disabled"));
    assert_eq!(server.await.unwrap(), vec![body]);
}
