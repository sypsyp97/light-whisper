mod events;
mod protocol;
mod request;
mod stream;
mod transport;

pub use request::{build_llm_body, LlmImageInput, LlmRequestOptions, LlmUserInput};
#[allow(unused_imports)]
pub use stream::{read_anthropic_sse_stream, read_openai_responses_sse_stream, read_sse_stream};
pub use transport::send_llm_request;

pub(crate) use protocol::{ensure_non_empty_llm_content, is_empty_llm_response_error};

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
    fn generic_openai_compat_chat_body_sets_max_tokens() {
        let endpoint = make_test_endpoint();
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        assert_eq!(body["max_tokens"], serde_json::json!(4096));
    }

    #[test]
    fn custom_gateway_path_containing_openai_host_uses_generic_token_limit() {
        let endpoint = LlmEndpoint {
            provider: "custom-gateway".to_string(),
            api_url: "https://gateway.example/api.openai.com/v1/chat/completions".to_string(),
            model: "gpt-5.2".to_string(),
            timeout_secs: 10,
            api_format: ApiFormat::OpenaiCompat,
        };

        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        assert_eq!(body["max_tokens"], serde_json::json!(4096));
        assert!(
            body.get("max_completion_tokens").is_none(),
            "only the actual URL host should opt into official OpenAI chat token naming"
        );
    }

    #[test]
    fn openai_chat_body_sets_max_completion_tokens() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/chat/completions");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        assert_eq!(body["max_completion_tokens"], serde_json::json!(4096));
        assert!(
            body.get("max_tokens").is_none(),
            "OpenAI Chat Completions deprecates max_tokens and requires max_completion_tokens for current reasoning models"
        );
    }

    #[test]
    fn cerebras_chat_body_sets_max_completion_tokens() {
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
            LlmRequestOptions::default(),
        );

        assert_eq!(body["max_completion_tokens"], serde_json::json!(4096));
        assert!(
            body.get("max_tokens").is_none(),
            "Cerebras Chat Completions documents max_completion_tokens, not max_tokens"
        );
    }

    #[test]
    fn responses_body_sets_max_output_tokens() {
        let endpoint = openai_endpoint("https://api.openai.com/v1/responses");
        let body = build_llm_body(
            &endpoint,
            "system",
            &LlmUserInput::from("hello"),
            LlmRequestOptions::default(),
        );

        assert_eq!(body["max_output_tokens"], serde_json::json!(4096));
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
        endpoint.model = "gpt-6-astra".to_string();
        let api_key = chatgpt_codex_api_key();
        let expected = [
            (LlmReasoningMode::Off, "low"),
            (LlmReasoningMode::Light, "medium"),
            (LlmReasoningMode::Balanced, "high"),
            (LlmReasoningMode::Deep, "xhigh"),
        ];

        for (mode, effort) in expected {
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

        // json_object 优先于 stream：保留 response_format，放弃流式
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

    // --- Fast mode tests -------------------------------------------------
    //
    // Tests here deliberately do NOT import `OPENAI_FAST_MODE_SERVICE_TIER`.
    // The wire value is hard-coded as a string literal so that test and
    // implementation must agree *via* an independent, externally-anchored
    // ground truth — not via a shared constant that lets a typo slip through
    // both sides in lockstep (which was the pre-2026-04-20 tautological bug:
    // test and impl both said "fast", both went green, backend silently
    // ignored the field).
    //
    // Ground truth sources, anchored outside this repo:
    //   1. openai/codex `codex-rs/core/src/client.rs` main branch, function
    //      `build_responses_request` — maps `ServiceTier::Fast => "priority"`.
    //   2. OpenAI public request docs — `service_tier` accepts exactly
    //      `auto | default | flex | priority` (note: "fast" is NOT
    //      in this set; sending it is a silent no-op).
    //
    // Two independent assertions are used for this reason:
    //   - Literal-value test pins the current official wire value.
    //   - Whitelist test catches any future typo that still passes #1 (e.g.
    //      someone accidentally changes the impl to "priorty" or "urgent").

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

        // Hard-coded literal — NOT a re-export of OPENAI_FAST_MODE_SERVICE_TIER.
        // Changing the impl constant without updating this literal must fail.
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
        // Meta-guard: lock the whitelist itself against drift. If someone
        // "fixes" both impl + literal test by also adding "fast" to the
        // whitelist to silence the tautology detector, this pin fails first.
        // Canonical set per OpenAI Responses API public spec.
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
        // Independent guard: whatever the impl injects, it must be a value
        // the OpenAI Responses API will actually accept. "fast" famously
        // passes a naive string compare but is not in the whitelist — so
        // this test catches typos / stale labels that the literal-equality
        // test alone would miss if someone "fixed" both sides the same way.
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

    // --- ensure_non_empty_llm_content tests ------------------------------
    //
    // Contract:
    //   - Returns Err(...) when content.trim().is_empty()
    //   - Error message contains endpoint.provider, endpoint.model, and source
    //   - Otherwise returns Ok(content) — content is NOT trimmed

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

    // --- is_empty_llm_response_error tests --------------------------------
    //
    // 这条识别函数是 polish/assistant fallback 决定是否短路重试的依据。
    // 如果识别失稳（漏判 → 多花 3 次请求；误判 → 把无关错误当空响应丢弃），
    // 就退回到原始浪费/错失的状态。

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
        // 每个 callsite 都要被识别——任何一个漏掉就意味着那条路径上的空
        // 响应仍会触发 polish 4 段 fallback 全跑。
        let endpoint = make_test_endpoint();
        for source in [
            "non_stream",
            "openai_chat_sse_done",
            "openai_chat_sse_eos",
            "anthropic_sse_message_stop",
            "anthropic_sse_eos",
            "openai_responses_sse_completed",
            // 新增的 Responses SSE finalize 路径：done 事件结束流，
            // eos 是流意外终止后由 finalize 兜底。两条分支都必须被
            // recognizer 识别，否则 polish/assistant fallback 链路会
            // 误以为是真正的 transport 错误而连跑 4 段。
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
        // 未来加新错误时不应该误命中。这里也防止前缀被改成"空响应"等更通用
        // 的措辞导致与 anthropic / openai 自带的错误文案撞车。
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
        // EMPTY_LLM_RESPONSE_ERROR_PREFIX 是 helper 与 recognizer 共用的契约
        // 字符串。把它改了就要同步改两边——这条断言是个 wake-up，提醒读者
        // 这个常量不是装饰品。
        assert_eq!(super::EMPTY_LLM_RESPONSE_ERROR_PREFIX, "LLM 响应为空（");
    }

    // --- finalize_responses_sse_accumulated tests --------------------------
    //
    // OpenAI Responses SSE 在 stream 结束（done）或意外断流（eos）时，
    // 我们手里同时握有：
    //   - accumulated：从 delta 事件里逐段拼起来的正文
    //   - fallback_content：completed/done 事件里附带的最终 content（可选）
    // finalizer 的合同是：
    //   1) accumulated 优先（含义最权威，且包含逐 token 流出的内容），
    //      非空就直接返回，且**不** trim——和 ensure_non_empty_llm_content
    //      对齐：只校验 trim 后是否为空，但返回原始字符串以保留前后空白。
    //   2) accumulated 为空才退回到事件里挂的 fallback_content；同样要求非空。
    //   3) 两者皆空再走 ensure_non_empty_llm_content，借它统一拼出
    //      EMPTY_LLM_RESPONSE_ERROR_PREFIX 起头的错误，让 recognizer 能识别。

    #[test]
    fn finalize_responses_sse_accumulated_returns_accumulated_when_non_empty() {
        // accumulated 存在就赢——即使 fallback 也非空也要被忽略。否则会出现
        // 流式内容被 done 事件的截断 fallback 覆盖，丢字。
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
        // 与 ensure_non_empty_llm_content 行为一致：判空用 trim，但返回原字符串。
        // 流式拼接出来的前后空白可能是有意义的（例如续写场景）。
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
        // 部分 provider 在 delta 里只丢 reasoning，正文只在 completed/done
        // 事件里给一次。accumulated 为空时必须用 fallback 兜底。
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
        // fallback 是 Some 但内容为空，等价于没有 fallback。要走空响应错误，
        // 而不是返回 Ok("")——后者会让上游误以为模型给了空字符串答案。
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
        // 完全没有 fallback 也是常见场景（流被中断、只能用 eos 兜底）。
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
        // 错误信息要能在日志里直接定位是哪个 provider/model/调用点出问题的。
        // 这条断言把三段都钉死，避免有人把 source 标签替换成更"通用"的措辞
        // 而让 grep 失效。
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
