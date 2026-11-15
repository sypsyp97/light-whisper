mod events;
mod protocol;
mod request;
mod stream;
mod transport;

pub(crate) use events::emit_api_warning;

pub use request::{build_llm_body, LlmImageInput, LlmRequestOptions, LlmUserInput};
#[cfg(test)]
pub use stream::{read_anthropic_sse_stream, read_openai_responses_sse_stream, read_sse_stream};
pub use transport::send_llm_request;

pub(crate) use protocol::{
    ensure_non_empty_llm_content, is_empty_llm_response_error, is_terminal_provider_error,
};

pub(crate) use stream::AI_POLISH_STREAM_PROGRESS_TIMEOUT_SECS;

#[cfg(test)]
pub(crate) use events::{
    build_stream_error_payload, build_stream_event_payload, collect_url_citation_payloads,
};
#[cfg(test)]
pub(crate) use protocol::{
    adapt_body_for_backend, extract_api_error_message, extract_content,
    extract_openai_compat_error_message, finalize_responses_sse_accumulated,
    looks_like_output_token_limit_unsupported_error, EMPTY_LLM_RESPONSE_ERROR_PREFIX,
    OPENAI_RESPONSES_SERVICE_TIER_WHITELIST,
};
#[cfg(test)]
pub(crate) use stream::{apply_responses_done_text, stream_read_budget_at, stream_total_timeout};
#[cfg(test)]
pub(crate) use transport::{dynamic_timeout, is_retryable_overload_error, request_url_for_backend};

#[cfg(test)]
mod tests {
    use super::{
        adapt_body_for_backend, apply_responses_done_text, build_llm_body,
        build_stream_error_payload, build_stream_event_payload, collect_url_citation_payloads,
        dynamic_timeout, ensure_non_empty_llm_content, extract_api_error_message, extract_content,
        extract_openai_compat_error_message, finalize_responses_sse_accumulated,
        is_retryable_overload_error, looks_like_output_token_limit_unsupported_error,
        request_url_for_backend, stream_read_budget_at, LlmRequestOptions, LlmUserInput,
        OPENAI_RESPONSES_SERVICE_TIER_WHITELIST,
    };
    use crate::services::codex_oauth_service;
    use crate::services::grok_build_oauth_service;
    use crate::services::llm_provider::LlmEndpoint;
    use crate::state::user_profile::{ApiFormat, LlmReasoningMode};
    use base64::Engine;
    use std::time::Duration;

    fn make_test_endpoint() -> crate::services::llm_provider::LlmEndpoint {
        crate::services::llm_provider::LlmEndpoint {
            provider: "test-provider".to_string(),
            api_url: "https://test.example.com/v1/chat/completions".to_string(),
            model: "test-model-x".to_string(),
            timeout_secs: 10,
            api_format: crate::state::user_profile::ApiFormat::OpenaiCompat,
        }
    }

    fn openai_endpoint(api_url: &str) -> LlmEndpoint {
        LlmEndpoint {
            provider: "openai".to_string(),
            api_url: api_url.to_string(),
            model: "gpt-4.1-mini".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::OpenaiCompat,
        }
    }

    fn chatgpt_codex_api_key() -> String {
        format!(
            "openai-codex-chatgpt:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"access_token":"test","account_id":"acc"}"#)
        )
    }

    fn oauth_wrapped_api_key() -> String {
        codex_oauth_service::encode_oauth_api_key("sk-oauth-session").expect("wrapped key")
    }

    #[test]
    fn request_url_for_backend_selects_origin_urls_only_for_matching_oauth() {
        let grok_endpoint = LlmEndpoint {
            provider: "xai".to_string(),
            api_url: "https://api.x.ai/v1/responses".to_string(),
            model: "grok-4.6".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::OpenaiCompat,
        };
        let grok_oauth_key =
            grok_build_oauth_service::encode_grok_build_oauth_access_token("grok-access-token")
                .expect("Grok OAuth token should encode");
        assert_eq!(
            request_url_for_backend(&grok_endpoint, &grok_oauth_key),
            grok_build_oauth_service::GROK_BUILD_RESPONSES_URL
        );

        let codex_endpoint = openai_endpoint("https://proxy.example/v1/responses");
        let codex_oauth_key = chatgpt_codex_api_key();
        assert_eq!(
            request_url_for_backend(&codex_endpoint, &codex_oauth_key),
            codex_oauth_service::CHATGPT_CODEX_RESPONSES_URL
        );

        assert_eq!(
            request_url_for_backend(&grok_endpoint, "xai-plain-api-key"),
            grok_endpoint.api_url
        );
        assert_eq!(
            request_url_for_backend(&codex_endpoint, &grok_oauth_key),
            codex_endpoint.api_url
        );
        assert_eq!(
            request_url_for_backend(&codex_endpoint, &oauth_wrapped_api_key()),
            codex_endpoint.api_url
        );
    }

