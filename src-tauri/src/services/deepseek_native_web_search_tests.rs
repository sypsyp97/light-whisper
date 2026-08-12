use super::llm_client::{build_llm_body, send_llm_request, LlmRequestOptions, LlmUserInput};
use super::llm_provider::{assistant_endpoint_for_config, endpoint_for_config, LlmEndpoint};
use crate::state::user_profile::{ApiFormat, CustomProvider, LlmProviderConfig, LlmReasoningMode};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug)]
struct CapturedHttpRequest {
    path: String,
    body: Value,
}

fn separate_assistant_config(
    provider: &str,
    model: &str,
    custom_providers: Vec<CustomProvider>,
) -> LlmProviderConfig {
    LlmProviderConfig {
        active: "cerebras".to_string(),
        custom_model: Some("gpt-oss-120b".to_string()),
        assistant_use_separate_model: true,
        assistant_provider: Some(provider.to_string()),
        assistant_model: Some(model.to_string()),
        custom_providers,
        ..LlmProviderConfig::default()
    }
}

async fn spawn_capture_server(
    initial_path: &str,
    response_body: Value,
) -> (String, tokio::task::JoinHandle<CapturedHttpRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("capture server should bind");
    let address = listener
        .local_addr()
        .expect("capture server should have a local address");
    let response_body = response_body.to_string();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener
            .accept()
            .await
            .expect("LLM request should reach capture server");
        let mut request = Vec::new();
        let (header_end, content_length) = loop {
            let mut chunk = [0_u8; 4096];
            let count = socket
                .read(&mut chunk)
                .await
                .expect("capture server should read request");
            assert!(count > 0, "client closed before sending a complete request");
            request.extend_from_slice(&chunk[..count]);

            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let header_text = String::from_utf8_lossy(&request[..header_end]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .expect("JSON request must carry content-length");
            if request.len() >= header_end + 4 + content_length {
                break (header_end, content_length);
            }
        };

        let request_line = String::from_utf8_lossy(&request[..header_end])
            .lines()
            .next()
            .expect("request line should exist")
            .to_string();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .expect("request path should exist")
            .to_string();
        let body_start = header_end + 4;
        let body = serde_json::from_slice(&request[body_start..body_start + content_length])
            .expect("wire body should be valid JSON");

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("capture server should write response");

        CapturedHttpRequest { path, body }
    });

    (format!("http://{address}{initial_path}"), handle)
}

fn chat_response(text: &str) -> Value {
    serde_json::json!({"choices": [{"message": {"content": text}}]})
}

fn responses_response(text: &str) -> Value {
    serde_json::json!({
        "output": [{
            "type": "message",
            "content": [{"type": "output_text", "text": text}]
        }]
    })
}

fn anthropic_response(text: &str) -> Value {
    serde_json::json!({"content": [{"type": "text", "text": text}]})
}

async fn dispatch_and_capture(
    endpoint: LlmEndpoint,
    initial_path: &str,
    response_body: Value,
    use_native_search: bool,
) -> (Result<String, String>, CapturedHttpRequest) {
    dispatch_and_capture_with_reasoning(
        endpoint,
        initial_path,
        response_body,
        use_native_search,
        LlmReasoningMode::ProviderDefault,
    )
    .await
}

async fn dispatch_and_capture_with_reasoning(
    mut endpoint: LlmEndpoint,
    initial_path: &str,
    response_body: Value,
    use_native_search: bool,
    reasoning_mode: LlmReasoningMode,
) -> (Result<String, String>, CapturedHttpRequest) {
    let (api_url, server) = spawn_capture_server(initial_path, response_body).await;
    endpoint.api_url = api_url;
    let options = LlmRequestOptions {
        stream: false,
        web_search: use_native_search,
        reasoning_mode,
        ..LlmRequestOptions::default()
    };
    let body = build_llm_body(&endpoint, "system", &LlmUserInput::from("request"), options);
    let client = reqwest::Client::new();

    let result =
        send_llm_request(&client, &endpoint, "test-api-key", &body, 7, None, options).await;
    let captured = server
        .await
        .expect("capture server task should finish normally");
    (result, captured)
}

#[tokio::test]
async fn deepseek_native_web_search_v4_assistant_dispatches_responses_wire_request() {
    for model in ["deepseek-v4-flash", "deepseek-v4-pro"] {
        let config = separate_assistant_config("deepseek", model, Vec::new());
        let polish_endpoint = endpoint_for_config(&config);
        let assistant_endpoint = assistant_endpoint_for_config(&config);

        assert_eq!(polish_endpoint.provider, "cerebras");
        assert_eq!(assistant_endpoint.provider, "deepseek");
        assert_eq!(assistant_endpoint.model, model);

        let (result, captured) = dispatch_and_capture(
            assistant_endpoint,
            "/v1/chat/completions",
            responses_response("searched answer"),
            true,
        )
        .await;

        assert_eq!(captured.path, "/v1/responses", "model={model}");
        assert_eq!(
            captured.body["tools"],
            serde_json::json!([{ "type": "web_search" }]),
            "model={model}"
        );
        assert!(captured.body.get("input").is_some(), "model={model}");
        assert!(captured.body.get("messages").is_none(), "model={model}");
        assert!(captured.body.get("thinking").is_none(), "model={model}");
        assert!(captured.body.get("reasoning").is_none(), "model={model}");
        assert_eq!(
            result.expect("Responses payload should parse"),
            "searched answer",
            "model={model}"
        );
    }
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_provider_default_uses_responses_wire_request() {
    for model in ["deepseek-v4-flash", "deepseek-v4-pro"] {
        let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
            "deepseek",
            model,
            Vec::new(),
        ));

        let (result, captured) = dispatch_and_capture_with_reasoning(
            endpoint,
            "/v1/chat/completions",
            responses_response("ordinary answer"),
            false,
            LlmReasoningMode::ProviderDefault,
        )
        .await;

        assert_eq!(captured.path, "/v1/responses", "model={model}");
        assert!(captured.body.get("tools").is_none(), "model={model}");
        assert!(captured.body.get("input").is_some(), "model={model}");
        assert!(captured.body.get("messages").is_none(), "model={model}");
        assert!(captured.body.get("thinking").is_none(), "model={model}");
        assert!(captured.body.get("reasoning").is_none(), "model={model}");
        assert_eq!(
            result.expect("Responses payload should parse"),
            "ordinary answer",
            "model={model}"
        );
    }
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_maps_explicit_modes_to_responses_effort() {
    for model in ["deepseek-v4-flash", "deepseek-v4-pro"] {
        for (reasoning_mode, expected_effort) in [
            (LlmReasoningMode::Light, "low"),
            (LlmReasoningMode::Balanced, "medium"),
            (LlmReasoningMode::Deep, "high"),
        ] {
            let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
                "deepseek",
                model,
                Vec::new(),
            ));
            let (result, captured) = dispatch_and_capture_with_reasoning(
                endpoint,
                "/v1/chat/completions",
                responses_response("reasoned answer"),
                false,
                reasoning_mode,
            )
            .await;

            assert_eq!(
                captured.path, "/v1/responses",
                "model={model}, mode={reasoning_mode:?}"
            );
            assert!(
                captured.body.get("input").is_some(),
                "model={model}, mode={reasoning_mode:?}"
            );
            assert!(
                captured.body.get("messages").is_none(),
                "model={model}, mode={reasoning_mode:?}"
            );
            assert_eq!(
                captured.body["reasoning"]["effort"], expected_effort,
                "model={model}, mode={reasoning_mode:?}"
            );
            assert!(
                captured.body.get("thinking").is_none(),
                "DeepSeek Responses ignores top-level thinking; model={model}, mode={reasoning_mode:?}"
            );
            assert_eq!(
                result.expect("Responses payload should parse"),
                "reasoned answer",
                "model={model}, mode={reasoning_mode:?}"
            );
        }
    }
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_off_uses_responses_and_disables_thinking() {
    for model in ["deepseek-v4-flash", "deepseek-v4-pro"] {
        let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
            "deepseek",
            model,
            Vec::new(),
        ));
        let (result, captured) = dispatch_and_capture_with_reasoning(
            endpoint,
            "/v1/chat/completions",
            responses_response("direct answer"),
            false,
            LlmReasoningMode::Off,
        )
        .await;

        assert_eq!(captured.path, "/v1/responses", "model={model}");
        assert!(captured.body.get("input").is_some(), "model={model}");
        assert!(captured.body.get("messages").is_none(), "model={model}");
        assert_eq!(
            captured.body["reasoning"]["effort"], "none",
            "model={model}"
        );
        assert!(captured.body.get("thinking").is_none(), "model={model}");
        assert_eq!(
            result.expect("Responses payload should parse"),
            "direct answer",
            "model={model}"
        );
    }
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_web_search_overrides_off_to_responses() {
    for model in ["deepseek-v4-flash", "deepseek-v4-pro"] {
        let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
            "deepseek",
            model,
            Vec::new(),
        ));
        let (result, captured) = dispatch_and_capture_with_reasoning(
            endpoint,
            "/v1/chat/completions",
            responses_response("searched answer"),
            true,
            LlmReasoningMode::Off,
        )
        .await;

        assert_eq!(captured.path, "/v1/responses", "model={model}");
        assert_eq!(
            captured.body["tools"],
            serde_json::json!([{ "type": "web_search" }]),
            "model={model}"
        );
        assert!(captured.body.get("input").is_some(), "model={model}");
        assert!(captured.body.get("messages").is_none(), "model={model}");
        assert!(captured.body.get("thinking").is_none(), "model={model}");
        assert_eq!(
            captured.body["reasoning"]["effort"], "none",
            "model={model}"
        );
        assert_eq!(
            result.expect("Responses payload should parse"),
            "searched answer",
            "model={model}"
        );
    }
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_keeps_legacy_models_on_chat_with_off_control() {
    for model in ["deepseek-chat", "deepseek-reasoner"] {
        let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
            "deepseek",
            model,
            Vec::new(),
        ));
        let (result, captured) = dispatch_and_capture_with_reasoning(
            endpoint,
            "/v1/chat/completions",
            chat_response("legacy answer"),
            false,
            LlmReasoningMode::Off,
        )
        .await;

        assert_eq!(captured.path, "/v1/chat/completions", "model={model}");
        assert!(captured.body.get("messages").is_some(), "model={model}");
        assert!(captured.body.get("input").is_none(), "model={model}");
        assert_eq!(
            captured.body["thinking"]["type"], "disabled",
            "model={model}"
        );
        assert!(captured.body.get("reasoning").is_none(), "model={model}");
        assert_eq!(
            result.expect("legacy Chat payload should parse"),
            "legacy answer",
            "model={model}"
        );
    }
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_keeps_non_target_providers_and_models_on_chat() {
    let cases = [
        ("deepseek", "deepseek-v4-flash-preview", Vec::new()),
        (
            "custom-deepseek-gateway",
            "deepseek-v4-flash",
            vec![CustomProvider {
                id: "custom-deepseek-gateway".to_string(),
                name: "Custom DeepSeek Gateway".to_string(),
                base_url: "https://gateway.example/deepseek".to_string(),
                model: "deepseek-v4-flash".to_string(),
                api_format: ApiFormat::OpenaiCompat,
            }],
        ),
        ("cerebras", "gpt-oss-120b", Vec::new()),
        ("siliconflow", "Qwen/Qwen3-32B", Vec::new()),
    ];

    for (provider, model, custom_providers) in cases {
        let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
            provider,
            model,
            custom_providers,
        ));
        let (result, captured) = dispatch_and_capture_with_reasoning(
            endpoint,
            "/v1/chat/completions",
            chat_response("chat answer"),
            false,
            LlmReasoningMode::Balanced,
        )
        .await;

        assert_eq!(
            captured.path, "/v1/chat/completions",
            "provider={provider}, model={model}"
        );
        assert!(
            captured.body.get("messages").is_some(),
            "provider={provider}, model={model}"
        );
        assert!(
            captured.body.get("input").is_none(),
            "provider={provider}, model={model}"
        );
        assert_eq!(
            result.expect("Chat payload should parse"),
            "chat answer",
            "provider={provider}, model={model}"
        );
    }
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_keeps_openai_on_responses() {
    let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
        "openai",
        "gpt-4.1-mini",
        Vec::new(),
    ));
    let (result, captured) = dispatch_and_capture_with_reasoning(
        endpoint,
        "/v1/responses",
        responses_response("openai answer"),
        false,
        LlmReasoningMode::ProviderDefault,
    )
    .await;

    assert_eq!(captured.path, "/v1/responses");
    assert!(captured.body.get("input").is_some());
    assert!(captured.body.get("messages").is_none());
    assert_eq!(
        result.expect("Responses payload should parse"),
        "openai answer"
    );
}