    #[test]
    fn responses_body_uses_stream_without_forcing_reasoning() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: true,
                json_output: true,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );

        assert_eq!(body["stream"], serde_json::json!(true));
        assert_eq!(
            body["text"]["format"]["type"],
            serde_json::json!("json_object")
        );
        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn chat_body_keeps_provider_default_reasoning() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: true,
                json_output: false,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );

        assert_eq!(body["stream"], serde_json::json!(true));
        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn volcengine_chat_body_maps_reasoning_mode_to_thinking() {
        let endpoint = LlmEndpoint {
            provider: "custom".to_string(),
            api_url: "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string(),
            model: "doubao-seed-1-6-thinking".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::OpenaiCompat,
        };
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: false,
                json_output: false,
                reasoning_mode: LlmReasoningMode::Off,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );

        assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn openai_chat_body_maps_reasoning_mode_to_effort() {
        let mut endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        endpoint.model = "gpt-5-mini".to_string();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: false,
                json_output: false,
                reasoning_mode: LlmReasoningMode::Deep,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );

        assert_eq!(body["reasoning_effort"], serde_json::json!("high"));
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn parses_openai_compat_top_level_error_message() {
        let message = extract_openai_compat_error_message(
            r#"{"message":"We're experiencing high traffic right now! Please try again soon.","type":"too_many_requests_error","param":"queue","code":"queue_exceeded"}"#,
        );

        assert_eq!(
            message.as_deref(),
            Some(
                "We're experiencing high traffic right now! Please try again soon. (code: queue_exceeded, param: queue)"
            )
        );
    }

    #[test]
    fn api_error_message_falls_back_to_openai_compat_parser() {
        let endpoint = openai_endpoint("https://api.cerebras.ai/v1/chat/completions");

        let message = extract_api_error_message(
            &endpoint,
            r#"{"error":{"message":"model does not support image input","code":"invalid_value"}}"#,
        );

        assert_eq!(
            message,
            "model does not support image input (code: invalid_value)"
        );
    }

    #[test]
    fn recognizes_retryable_queue_exceeded_errors() {
        assert!(is_retryable_overload_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "We're experiencing high traffic right now! Please try again soon. (code: queue_exceeded, param: queue)"
        ));
        assert!(!is_retryable_overload_error(
            reqwest::StatusCode::BAD_REQUEST,
            "queue_exceeded"
        ));
    }

    #[test]
    fn stream_event_payload_omits_partial_text() {
        let payload = build_stream_event_payload(Some(7), Some("abc"), 3);

        assert_eq!(payload["status"], serde_json::json!("streaming"));
        assert_eq!(payload["chunk"], serde_json::json!("abc"));
        assert_eq!(payload["tokens"], serde_json::json!(3));
        assert_eq!(payload["sessionId"], serde_json::json!(7));
        assert!(payload.get("partialText").is_none());
    }

    #[test]
    fn responses_citation_parser_collects_unique_sources() {
        let response = serde_json::json!({
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "annotations": [
                        {"type": "url_citation", "title": "OpenAI", "url": "https://openai.com"},
                        {"type": "url_citation", "title": "Duplicate", "url": "https://openai.com"},
                        {"type": "url_citation", "title": "Docs", "url": "https://developers.openai.com"}
                    ]
                }]
            }]
        });
        let mut citations = Vec::new();

        collect_url_citation_payloads(&response, &mut citations);

        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0]["title"], serde_json::json!("OpenAI"));
        assert_eq!(
            citations[1]["url"],
            serde_json::json!("https://developers.openai.com")
        );
    }

    #[test]
    fn timeout_budget_accounts_for_images_and_tool_context() {
        let plain = dynamic_timeout(10, 0, &serde_json::json!({}), false);
        let image_body = serde_json::json!({
            "messages": [{
                "content": [{
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:image/jpeg;base64,{}", "a".repeat(600_000))
                    }
                }]
            }]
        });
        let with_context = dynamic_timeout(10, 0, &image_body, true);

        assert!(with_context > plain);
    }

    #[test]
    fn stream_error_payload_preserves_message_and_optional_session() {
        let with_session = build_stream_error_payload(Some(7), "provider failed");
        assert_eq!(with_session["status"], serde_json::json!("error"));
        assert_eq!(
            with_session["message"],
            serde_json::json!("provider failed")
        );
        assert_eq!(with_session["sessionId"], serde_json::json!(7));

        let without_session = build_stream_error_payload(None, "provider failed");
        assert_eq!(without_session["status"], serde_json::json!("error"));
        assert_eq!(
            without_session["message"],
            serde_json::json!("provider failed")
        );
        assert!(without_session.get("sessionId").is_none());
    }

    #[test]
    fn stream_budget_times_out_when_visible_progress_stalls() {
        let now = tokio::time::Instant::now();
        let started_at = now - Duration::from_secs(20);
        let last_progress_at = now - Duration::from_secs(11);

        let result = stream_read_budget_at(
            now,
            started_at,
            last_progress_at,
            Duration::from_secs(90),
            Duration::from_secs(300),
            Some(Duration::from_secs(10)),
        );

        let err = result.expect_err("stalled visible output must trip progress timeout");
        assert!(err.contains("流式输出停滞"));
    }

    #[test]
    fn stream_budget_uses_nearest_deadline() {
        let now = tokio::time::Instant::now();
        let started_at = now - Duration::from_secs(20);
        let last_progress_at = now - Duration::from_secs(7);

        let budget = stream_read_budget_at(
            now,
            started_at,
            last_progress_at,
            Duration::from_secs(90),
            Duration::from_secs(300),
            Some(Duration::from_secs(10)),
        )
        .expect("progress timeout still has 3s remaining");

        assert_eq!(budget, Duration::from_secs(3));
    }

    #[test]
    fn request_options_carry_per_request_stream_total_timeout() {
        let options = LlmRequestOptions {
            stream: true,
            stream_total_timeout_secs: Some(300),
            ..LlmRequestOptions::default()
        };

        assert_eq!(
            super::stream_total_timeout(options),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn stream_total_timeout_rejects_legacy_twenty_four_hour_budget() {
        let options = LlmRequestOptions {
            stream: true,
            stream_total_timeout_secs: Some(24 * 60 * 60),
            ..LlmRequestOptions::default()
        };

        assert!(
            super::stream_total_timeout(options) <= Duration::from_secs(600),
            "stream total timeout must be task-scoped, not the old 24h reader budget"
        );
    }

    #[test]
    fn responses_done_text_replaces_partial_delta_buffer() {
        let mut accumulated = "{\"polished\":\"半".to_string();

        let (should_emit, progressed) = apply_responses_done_text(
            &mut accumulated,
            "{\"polished\":\"半句话\",\"corrections\":[],\"key_terms\":[]}",
        );

        assert!(!should_emit);
        assert!(progressed);
        assert_eq!(
            accumulated,
            "{\"polished\":\"半句话\",\"corrections\":[],\"key_terms\":[]}"
        );
    }

    #[test]
    fn responses_done_empty_text_preserves_partial_delta_buffer() {
        let mut accumulated = "{\"polished\":\"半".to_string();

        let (should_emit, progressed) = apply_responses_done_text(&mut accumulated, "");

        assert!(!should_emit);
        assert!(!progressed);
        assert_eq!(accumulated, "{\"polished\":\"半");
    }

    #[test]
    fn responses_done_unrelated_text_preserves_existing_buffer() {
        let mut accumulated = "first-block".to_string();

        let (should_emit, progressed) = apply_responses_done_text(&mut accumulated, "second-block");

        assert!(!should_emit);
        assert!(!progressed);
        assert_eq!(accumulated, "first-block");
    }

    #[test]
    fn responses_extract_content_reads_output_text_parts() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let json = serde_json::json!({
            "output": [
                {
                    "type": "message",
                    "content": [
                        {"type": "reasoning_text", "text": "internal"},
                        {"type": "output_text", "text": "{\"polished\":\"ok\"}"}
                    ]
                }
            ]
        });

        let content = extract_content(&endpoint, &json);

        assert_eq!(content.as_deref(), Some("{\"polished\":\"ok\"}"));
    }

    #[test]
    fn openai_compat_bodies_omit_optional_output_limits_and_preserve_request_features() {
        for (provider, base_url, model) in [
            ("openai", "https://api.openai.com/v1", "gpt-5-mini"),
            (
                "custom-gateway",
                "https://gateway.example/v1",
                "future-model",
            ),
            (
                "custom-gateway",
                "https://gateway.example/api.openai.com/v1",
                "gpt-5-mini",
            ),
            ("cerebras", "https://api.cerebras.ai/v1", "gpt-oss-120b"),
            (
                "deepseek",
                "https://api.deepseek.com/v1",
                "deepseek-reasoner",
            ),
            ("deepseek", "https://api.deepseek.com/v1", "deepseek-flash"),
            (
                "openrouter",
                "https://openrouter.ai/api/v1",
                "~deepseek/deepseek-flash-latest",
            ),
        ] {
            for suffix in ["chat/completions", "responses"] {
                let endpoint = LlmEndpoint {
                    provider: provider.to_string(),
                    api_url: format!("{base_url}/{suffix}"),
                    model: model.to_string(),
                    timeout_secs: 10,
                    api_format: ApiFormat::OpenaiCompat,
                };
                let responses =
                    suffix == "responses" || (provider == "deepseek" && model == "deepseek-flash");
                let body = build_llm_body(
                    &endpoint,
                    "system",
                    &LlmUserInput::from("hello"),
                    LlmRequestOptions {
                        stream: true,
                        json_output: true,
                        web_search: true,
                        reasoning_mode: LlmReasoningMode::Balanced,
                        ..LlmRequestOptions::default()
                    },
                );

                for key in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
                    assert!(
                        body.get(key).is_none(),
                        "{provider}/{suffix} sets {key}: {body}"
                    );
                }
                assert_eq!(body["model"], model);
                assert_eq!(
                    body.get("stream").and_then(serde_json::Value::as_bool),
                    (provider != "cerebras" || responses).then_some(true),
                    "existing Cerebras JSON/stream compatibility must be preserved: {body}"
                );
                if responses {
                    assert_eq!(body["instructions"], "system");
                    assert_eq!(body["input"][1]["content"][0]["text"], "hello");
                    assert_eq!(body["text"]["format"]["type"], "json_object");
                    assert_eq!(body["tools"][0]["type"], "web_search");
                } else {
                    assert_eq!(body["messages"][0]["content"], "system");
                    assert_eq!(body["messages"][1]["content"], "hello");
                    assert_eq!(body["response_format"]["type"], "json_object");
                    assert_eq!(body["tools"][0]["type"], "web_search_preview");
                }
                if provider == "cerebras" {
                    assert_eq!(body["reasoning_effort"], "medium");
                } else if provider == "deepseek" && !responses {
                    assert_eq!(body["thinking"]["type"], "enabled");
                } else if responses || provider == "openrouter" {
                    assert_eq!(body["reasoning"]["effort"], "medium");
                } else {
                    assert_eq!(body["reasoning_effort"], "medium");
                }
            }
        }
    }

    #[test]
    fn native_anthropic_body_keeps_required_larger_output_budget_and_features() {
        let endpoint = LlmEndpoint {
            provider: "custom-anthropic".to_string(),
            api_url: "https://api.anthropic.com/v1/messages".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::Anthropic,
        };
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                stream: true,
                web_search: true,
                reasoning_mode: LlmReasoningMode::Deep,
                ..LlmRequestOptions::default()
            },
        );

        assert_eq!(body["max_tokens"], 16_384);
        assert!(body.get("max_completion_tokens").is_none());
        assert!(body.get("max_output_tokens").is_none());
        assert_eq!(body["system"][0]["text"], "system");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert_eq!(body["stream"], true);
        assert_eq!(body["tools"][0]["type"], "web_search_20250305");
    }

    #[test]
    fn chatgpt_backend_omits_max_output_tokens_before_first_request() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );
        let api_key = format!(
            "openai-codex-chatgpt:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"access_token":"test","account_id":"acc"}"#)
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert!(
            adapted.get("max_output_tokens").is_none(),
            "ChatGPT Codex backend rejects max_output_tokens; it must be removed before dispatch"
        );
        assert_eq!(adapted["store"], serde_json::json!(false));
    }

    #[test]
    fn chatgpt_backend_responses_json_output_forces_stream_transport() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: false,
                json_output: true,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );
        let api_key = chatgpt_codex_api_key();

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert_eq!(adapted["store"], serde_json::json!(false));
        assert_eq!(adapted["stream"], serde_json::json!(true));
    }

    #[test]
    fn chatgpt_backend_responses_gpt5_reasoning_off_forces_stream_transport() {
        let mut endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        endpoint.model = "gpt-5.1-mini".to_string();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: false,
                json_output: false,
                reasoning_mode: LlmReasoningMode::Off,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );
        let api_key = chatgpt_codex_api_key();

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert_eq!(adapted["store"], serde_json::json!(false));
        assert_eq!(adapted["reasoning"]["effort"], serde_json::json!("none"));
        assert_eq!(adapted["stream"], serde_json::json!(true));
    }

    #[test]
    fn chatgpt_backend_responses_gpt6_maps_public_reasoning_modes_without_chat_fields() {
        let mut endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = chatgpt_codex_api_key();
        let expected = [
            (LlmReasoningMode::Off, "low"),
            (LlmReasoningMode::Light, "medium"),
            (LlmReasoningMode::Balanced, "high"),
            (LlmReasoningMode::Deep, "xhigh"),
        ];

        for (model, mode, effort) in ["gpt-6-astra", "gpt-6-sol", "gpt-6.1-sol"]
            .into_iter()
            .flat_map(|model| expected.map(|(mode, effort)| (model, mode, effort)))
        {
            endpoint.model = model.to_string();
            let body = build_llm_body(
                &endpoint,
                "system",
                &LlmUserInput::from("hello"),
                LlmRequestOptions {
                    reasoning_mode: mode,
                    ..LlmRequestOptions::default()
                },
            );
            assert_eq!(body["reasoning"]["effort"], serde_json::json!(effort));
            assert!(body.get("temperature").is_none());
            assert!(body.get("top_p").is_none());

            let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);
            assert_eq!(adapted["reasoning"]["effort"], serde_json::json!(effort));
            assert_eq!(adapted["stream"], serde_json::json!(true));
            assert_eq!(adapted["store"], serde_json::json!(false));
            assert!(adapted.get("reasoning_effort").is_none());
            assert!(adapted.get("max_output_tokens").is_none());
            assert!(adapted.get("temperature").is_none());
            assert!(adapted.get("top_p").is_none());
        }

        let provider_default = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );
        assert!(provider_default.get("reasoning").is_none());
        assert!(provider_default.get("reasoning_effort").is_none());
        let adapted_default = adapt_body_for_backend(&endpoint, &api_key, &provider_default, false);
        assert_eq!(adapted_default["stream"], serde_json::json!(true));
        assert_eq!(adapted_default["store"], serde_json::json!(false));
        assert!(adapted_default.get("reasoning").is_none());
        assert!(adapted_default.get("reasoning_effort").is_none());
        assert!(adapted_default.get("max_output_tokens").is_none());
    }

    fn direct_chatgpt_auth() -> String {
        format!(
            "openai-chatgpt-plan:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"access_token":"direct-token","account_id":"oaiapp_test"}"#)
        )
    }

    #[test]
    fn chatgpt_plan_auth_uses_public_api_and_app_identity() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let auth = direct_chatgpt_auth();
        assert_eq!(request_url_for_backend(&endpoint, &auth), endpoint.api_url);
        let headers =
            crate::services::llm_provider::build_auth_headers(&ApiFormat::OpenaiCompat, &auth)
                .unwrap();
        assert_eq!(headers["authorization"], "Bearer direct-token");
        assert!(!headers.contains_key("chatgpt-account-id"));
        assert!(!headers.contains_key("originator"));
    }

    #[test]
    fn chatgpt_plan_request_obeys_preview_contract_and_preserves_input() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = serde_json::json!({
            "input": [{"role":"user","content":"hello"}], "instructions":"polish",
            "stream":false, "store":true, "temperature":0.2,
            "max_output_tokens":128, "previous_response_id":"old", "metadata":{},
            "reasoning":{"effort":"low"}
        });
        let adapted = adapt_body_for_backend(&endpoint, &direct_chatgpt_auth(), &body, false);
        assert_eq!(adapted["input"], body["input"]);
        assert_eq!(adapted["instructions"], "polish");
        assert_eq!(adapted["reasoning"]["effort"], "low");
        assert_eq!(adapted["stream"], true);
        assert_eq!(adapted["store"], false);
        for field in [
            "temperature",
            "max_output_tokens",
            "previous_response_id",
            "metadata",
        ] {
            assert!(adapted.get(field).is_none(), "{field}");
        }
        assert_eq!(body["stream"], false, "caller body must be immutable");
    }

    #[test]
    fn exhausted_subscription_is_not_retryable_traffic() {
        assert!(!is_retryable_overload_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "Rate limit reached (code: subscription_sharing_usage_limit_exceeded)"
        ));
    }

    #[test]
    fn chatgpt_backend_responses_gpt6_sol_off_uses_catalog_supported_low() {
        let mut endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        endpoint.model = "gpt-6-sol".to_string();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                reasoning_mode: LlmReasoningMode::Off,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &chatgpt_codex_api_key(), &body, false);
        assert_eq!(adapted["reasoning"]["effort"], serde_json::json!("low"));
        assert_eq!(adapted["stream"], serde_json::json!(true));
        assert_eq!(adapted["store"], serde_json::json!(false));
        assert!(adapted.get("reasoning_effort").is_none());
        assert!(adapted.get("temperature").is_none());
        assert!(adapted.get("top_p").is_none());
    }

    #[test]
    fn recognizes_output_token_limit_unsupported_errors() {
        assert!(looks_like_output_token_limit_unsupported_error(
            "Unknown parameter: max_output_tokens"
        ));
        assert!(looks_like_output_token_limit_unsupported_error(
            "max_output_tokens is not supported by this backend"
        ));
        assert!(looks_like_output_token_limit_unsupported_error(
            "max_completion_tokens is not recognized by this backend"
        ));
        assert!(!looks_like_output_token_limit_unsupported_error(
            "invalid max_tokens value"
        ));
        assert!(!looks_like_output_token_limit_unsupported_error(
            "max_tokens must be less than or equal to 8192"
        ));
        assert!(!looks_like_output_token_limit_unsupported_error(
            "request timed out after 30s"
        ));
    }

    #[test]
    fn cerebras_json_output_disables_stream_to_preserve_response_format() {
        let endpoint = LlmEndpoint {
            provider: "cerebras".to_string(),
            api_url: "https://api.cerebras.ai/v1/chat/completions".to_string(),
            model: "gpt-oss-120b".to_string(),
            timeout_secs: 5,
            api_format: ApiFormat::OpenaiCompat,
        };
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: true,
                json_output: true,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );

        assert_eq!(
            body["response_format"],
            serde_json::json!({"type": "json_object"})
        );
        assert!(!body
            .get("stream")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false));
    }

    #[test]
    fn cerebras_without_json_output_keeps_stream() {
        let endpoint = LlmEndpoint {
            provider: "cerebras".to_string(),
            api_url: "https://api.cerebras.ai/v1/chat/completions".to_string(),
            model: "gpt-oss-120b".to_string(),
            timeout_secs: 5,
            api_format: ApiFormat::OpenaiCompat,
        };
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                auth_context: None,
                stream: true,
                json_output: false,
                reasoning_mode: LlmReasoningMode::ProviderDefault,
                stream_event: None,
                session_id: None,
                web_search: false,
                openai_fast_mode: false,
                stream_progress_timeout_secs: None,
                stream_total_timeout_secs: None,
            },
        );

        assert!(body.get("response_format").is_none());
        assert_eq!(body["stream"], serde_json::json!(true));
    }

    #[test]
    fn chatgpt_backend_injects_service_tier_priority_when_fast_mode_enabled() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        assert_eq!(
            adapted["service_tier"],
            serde_json::json!("priority"),
            "Fast mode must inject service_tier=\"priority\" on the wire. \
             Source: openai/codex codex-rs/core/src/client.rs (Fast → \"priority\"). \
             If this fails, either the official CLI changed its mapping or a \
             regression landed locally — do NOT weaken this test without \
             re-verifying against the upstream source."
        );
    }

    #[test]
    fn service_tier_whitelist_matches_openai_responses_api_spec() {
        assert_eq!(
            OPENAI_RESPONSES_SERVICE_TIER_WHITELIST,
            &["auto", "default", "flex", "priority"],
            "Whitelist drifted. Reconfirm against the current OpenAI Responses \
             API spec before changing — this pin exists to prevent silent \
             widening (e.g. adding the product label \"fast\") that would \
             neutralize the other fast-mode assertions."
        );
        assert!(
            !OPENAI_RESPONSES_SERVICE_TIER_WHITELIST.contains(&"fast"),
            "\"fast\" is the product label, not a valid API value — it MUST \
             NOT appear in the whitelist under any circumstance."
        );
    }

    #[test]
    fn injected_service_tier_is_in_the_openai_responses_api_whitelist() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        let injected = adapted["service_tier"]
            .as_str()
            .expect("service_tier must be injected as a JSON string when fast mode is on");

        assert!(
            OPENAI_RESPONSES_SERVICE_TIER_WHITELIST.contains(&injected),
            "Injected service_tier={injected:?} is NOT a valid OpenAI Responses \
             API value. Accepted values (per the public API spec): {:?}. \
             Note in particular: \"fast\" is the user-facing product label, \
             not a valid wire value — the backend silently drops it.",
            OPENAI_RESPONSES_SERVICE_TIER_WHITELIST
        );
    }

    #[test]
    fn chatgpt_backend_omits_service_tier_when_fast_mode_disabled() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, false);

        assert!(
            adapted.get("service_tier").is_none(),
            "service_tier must NOT be present when fast mode is disabled; got {:?}",
            adapted.get("service_tier")
        );
    }

    #[test]
    fn plain_openai_api_key_never_gets_service_tier_even_if_fast_mode_true() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = "sk-test";
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, api_key, &body, true);

        assert!(
            adapted.get("service_tier").is_none(),
            "Plain OpenAI API keys must never receive service_tier=fast; that header/body flag is ChatGPT-OAuth only"
        );
    }

    #[test]
    fn wrapped_oauth_api_key_gets_service_tier_without_chatgpt_backend_fields() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let api_key = oauth_wrapped_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        assert_eq!(adapted["service_tier"], serde_json::json!("priority"));
        assert!(
            adapted.get("store").is_none(),
            "Wrapped OAuth API keys must stay on the normal OpenAI endpoint path"
        );
    }

    #[test]
    fn chat_completions_chatgpt_backend_also_gets_service_tier_when_enabled() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        let api_key = chatgpt_codex_api_key();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions {
                openai_fast_mode: true,
                ..LlmRequestOptions::default()
            },
        );

        let adapted = adapt_body_for_backend(&endpoint, &api_key, &body, true);

        assert_eq!(
            adapted["service_tier"],
            serde_json::json!("priority"),
            "Fast mode is request-body scoped and must apply to chat/completions \
             too when ChatGPT-auth is used. Wire value hard-coded here to catch \
             divergence from openai/codex upstream — see the module-level \
             comment above these tests for why this literal is not a shared const."
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_rejects_empty_string() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content(String::new(), &endpoint, "polish");

        assert!(
            result.is_err(),
            "empty content must produce Err; got {:?}",
            result
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_rejects_whitespace_only() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content("   \n\t".to_string(), &endpoint, "polish");

        assert!(
            result.is_err(),
            "whitespace-only content must produce Err; got {:?}",
            result
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_passes_real_text() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content("hello world".to_string(), &endpoint, "polish");

        assert_eq!(
            result.as_deref(),
            Ok("hello world"),
            "real text must pass through verbatim, NOT trimmed"
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_preserves_internal_whitespace() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content("  hi  ".to_string(), &endpoint, "polish");

        assert_eq!(
            result.as_deref(),
            Ok("  hi  "),
            "leading/trailing whitespace must be preserved when content has \
             non-whitespace characters — the function checks emptiness via \
             trim() but must NOT mutate the returned string"
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_error_message_includes_provider_and_model() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content(String::new(), &endpoint, "polish");

        let err = result.expect_err("empty content must produce Err");
        assert!(
            err.contains("test-provider"),
            "error message must mention provider {:?}; got {:?}",
            endpoint.provider,
            err
        );
        assert!(
            err.contains("test-model-x"),
            "error message must mention model {:?}; got {:?}",
            endpoint.model,
            err
        );
    }

    #[test]
    fn ensure_non_empty_llm_content_error_message_includes_source_label() {
        let endpoint = make_test_endpoint();
        let result = ensure_non_empty_llm_content("   ".to_string(), &endpoint, "ai-polish");

        let err = result.expect_err("whitespace-only content must produce Err");
        assert!(
            err.contains("ai-polish"),
            "error message must mention source label {:?} verbatim; got {:?}",
            "ai-polish",
            err
        );
    }

    #[test]
    fn is_empty_llm_response_error_matches_fresh_helper_output() {
        let endpoint = make_test_endpoint();
        let err = ensure_non_empty_llm_content(String::new(), &endpoint, "non_stream")
            .expect_err("empty content must produce Err");
        assert!(
            super::is_empty_llm_response_error(&err),
            "recognizer must accept the very error string the helper produces; got {:?}",
            err
        );
    }

    #[test]
    fn is_empty_llm_response_error_matches_each_documented_source() {
        let endpoint = make_test_endpoint();
        for source in [
            "non_stream",
            "openai_chat_sse_done",
            "openai_chat_sse_eos",
            "anthropic_sse_message_stop",
            "anthropic_sse_eos",
            "openai_responses_sse_completed",
            "openai_responses_sse_done",
            "openai_responses_sse_eos",
        ] {
            let err = ensure_non_empty_llm_content(String::new(), &endpoint, source)
                .expect_err("empty content must produce Err");
            assert!(
                super::is_empty_llm_response_error(&err),
                "recognizer missed source={:?}; err={:?}",
                source,
                err
            );
        }
    }

    #[test]
    fn is_empty_llm_response_error_rejects_unrelated_errors() {
        for unrelated in [
            "HTTP 500: internal server error",
            "流式读取失败: connection reset",
            "Anthropic 流式错误: rate_limit_exceeded",
            "Responses 流式错误: invalid_request",
            "API 返回错误 401: invalid api key",
            "响应解析失败: expected ident at column 5",
            "",
        ] {
            assert!(
                !super::is_empty_llm_response_error(unrelated),
                "recognizer must NOT match unrelated error; got match for {:?}",
                unrelated
            );
        }
    }

    #[test]
    fn is_empty_llm_response_error_prefix_constant_is_load_bearing() {
        assert_eq!(super::EMPTY_LLM_RESPONSE_ERROR_PREFIX, "LLM 响应为空（");
    }

    #[test]
    fn finalize_responses_sse_accumulated_returns_accumulated_when_non_empty() {
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            "hello".to_string(),
            Some("ignored".to_string()),
            &endpoint,
            "openai_responses_sse_done",
        );

        assert_eq!(
            result.as_deref(),
            Ok("hello"),
            "accumulated 非空时必须直接返回 accumulated 原值，忽略 fallback"
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_preserves_accumulated_whitespace() {
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            "  hi  ".to_string(),
            None,
            &endpoint,
            "openai_responses_sse_done",
        );

        assert_eq!(
            result.as_deref(),
            Ok("  hi  "),
            "accumulated 必须 verbatim 返回，不允许被 trim 改写"
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_falls_back_when_accumulated_empty() {
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            String::new(),
            Some("from_event".to_string()),
            &endpoint,
            "openai_responses_sse_done",
        );

        assert_eq!(
            result.as_deref(),
            Ok("from_event"),
            "accumulated 为空、fallback 非空时必须返回 fallback 原值"
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_ignores_empty_fallback() {
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            String::new(),
            Some(String::new()),
            &endpoint,
            "openai_responses_sse_done",
        );

        let err = result.expect_err("accumulated 与 fallback 都为空时必须 Err");
        assert!(
            super::is_empty_llm_response_error(&err),
            "错误必须能被 is_empty_llm_response_error 识别，否则 polish fallback 短路失效；got {:?}",
            err
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_ignores_none_fallback() {
        let endpoint = make_test_endpoint();
        let result = finalize_responses_sse_accumulated(
            String::new(),
            None,
            &endpoint,
            "openai_responses_sse_eos",
        );

        let err = result.expect_err("accumulated 空、fallback None 时必须 Err");
        assert!(
            super::is_empty_llm_response_error(&err),
            "错误必须能被 is_empty_llm_response_error 识别；got {:?}",
            err
        );
    }

    #[test]
    fn finalize_responses_sse_accumulated_error_carries_provider_model_and_source() {
        let endpoint = make_test_endpoint();
        let err = finalize_responses_sse_accumulated(
            String::new(),
            None,
            &endpoint,
            "openai_responses_sse_done",
        )
        .expect_err("空响应必须 Err");

        assert!(
            err.contains(&endpoint.provider),
            "错误必须包含 provider {:?}; got {:?}",
            endpoint.provider,
            err
        );
        assert!(
            err.contains(&endpoint.model),
            "错误必须包含 model {:?}; got {:?}",
            endpoint.model,
            err
        );
        assert!(
            err.contains("openai_responses_sse_done"),
            "错误必须包含 source 标签 verbatim，方便日志/grep 定位; got {:?}",
            err
        );
    }
}