#[tokio::test]
async fn deepseek_native_web_search_requires_exact_v4_model() {
    for model in ["deepseek-v4-flash-preview", "deepseek-chat"] {
        let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
            "deepseek",
            model,
            Vec::new(),
        ));
        let (result, captured) = dispatch_and_capture(
            endpoint,
            "/v1/chat/completions",
            chat_response("chat answer"),
            true,
        )
        .await;

        assert_eq!(captured.path, "/v1/chat/completions", "model={model}");
        assert_eq!(
            captured.body["tools"],
            serde_json::json!([{
                "type": "web_search_preview",
                "web_search_preview": {}
            }]),
            "model={model}"
        );
        assert_eq!(result.expect("Chat payload should parse"), "chat answer");
    }
}

#[tokio::test]
async fn deepseek_native_web_search_does_not_route_custom_provider_by_model_name() {
    let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
        "custom-deepseek-gateway",
        "deepseek-v4-flash",
        vec![CustomProvider {
            id: "custom-deepseek-gateway".to_string(),
            name: "Custom DeepSeek Gateway".to_string(),
            base_url: "https://gateway.example/deepseek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            api_format: ApiFormat::OpenaiCompat,
        }],
    ));

    let (result, captured) = dispatch_and_capture(
        endpoint,
        "/v1/chat/completions",
        chat_response("custom answer"),
        true,
    )
    .await;

    assert_eq!(captured.path, "/v1/chat/completions");
    assert_eq!(
        captured.body["tools"],
        serde_json::json!([{
            "type": "web_search_preview",
            "web_search_preview": {}
        }])
    );
    assert_eq!(
        result.expect("custom Chat payload should parse"),
        "custom answer"
    );
}

#[tokio::test]
async fn deepseek_native_web_search_keeps_openai_responses_dispatch() {
    let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
        "openai",
        "gpt-4.1-mini",
        Vec::new(),
    ));

    let (result, captured) = dispatch_and_capture(
        endpoint,
        "/v1/responses",
        responses_response("openai answer"),
        true,
    )
    .await;

    assert_eq!(captured.path, "/v1/responses");
    assert_eq!(
        captured.body["tools"],
        serde_json::json!([{ "type": "web_search" }])
    );
    assert_eq!(
        result.expect("Responses payload should parse"),
        "openai answer"
    );
}

#[tokio::test]
async fn deepseek_native_web_search_keeps_custom_anthropic_dispatch() {
    let endpoint = assistant_endpoint_for_config(&separate_assistant_config(
        "custom-anthropic",
        "claude-sonnet-4-5",
        vec![CustomProvider {
            id: "custom-anthropic".to_string(),
            name: "Custom Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            api_format: ApiFormat::Anthropic,
        }],
    ));

    let (result, captured) = dispatch_and_capture(
        endpoint,
        "/v1/messages",
        anthropic_response("anthropic answer"),
        true,
    )
    .await;

    assert_eq!(captured.path, "/v1/messages");
    assert_eq!(
        captured.body["tools"],
        serde_json::json!([{
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 3
        }])
    );
    assert_eq!(
        result.expect("Anthropic payload should parse"),
        "anthropic answer"
    );
}

#[tokio::test]
async fn deepseek_v4_reasoning_route_off_keeps_deepseek_polish_on_responses() {
    for model in ["deepseek-v4-flash", "deepseek-v4-pro"] {
        let config = LlmProviderConfig {
            active: "deepseek".to_string(),
            custom_model: Some(model.to_string()),
            ..LlmProviderConfig::default()
        };
        let endpoint = endpoint_for_config(&config);

        let (result, captured) = dispatch_and_capture_with_reasoning(
            endpoint,
            "/v1/chat/completions",
            responses_response("polished"),
            false,
            LlmReasoningMode::Off,
        )
        .await;

        assert_eq!(captured.path, "/v1/responses", "model={model}");
        assert!(captured.body.get("tools").is_none(), "model={model}");
        assert!(captured.body.get("input").is_some(), "model={model}");
        assert!(captured.body.get("messages").is_none(), "model={model}");
        assert_eq!(
            captured.body["reasoning"]["effort"], "none",
            "model={model}"
        );
        assert_eq!(
            result.expect("polish Responses payload should parse"),
            "polished",
            "model={model}"
        );
    }
}
