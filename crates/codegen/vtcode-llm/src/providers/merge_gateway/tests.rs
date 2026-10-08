use super::*;
use crate::provider::{LLMProvider, LLMRequest, Message, NormalizedStreamEvent, ToolCall, ToolChoice};
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::models;
use vtcode_utility_tool_specs::{apply_patch_parameters, write_stdin_parameters};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_provider(base_url: &str) -> MergeGatewayProvider {
    MergeGatewayProvider::new_with_client(
        "test-key".to_string(),
        models::merge_gateway::DEFAULT_ROUTING.to_string(),
        HttpClient::new(),
        base_url.to_string(),
        TimeoutsConfig::default(),
    )
}

fn sse(event: &str, data: Value) -> String {
    format!("event: {event}\ndata: {}\n\n", serde_json::to_string(&data).expect("event payload"))
}

fn sse_data(data: Value) -> String {
    format!("data: {}\n\n", serde_json::to_string(&data).expect("event payload"))
}

#[test]
fn non_streaming_capability_is_pinned_for_stream_timeout_fallback() {
    // The harness's stream-timeout retry falls back to non-streaming only
    // when this capability is advertised; losing it silently re-streams
    // every retry into the first-token timeout watchdog.
    let provider = test_provider("http://127.0.0.1:1");
    for model in models::merge_gateway::SUPPORTED_MODELS {
        assert!(
            LLMProvider::supports_non_streaming(&provider, model),
            "route {model} must advertise non-streaming fallback capability"
        );
    }
}

#[test]
fn native_payload_maps_openai_service_tiers_and_omits_ultrafast() {
    // Merge Gateway only accepts `standard`/`flex`/`priority`; OpenAI's
    // `ultrafast` has no equivalent and must be omitted (gateway default
    // routing) rather than 422ing on `literal_error`.
    assert_eq!(map_openai_service_tier_for_merge("flex"), Some("flex"));
    assert_eq!(map_openai_service_tier_for_merge("priority"), Some("priority"));
    assert_eq!(map_openai_service_tier_for_merge("standard"), Some("standard"));
    assert_eq!(map_openai_service_tier_for_merge("Flex"), Some("flex"));
    assert_eq!(map_openai_service_tier_for_merge("ultrafast"), None);

    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::OPENAI_GPT_6_1_SOL.to_string());
    let mut ultrafast = LLMRequest {
        messages: vec![Message::user("hello".to_string())].into(),
        model: models::merge_gateway::OPENAI_GPT_6_1_SOL.to_string(),
        service_tier: Some("ultrafast".to_string()),
        ..Default::default()
    };
    let payload = provider.build_native_payload(&ultrafast, false).expect("payload");
    assert!(payload.get("service_tier").is_none(), "ultrafast must be omitted for Merge Gateway routes");

    ultrafast.service_tier = Some("priority".to_string());
    let payload = provider.build_native_payload(&ultrafast, false).expect("payload");
    assert_eq!(payload.get("service_tier").and_then(Value::as_str), Some("priority"));
}

#[test]
fn tier_pricing_rejection_matches_unpriced_route_body() {
    // Merge fail-closed 400 for valid-but-unpriced tiers (service-tiers docs).
    let body = r#"{"error":{"type":"invalid_request_error","message":"Model 'openai/gpt-6.1-sol' does not support service tier 'flex'.","source":"gateway"}}"#;
    assert!(is_merge_tier_pricing_rejection(StatusCode::BAD_REQUEST, body));
    assert!(is_merge_tier_pricing_rejection(
        StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"Model 'openai/gpt-6-sol' does not support service tier 'priority'.","source":"gateway"}}"#
    ));
    // Disjoint from capability routing: capability bodies never name the tier field.
    assert!(!is_merge_tier_pricing_rejection(
        StatusCode::BAD_REQUEST,
        "has no vendor that supports the requested capabilities (['tools'])"
    ));
    assert!(!is_merge_tier_pricing_rejection(StatusCode::UNPROCESSABLE_ENTITY, body));
    assert!(!is_merge_tier_pricing_rejection(StatusCode::BAD_REQUEST, ""));
}

#[tokio::test]
async fn native_generate_retries_without_tier_on_pricing_rejection() {
    use std::sync::Mutex;

    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_mock = Arc::clone(&seen);

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            let payload: Value = serde_json::from_slice(&req.body).expect("valid json body");
            let tier = payload.get("service_tier").and_then(Value::as_str).map(ToOwned::to_owned);
            seen_for_mock.lock().expect("mutex not poisoned").push(tier.clone());
            match tier.as_deref() {
                Some("flex") => ResponseTemplate::new(400).set_body_json(json!({
                    "error": {
                        "type": "invalid_request_error",
                        "message": "Model 'openai/gpt-6.1-sol' does not support service tier 'flex'.",
                        "source": "gateway"
                    }
                })),
                None => ResponseTemplate::new(200).set_body_json(json!({
                    "id": "resp_retry",
                    "object": "response",
                    "created_at": "2026-03-23T12:03:00Z",
                    "model": "openai/gpt-6.1-sol",
                    "output": [{
                        "type": "message",
                        "id": "msg_1",
                        "role": "assistant",
                        "content": [{"type": "text", "text": "Hello"}],
                        "finish_reason": "stop"
                    }],
                    "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15, "cost": 0.01}
                })),
                other => ResponseTemplate::new(500).set_body_string(format!("unexpected tier: {other:?}")),
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let response = provider
        .generate(LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: models::merge_gateway::OPENAI_GPT_6_1_SOL.to_string(),
            service_tier: Some("flex".to_string()),
            ..Default::default()
        })
        .await
        .expect("pricing retry without tier should succeed");
    assert_eq!(response.content.as_deref(), Some("Hello"));
    assert_eq!(seen.lock().expect("mutex not poisoned").as_slice(), &[Some("flex".to_string()), None]);
}

#[test]
fn capability_unavailable_detection_matches_gateway_body() {
    let body = r#"{"error":{"type":"invalid_request_error","message":"Model 'openai/gpt-6.1-sol' has no vendor that supports the requested capabilities (['streaming_tools', 'tools']).","source":"gateway","code":"capability_unavailable","param":"model"}}"#;
    assert!(is_capability_unavailable(StatusCode::BAD_REQUEST, body));
    assert!(is_capability_unavailable(StatusCode::UNPROCESSABLE_ENTITY, body));
    assert!(!is_capability_unavailable(StatusCode::BAD_REQUEST, r#"{"error":{"code":"invalid_parameter"}}"#));
    assert!(!is_capability_unavailable(StatusCode::INTERNAL_SERVER_ERROR, body));
    assert!(!is_capability_unavailable(StatusCode::BAD_REQUEST, ""));
}

#[test]
fn reasoning_capability_rejection_attribution_matches_gateway_body() {
    let reasoning_body = r#"{"error":{"type":"invalid_request_error","message":"Model 'xiaomimimo/mimo-v2.6-flash' has no vendor that supports the requested capabilities (['reasoning', 'tools']).","source":"gateway","code":"capability_unavailable","param":"model"}}"#;
    assert!(is_reasoning_capability_rejection(reasoning_body));
    let tools_body = r#"{"error":{"type":"invalid_request_error","message":"Model 'openai/gpt-6.1-sol' has no vendor that supports the requested capabilities (['streaming_tools', 'tools']).","source":"gateway","code":"capability_unavailable","param":"model"}}"#;
    assert!(!is_reasoning_capability_rejection(tools_body));
    assert!(!is_reasoning_capability_rejection(r#"{"error":{"code":"invalid_parameter"}}"#));
    assert!(!is_reasoning_capability_rejection(""));
}

#[test]
fn native_payload_omits_reasoning_for_haiku_5_5_route() {
    // Like the `xiaomimimo/` routes, the gateway has no vendor serving
    // reasoning jointly with tools for `anthropic/claude-haiku-5-5`
    // (`capability_unavailable` for `['reasoning', 'tools']`), so the route
    // stays unclassified and reasoning controls are omitted.
    let provider = test_provider("http://127.0.0.1:1");
    let model = models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5;
    assert!(!provider.supports_reasoning(model), "{model} must not advertise reasoning");
    assert!(!provider.supports_reasoning_effort(model), "{model} must not advertise reasoning effort");
    let payload = provider
        .build_native_payload(
            &LLMRequest {
                messages: vec![Message::user("hello".to_string())].into(),
                model: model.to_string(),
                reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
                max_tokens: Some(4096),
                ..Default::default()
            },
            false,
        )
        .expect("payload builds");
    assert!(payload.get("reasoning_effort").is_none(), "{model} must not forward reasoning_effort");
    assert!(payload.get("thinking").is_none(), "{model} must not forward thinking");
}

#[test]
fn haiku_routes_advertise_vision_matching_catalog() {
    // Both gateway Haiku entries declare image input in `docs/models.json`;
    // the provider must agree so image payloads are not dropped client-side.
    let provider = test_provider("http://127.0.0.1:1");
    for model in [
        models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_4_5_20251001,
        models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5,
    ] {
        assert!(provider.supports_vision(model), "{model} must advertise vision");
    }
}

#[test]
fn native_payload_omits_reasoning_for_xiaomimimo_routes() {
    // The gateway has no vendor serving reasoning jointly with tools for
    // `xiaomimimo/` routes: forwarding `thinking` turns every agentic request
    // into a `capability_unavailable` rejection, so the routes stay
    // unclassified and reasoning controls are omitted.
    let provider = test_provider("http://127.0.0.1:1");
    for model in [
        models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_PRO,
        models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_FLASH,
    ] {
        assert!(!provider.supports_reasoning(model), "{model} must not advertise reasoning");
        assert!(!provider.supports_reasoning_effort(model), "{model} must not advertise reasoning effort");
        let payload = provider
            .build_native_payload(
                &LLMRequest {
                    messages: vec![Message::user("hello".to_string())].into(),
                    model: model.to_string(),
                    reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
                    max_tokens: Some(4096),
                    ..Default::default()
                },
                false,
            )
            .expect("payload builds");
        assert!(payload.get("reasoning_effort").is_none(), "{model} must not forward reasoning_effort");
        assert!(payload.get("thinking").is_none(), "{model} must not forward thinking");
    }
}

#[tokio::test]
async fn reasoning_capability_rejection_does_not_poison_tool_vendor_cache() {
    use std::sync::Mutex;

    // A rejection naming `reasoning` (e.g. `(['reasoning', 'tools'])`)
    // blames the combination, not tools alone: later turns must re-probe the
    // network instead of failing fast on a cached no-tool-vendor verdict.
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());
    let seen_thinking = Arc::new(Mutex::new(Vec::new()));
    let seen_for_mock = Arc::clone(&seen_thinking);

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            let payload: Value = serde_json::from_slice(&req.body).expect("valid json body");
            seen_for_mock.lock().expect("mutex not poisoned").push(payload.get("thinking").is_some());
            ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "type": "invalid_request_error",
                    "message": "Model 'deepseek/deepseek-v4.1-flash' has no vendor that supports the requested capabilities (['reasoning', 'tools']).",
                    "source": "gateway",
                    "code": "capability_unavailable",
                    "param": "model"
                }
            }))
        })
        .expect(2)
        .mount(&server)
        .await;

    let tool_request = || LLMRequest {
        messages: vec![Message::user("hello".to_string())].into(),
        model: models::merge_gateway::DEEPSEEK_FLASH.to_string(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather".to_string(),
            json!({"type": "object", "properties": {}}),
        )])),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::Medium),
        ..Default::default()
    };
    let first = provider
        .generate(tool_request())
        .await
        .expect_err("reasoning+tools rejection must fail");
    assert!(first.to_string().contains("capability"), "got: {first}");
    assert!(!first.to_string().contains("cached"), "first failure must not claim a cached verdict, got: {first}");

    let second = provider
        .generate(tool_request())
        .await
        .expect_err("second turn must re-probe, not fail fast");
    assert!(
        !second.to_string().contains("cached"),
        "reasoning-caused rejection must not poison the tool-vendor cache, got: {second}"
    );

    // Both turns sent the thinking block that triggered the joint rejection.
    assert_eq!(seen_thinking.lock().expect("mutex not poisoned").as_slice(), &[true, true]);
}

#[tokio::test]
async fn native_stream_retries_without_streaming_on_capability_unavailable() {
    use std::sync::Mutex;

    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_mock = Arc::clone(&seen);

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            let payload: Value = serde_json::from_slice(&req.body).expect("valid json body");
            let streaming = payload.get("stream").and_then(Value::as_bool).unwrap_or(false);
            let has_tools = payload.get("tools").is_some();
            seen_for_mock.lock().expect("mutex not poisoned").push((streaming, has_tools));
            if streaming {
                ResponseTemplate::new(400).set_body_json(json!({
                    "error": {
                        "type": "invalid_request_error",
                        "message": "Model 'openai/gpt-6.1-sol' has no vendor that supports the requested capabilities (['streaming_tools', 'tools']).",
                        "source": "gateway",
                        "code": "capability_unavailable",
                        "param": "model"
                    }
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "resp_retry",
                    "object": "response",
                    "created_at": "2026-03-23T12:03:00Z",
                    "model": "openai/gpt-6.1-sol",
                    "output": [{
                        "type": "message",
                        "id": "msg_1",
                        "role": "assistant",
                        "content": [{"type": "text", "text": "Hello"}],
                        "finish_reason": "stop"
                    }],
                    "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15, "cost": 0.01}
                }))
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let request = LLMRequest {
        messages: vec![Message::user("hello".to_string())].into(),
        model: models::merge_gateway::OPENAI_GPT_6_1_SOL.to_string(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather".to_string(),
            json!({"type": "object", "properties": {}}),
        )])),
        ..Default::default()
    };
    let stream = provider.stream(request).await.expect("stream with fallback");
    let events = stream.collect::<Vec<_>>().await;
    let completed = events
        .into_iter()
        .find_map(|event| match event.expect("stream event") {
            LLMStreamEvent::Completed { response } => Some(response),
            _ => None,
        })
        .expect("fallback stream must complete");
    assert_eq!(completed.content.as_deref(), Some("Hello"));

    // First attempt streams with tools; the retry keeps tools but drops streaming.
    assert_eq!(seen.lock().expect("mutex not poisoned").as_slice(), &[(true, true), (false, true)]);
}

#[tokio::test]
async fn repeated_tool_turns_fail_fast_after_first_capability_rejection() {
    use std::sync::Mutex;

    // The first turn burns one call proving the route has no tool vendor;
    // later turns must fail fast without touching the network.
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "type": "invalid_request_error",
                "message": "Model 'openai/gpt-6.1-sol' has no vendor that supports the requested capabilities (['tools']).",
                "source": "gateway",
                "code": "capability_unavailable",
                "param": "model"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let tool_request = || LLMRequest {
        messages: vec![Message::user("hello".to_string())].into(),
        model: models::merge_gateway::OPENAI_GPT_6_1_SOL.to_string(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather".to_string(),
            json!({"type": "object", "properties": {}}),
        )])),
        ..Default::default()
    };
    let first = provider.generate(tool_request()).await.expect_err("no tool vendor must fail");
    assert!(first.to_string().contains("default_routing"));

    let second = provider.generate(tool_request()).await.expect_err("second turn must fail fast");
    assert!(second.to_string().contains("cached"), "fast-fail must say the verdict is cached, got: {second}");
}

#[tokio::test]
async fn terminal_capability_error_names_a_working_route() {
    // When even the non-streaming retry finds no tool vendor, the surfaced
    // error must tell the user the route (not the request) is at fault.
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "type": "invalid_request_error",
                "message": "Model 'openai/gpt-6.1-sol' has no vendor that supports the requested capabilities (['tools']).",
                "source": "gateway",
                "code": "capability_unavailable",
                "param": "model"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let err = provider
        .generate(LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: models::merge_gateway::OPENAI_GPT_6_1_SOL.to_string(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get weather".to_string(),
                json!({"type": "object", "properties": {}}),
            )])),
            ..Default::default()
        })
        .await
        .expect_err("route with no tool vendor must fail closed");
    let text = err.to_string();
    assert!(text.contains("capability"), "error must preserve the gateway diagnostic, got: {text}");
    assert!(text.contains("default_routing"), "error must name a working route, got: {text}");
}

#[test]
fn native_payload_serializes_message_tool_use_and_tool_result_items() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let request = LLMRequest {
        system_prompt: Some(Arc::from("You are helpful")),
        messages: vec![
            Message::user("hello".to_string()),
            Message::assistant_with_tools(
                "calling tool".to_string(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "get_weather".to_string(),
                    r#"{"location":"Paris"}"#.to_string(),
                )],
            ),
            Message::tool_response("call_1".to_string(), "sunny".to_string()),
        ]
        .into(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        )])),
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        max_tokens: Some(128),
        temperature: Some(0.2),
        top_p: Some(0.9),
        stop_sequences: Some(vec!["END".to_string()]),
        tool_choice: Some(ToolChoice::Auto),
        output_format: Some(json!({
            "type": "json_schema",
            "json_schema": {
                "name": "weather",
                "schema": {"type": "object"}
            }
        })),
        service_tier: Some("flex".to_string()),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");
    let input = payload["input"].as_array().expect("input array");

    assert_eq!(payload["model"], models::merge_gateway::DEFAULT_ROUTING);
    assert_eq!(payload["max_tokens"], 128);
    assert!((payload["temperature"].as_f64().expect("temperature should be numeric") - 0.2).abs() < 1e-6);
    assert!((payload["top_p"].as_f64().expect("top_p should be numeric") - 0.9).abs() < 1e-6);
    assert_eq!(payload["stop"], json!(["END"]));
    assert_eq!(payload["tool_choice"], json!("auto"));
    assert_eq!(payload["response_format"]["type"], "json_schema");
    assert_eq!(payload["service_tier"], "flex");
    assert_eq!(payload["tools"][0]["type"], "function");
    assert_eq!(payload["tools"][0]["name"], "get_weather");
    assert_eq!(payload["tools"][0]["parameters"]["required"], json!(["location"]));
    assert_eq!(input.len(), 4);
    assert_eq!(input[0]["type"], "message");
    assert_eq!(input[0]["role"], "system");
    assert_eq!(input[0]["content"], "You are helpful");
    assert_eq!(input[1]["type"], "message");
    assert_eq!(input[1]["role"], "user");
    assert_eq!(input[1]["content"], "hello");
    assert_eq!(input[2]["type"], "message");
    assert_eq!(input[2]["role"], "assistant");
    assert!(input[2]["content"].is_array());
    assert_eq!(input[2]["content"][0]["type"], "text");
    assert_eq!(input[2]["content"][1]["type"], "tool_use");
    assert_eq!(input[2]["content"][1]["id"], "call_1");
    assert_eq!(input[2]["content"][1]["name"], "get_weather");
    assert_eq!(input[2]["content"][1]["input"], json!({"location": "Paris"}));
    assert_eq!(input[3]["type"], "tool_result");
    assert_eq!(input[3]["tool_use_id"], "call_1");
    assert_eq!(input[3]["content"], "sunny");
}

#[test]
fn native_payload_keeps_wire_prefix_byte_stable_across_grown_history() {
    // Companion to the OpenAI `grown_history_keeps_wire_prefix_byte_stable`
    // guard: Merge Gateway is the default provider, so its native payload
    // must also render grown history as a pure extension. The system
    // message stays at input[0] and earlier items must be byte-identical.
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let history = vec![
        Message::user("list files".to_string()),
        Message::assistant_with_tools(
            "checking".to_string(),
            vec![ToolCall::function(
                "call_1".to_string(),
                "get_weather".to_string(),
                r#"{"location":"Paris"}"#.to_string(),
            )],
        ),
        Message::tool_response("call_1".to_string(), "sunny".to_string()),
    ];
    let tools = Some(Arc::new(vec![ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather".to_string(),
        json!({
            "type": "object",
            "properties": {"location": {"type": "string"}},
            "required": ["location"]
        }),
    )]));
    let first = LLMRequest {
        system_prompt: Some(Arc::from("You are helpful")),
        messages: history.clone().into(),
        tools: tools.clone(),
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        ..Default::default()
    };
    let mut grown = history;
    grown.push(Message::assistant_with_tools(
        "reading".to_string(),
        vec![ToolCall::function(
            "call_2".to_string(),
            "get_weather".to_string(),
            r#"{"location":"Nice"}"#.to_string(),
        )],
    ));
    grown.push(Message::tool_response("call_2".to_string(), "rainy".to_string()));
    let second = LLMRequest {
        system_prompt: Some(Arc::from("You are helpful")),
        messages: grown.into(),
        tools,
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        ..Default::default()
    };

    let a = provider.build_native_payload(&first, false).expect("payload");
    let b = provider.build_native_payload(&second, false).expect("payload");
    assert_eq!(a["tools"], b["tools"], "tools must not be rewritten by appended history");
    let input_a = a["input"].as_array().expect("input array");
    let input_b = b["input"].as_array().expect("input array");
    assert!(input_b.len() > input_a.len(), "grown history must extend input");
    assert_eq!(&input_b[..input_a.len()], &input_a[..], "input prefix must be append-stable across turns");
}

#[test]
fn native_payload_omits_tool_choice_none_but_keeps_tools_for_cache() {
    for model in models::merge_gateway::SUPPORTED_MODELS {
        let provider = MergeGatewayProvider::with_model("test-key".to_string(), (*model).to_string());
        let mut request = LLMRequest {
            model: (*model).to_string(),
            messages: vec![Message::user("Summarize the conversation.".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "read_file".to_string(),
                "Read a file".to_string(),
                json!({"type": "object"}),
            )])),
            tool_choice: Some(ToolChoice::None),
            ..Default::default()
        };

        let payload = provider.build_native_payload(&request, false).expect("payload");
        // Definitions stay on the wire so recovery turns can reuse the
        // same cached prefix as tool-enabled turns.
        assert!(payload.get("tools").is_some(), "tools must stay on the wire for route {model}");
        assert!(payload.get("tool_choice").is_none(), "tool_choice=none must not be sent for route {model}");

        // The no-tool normalization must not suppress an explicit
        // tool-enabled request on any route.
        request.tool_choice = Some(ToolChoice::Auto);
        let enabled_payload = provider.build_native_payload(&request, false).expect("enabled payload");
        assert!(enabled_payload.get("tools").is_some(), "tools must be preserved for route {model}");
        assert_eq!(enabled_payload["tool_choice"], json!("auto"));
    }
}

#[test]
fn legacy_payload_omits_tool_choice_none_but_keeps_tools_for_cache() {
    let provider = MergeGatewayProvider::from_config(
        Some("test-key".to_string()),
        Some(models::merge_gateway::DEFAULT_ROUTING.to_string()),
        Some("https://example.test/v1/openai".to_string()),
        None,
        None,
        None,
        None,
    );
    let core = provider.legacy_core.as_ref().expect("legacy core");

    for model in models::merge_gateway::SUPPORTED_MODELS {
        let mut request = LLMRequest {
            model: (*model).to_string(),
            messages: vec![Message::user("Summarize the conversation.".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "read_file".to_string(),
                "Read a file".to_string(),
                json!({"type": "object"}),
            )])),
            tool_choice: Some(ToolChoice::None),
            ..Default::default()
        };

        let payload = core.convert_request(&request).expect("legacy payload");
        assert!(payload.get("tools").is_some(), "tools must stay on the wire for route {model}");
        assert!(payload.get("tool_choice").is_none(), "tool_choice=none must not be sent for route {model}");

        request.tool_choice = Some(ToolChoice::Auto);
        let enabled_payload = core.convert_request(&request).expect("enabled legacy payload");
        assert!(enabled_payload.get("tools").is_some(), "tools must be preserved for route {model}");
        assert_eq!(enabled_payload["tool_choice"], json!("auto"));
    }
}

#[test]
fn native_payload_keeps_tools_for_tool_choice_none_but_omits_them_without_tool_vendor() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        messages: vec![Message::user("Summarize.".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read a file".to_string(),
            json!({"type": "object"}),
        )])),
        tool_choice: Some(ToolChoice::None),
        ..Default::default()
    };

    // Default: recovery keeps tools for cache stability.
    let payload = provider.build_native_payload(&request, false).expect("payload");
    assert!(payload.get("tools").is_some());

    // Once the route is known to lack a tool vendor, recovery omits tools
    // so synthesis can still run (cache is secondary on that route).
    provider.mark_tool_vendor_missing(&request.model);
    let recovered = provider.build_native_payload(&request, false).expect("recovery payload");
    assert!(recovered.get("tools").is_none(), "tools omitted when no tool vendor can serve them");
    assert!(recovered.get("tool_choice").is_none());

    // Tool-enabled requests still send tools (and fail fast upstream).
    request.tool_choice = Some(ToolChoice::Auto);
    let enabled = provider.build_native_payload(&request, false).expect("enabled payload");
    assert!(enabled.get("tools").is_some());
}

#[test]
fn native_payload_enables_response_streaming() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::XAI_GROK_4_6.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::XAI_GROK_4_6.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, true).expect("payload");

    assert_eq!(payload["stream"], true);
}

#[test]
fn native_payload_forwards_reasoning_effort_on_native_routes() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::OPENAI_GPT_5_5.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::OPENAI_GPT_5_5.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(payload["reasoning_effort"], "high");
    assert!(payload.get("thinking").is_none());
}

#[test]
fn native_payload_forwards_thinking_budget_for_budget_routes() {
    let provider = MergeGatewayProvider::with_model(
        "test-key".to_string(),
        models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5.to_string(),
    );
    let request = LLMRequest {
        model: models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        max_tokens: Some(2000),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("reasoning_effort").is_none());
    assert_eq!(payload["thinking"]["type"], "enabled");
    assert_eq!(payload["thinking"]["budget_tokens"], 1900);
}

#[test]
fn native_payload_omits_thinking_when_budget_exceeds_max_tokens() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEEPSEEK_FLASH.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::DEEPSEEK_FLASH.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::Medium),
        max_tokens: Some(1000),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("reasoning_effort").is_none());
    assert!(payload.get("thinking").is_none());
}

#[test]
fn native_payload_omits_reasoning_for_unclassified_routes() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("reasoning_effort").is_none());
    assert!(payload.get("thinking").is_none());
}

#[test]
fn reasoning_capabilities_are_route_aware() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::OPENAI_GPT_5_5.to_string());
    assert!(provider.supports_reasoning(models::merge_gateway::OPENAI_GPT_5_5));
    assert!(provider.supports_reasoning_effort(models::merge_gateway::OPENAI_GPT_5_5));
    assert!(provider.supports_reasoning(models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5));
    assert!(provider.supports_reasoning_effort(models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5));
    assert!(provider.supports_reasoning(models::merge_gateway::ZAI_GLM_5_3_FLASH));
    assert!(provider.supports_reasoning_effort(models::merge_gateway::ZAI_GLM_5_3_FLASH));
    assert!(!provider.supports_reasoning(models::merge_gateway::DEFAULT_ROUTING));
    assert!(!provider.supports_reasoning_effort(models::merge_gateway::DEFAULT_ROUTING));
}

#[test]
fn native_payload_forwards_reasoning_effort_on_zai_route() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(payload["reasoning_effort"], "high");
    assert!(payload.get("thinking").is_none());
}

#[test]
fn model_behavior_override_wins_for_reasoning_capabilities() {
    let model = models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5.to_string();
    let model_behavior = serde_json::from_value::<ModelConfig>(json!({
        "model_supports_reasoning": true,
        "model_supports_reasoning_effort": true,
    }))
    .expect("model behavior");
    let provider = MergeGatewayProvider::from_config(
        Some("test-key".to_string()),
        Some(model.clone()),
        None,
        None,
        None,
        None,
        Some(model_behavior),
    );

    assert!(provider.supports_reasoning(&model));
    assert!(provider.supports_reasoning_effort(&model));
}

#[test]
fn native_payload_sanitizes_gemini_incompatible_tool_schemas() {
    let provider = MergeGatewayProvider::with_model(
        "test-key".to_string(),
        models::merge_gateway::GOOGLE_GEMINI_3_7_FLASH.to_string(),
    );
    let request = LLMRequest {
        model: models::merge_gateway::GOOGLE_GEMINI_3_7_FLASH.to_string(),
        tools: Some(Arc::new(vec![
            ToolDefinition::function(
                "write_stdin".to_string(),
                "Write to a running command".to_string(),
                write_stdin_parameters(),
            ),
            ToolDefinition::function("apply_patch".to_string(), "Edit files".to_string(), apply_patch_parameters()),
        ])),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");
    let tools = payload["tools"].as_array().expect("native tools");

    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|tool| tool["parameters"].get("anyOf").is_none()));
    assert_eq!(tools[0]["parameters"]["required"], json!(["session_id"]));
    assert!(tools[1]["parameters"].get("required").is_none());
}

#[test]
fn native_payload_forwards_prompt_cache_key_for_routing_stickiness() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("vtcode:merge:session-123".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:merge:session-123"));
}

#[test]
fn native_payload_omits_blank_prompt_cache_key() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("   ".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("prompt_cache_key").is_none());
}

#[test]
fn native_payload_includes_session_id_from_cache_key_lineage() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        system_prompt: Some(Arc::from("sys")),
        messages: vec![Message::user("hi".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("vtcode:merge:session-lineage-1".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(
        payload.get("session_id").and_then(Value::as_str),
        Some("session-lineage-1"),
        "body session_id must be the namespaced lineage without the vtcode prefix"
    );
    assert_eq!(merge_session_identity(&request).as_deref(), Some("session-lineage-1"));
}

#[test]
fn native_payload_omits_blank_session_id() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        system_prompt: Some(Arc::from("sys")),
        messages: vec![Message::user("hi".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("   ".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("session_id").is_none());
    assert!(merge_session_identity(&request).is_none());
}

#[test]
fn merge_session_identity_strips_legacy_prefix_hash_suffix() {
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("vtcode:merge:lineage-abc-deadbeef01234567".to_string());
    assert_eq!(merge_session_identity(&request).as_deref(), Some("lineage-abc"));

    request.prompt_cache_key = Some("vtcode:openai:lineage-abc-0123456789abcdef".to_string());
    assert_eq!(merge_session_identity(&request).as_deref(), Some("lineage-abc"));

    // Non-hex / wrong-length tails stay intact.
    request.prompt_cache_key = Some("vtcode:merge:lineage-notahash".to_string());
    assert_eq!(merge_session_identity(&request).as_deref(), Some("lineage-notahash"));
}

#[test]
fn native_usage_surfaces_gateway_cache_signals() {
    let usage = MergeGatewayProvider::parse_native_usage(Some(&json!({
        "input_tokens": 1000,
        "output_tokens": 100,
        "total_tokens": 1100,
        "input_tokens_details": {"cached_tokens": 800},
        "prompt_tokens_details": {"cache_write_tokens": 50},
    })))
    .expect("usage");

    assert_eq!(usage.prompt_tokens, 1000);
    assert_eq!(usage.cached_prompt_tokens, Some(800));
    assert_eq!(usage.cache_creation_tokens, Some(50));
    assert_eq!(usage.cache_read_tokens, Some(800));
}

#[test]
fn native_usage_without_cache_signals_reports_no_cache_metrics() {
    let usage = MergeGatewayProvider::parse_native_usage(Some(&json!({
        "input_tokens": 100,
        "output_tokens": 10,
        "total_tokens": 110,
    })))
    .expect("usage");

    assert_eq!(usage.cached_prompt_tokens, None);
    assert_eq!(usage.cache_creation_tokens, None);
    assert_eq!(usage.cache_read_tokens, None);
}

#[test]
fn native_usage_prefers_explicit_cache_read_tokens() {
    let usage = MergeGatewayProvider::parse_native_usage(Some(&json!({
        "input_tokens": 500,
        "output_tokens": 50,
        "total_tokens": 550,
        "cache_read_tokens": 200,
        "prompt_cache_write_tokens": 30,
    })))
    .expect("usage");

    assert_eq!(usage.cache_read_tokens, Some(200));
    assert_eq!(usage.cache_creation_tokens, Some(30));
    // Gateway-only cache-read fields must also populate the OpenAI-style
    // cached field so trajectory metrics are not zero-filled.
    assert_eq!(usage.cached_prompt_tokens, Some(200));
}

#[test]
fn native_usage_maps_anthropic_style_cache_fields_into_cached_prompt_tokens() {
    // Merge Gateway native Responses reports Z.AI automatic cache activity
    // as Anthropic-style fields. Trajectory logs lead with
    // `cached_prompt_tokens`; without this mapping, healthy cache hits
    // look like permanent zeros.
    let usage = MergeGatewayProvider::parse_native_usage(Some(&json!({
        "input_tokens": 10000,
        "output_tokens": 400,
        "total_tokens": 10400,
        "cache_read_input_tokens": 9000,
        "cache_creation_input_tokens": 200,
    })))
    .expect("usage");

    assert_eq!(usage.cache_read_tokens, Some(9000));
    assert_eq!(usage.cache_creation_tokens, Some(200));
    assert_eq!(usage.cached_prompt_tokens, Some(9000));
}

#[tokio::test]
async fn native_generate_uses_responses_endpoint_and_parses_tool_use_response() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_123",
            "object": "response",
            "created_at": "2026-03-23T12:03:00Z",
            "model": "openai/gpt-5.1",
            "output": [{
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Hello"},
                    {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"location": "Paris"}}
                ],
                "finish_reason": "tool_use"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15, "cost": 0.01}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = provider
        .generate(LLMRequest {
            model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("native generate");

    assert_eq!(response.content.as_deref(), Some("Hello"));
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    assert_eq!(response.tool_calls.as_ref().expect("tool calls").len(), 1);
    let tool_call = &response.tool_calls.as_ref().expect("tool calls")[0];
    assert_eq!(tool_call.id, "call_1");
    assert_eq!(tool_call.tool_name(), Some("get_weather"));
    assert_eq!(tool_call.parsed_arguments().expect("tool args"), json!({"location": "Paris"}));
    assert_eq!(response.usage.as_ref().expect("usage").prompt_tokens, 10);
    assert_eq!(response.usage.as_ref().expect("usage").completion_tokens, 5);
    assert_eq!(response.usage.as_ref().expect("usage").total_tokens, 15);
    assert_eq!(response.request_id.as_deref(), Some("resp_123"));
}

#[tokio::test]
async fn response_delta_events_are_normalized_for_compatibility() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    let stream_body = [
        sse("response.output_text.delta", json!({"delta": "Hello"})),
        sse(
            "response.output_item.added",
            json!({
                "output_item": {
                    "type": "message",
                    "id": "msg_1",
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "call_1", "name": "get_weather"}
                    ],
                    "finish_reason": "tool_use"
                }
            }),
        ),
        sse(
            "response.function_call_arguments.delta",
            json!({"call_id": "call_1", "delta": r#"{"location":"San Francisco"}"#}),
        ),
        sse(
            "response.output_item.done",
            json!({
                "output_item": {
                    "type": "message",
                    "id": "msg_1",
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"location": "San Francisco"}}
                    ],
                    "finish_reason": "tool_use"
                }
            }),
        ),
        sse(
            "response.completed",
            json!({
                "response": {
                    "id": "resp_123",
                    "object": "response",
                    "created_at": "2026-03-23T12:03:00Z",
                    "model": "openai/gpt-5.1",
                    "output": [{
                        "type": "message",
                        "id": "msg_1",
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "Hello"},
                            {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"location": "San Francisco"}}
                        ],
                        "finish_reason": "tool_use"
                    }],
                    "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15, "cost": 0.01}
                }
            }),
        ),
    ]
    .concat();

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event"));
    }

    assert!(matches!(
        events.as_slice(),
        [
            NormalizedStreamEvent::TextDelta { delta },
            NormalizedStreamEvent::ToolCallStart { call_id, name },
            NormalizedStreamEvent::ToolCallDelta { call_id: delta_call_id, delta: tool_delta },
            NormalizedStreamEvent::Usage { usage },
            NormalizedStreamEvent::Done { response }
        ]
        if delta == "Hello"
            && call_id == "call_1"
            && name.as_deref() == Some("get_weather")
            && delta_call_id == "call_1"
            && tool_delta == r#"{"location":"San Francisco"}"#
            && usage.prompt_tokens == 10
            && usage.completion_tokens == 5
            && usage.total_tokens == 15
            && response.content.as_deref() == Some("Hello")
            && response.tool_calls.as_ref().is_some_and(|calls| calls.len() == 1)
    ));
}

#[tokio::test]
async fn native_stream_snapshots_emit_tool_arguments_and_done_terminal() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    let stream_response = json!({
        "id": "resp_123",
        "object": "response",
        "created_at": "2026-03-23T12:03:00Z",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "id": "msg_1",
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": "call_1",
                "name": "exec_command"
            }],
            "finish_reason": "tool_use"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15, "cost": 0.01}
    });
    let done_response = json!({
        "id": "resp_123",
        "object": "response",
        "created_at": "2026-03-23T12:03:00Z",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "id": "msg_1",
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": "call_1",
                "name": "exec_command",
                "input": {"cmd": "pwd"}
            }],
            "finish_reason": "tool_use"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15, "cost": 0.01}
    });
    let stream_body = [
        sse("response.stream", stream_response),
        sse("response.done", done_response),
    ]
    .concat();

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::XAI_GROK_4_6.to_string(),
            messages: vec![Message::user("run pwd".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event"));
    }

    assert!(matches!(
        events.as_slice(),
        [
            NormalizedStreamEvent::ToolCallStart { call_id, name },
            NormalizedStreamEvent::ToolCallDelta { call_id: delta_call_id, delta },
            NormalizedStreamEvent::Usage { usage },
            NormalizedStreamEvent::Done { response }
        ]
        if call_id == "call_1"
            && name.as_deref() == Some("exec_command")
            && delta_call_id == "call_1"
            && delta == r#"{"cmd":"pwd"}"#
            && usage.prompt_tokens == 10
            && usage.completion_tokens == 5
            && usage.total_tokens == 15
            && response.tool_calls.as_ref().is_some_and(|calls| {
                calls.len() == 1
                    && calls[0].tool_name() == Some("exec_command")
                    && calls[0].parsed_arguments().expect("tool args") == json!({"cmd": "pwd"})
            })
    ));
}

#[tokio::test]
async fn native_stream_snapshots_emit_text_before_terminal_frame() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    let first_response = json!({
        "id": "resp_123",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello"}],
            "finish_reason": null
        }]
    });
    let cumulative_response = json!({
        "id": "resp_123",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello world"}],
            "finish_reason": "stop"
        }]
    });
    let stream_body = [
        sse("response.stream", first_response),
        sse("response.stream", cumulative_response.clone()),
        sse("response.done", cumulative_response),
    ]
    .concat();

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::XAI_GROK_4_6.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event"));
    }

    assert!(matches!(
        events.as_slice(),
        [
            NormalizedStreamEvent::TextDelta { delta: first_delta },
            NormalizedStreamEvent::TextDelta { delta: second_delta },
            NormalizedStreamEvent::Done { response }
        ]
        if first_delta == "Hello"
            && second_delta == " world"
            && response.content.as_deref() == Some("Hello world")
    ));
}

#[tokio::test]
async fn native_stream_uses_latest_cumulative_snapshot() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    let first_response = json!({
        "id": "resp_123",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "obsolete"}],
            "finish_reason": "stop"
        }]
    });
    let final_response = json!({
        "id": "resp_123",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "final"}],
            "finish_reason": "stop"
        }]
    });
    let stream_body = [
        sse("response.stream", first_response),
        sse("response.stream", final_response.clone()),
        sse("response.done", final_response),
    ]
    .concat();

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::XAI_GROK_4_6.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event"));
    }

    assert!(matches!(
        events.as_slice(),
        [
            NormalizedStreamEvent::TextDelta { delta },
            NormalizedStreamEvent::Done { response }
        ]
        if delta == "final" && response.content.as_deref() == Some("final")
    ));
}

#[tokio::test]
async fn native_stream_uses_object_field_for_frame_kind() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    let first_response = json!({
        "id": "resp_123",
        "object": "response.stream",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "first"}],
            "finish_reason": null
        }]
    });
    let final_response = json!({
        "id": "resp_123",
        "object": "response.done",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "complete response"}],
            "finish_reason": "stop"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 3, "total_tokens": 13}
    });
    let stream_body = [sse_data(first_response), sse_data(final_response)].concat();

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::XAI_GROK_4_6.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event"));
    }

    assert!(matches!(
        events.as_slice(),
        [
            NormalizedStreamEvent::TextDelta { delta },
            NormalizedStreamEvent::Usage { usage },
            NormalizedStreamEvent::Done { response }
        ]
        if delta == "complete response"
            && usage.prompt_tokens == 10
            && usage.completion_tokens == 3
            && response.content.as_deref() == Some("complete response")
    ));
}

#[tokio::test]
async fn native_stream_prefers_sse_event_name_over_payload_kind() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    let first_response = json!({
        "id": "resp_123",
        "type": "response",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "complete response"}],
            "finish_reason": null
        }]
    });
    let final_response = json!({
        "id": "resp_123",
        "type": "response",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "complete response"}],
            "finish_reason": "stop"
        }],
        "usage": {"input_tokens": 10, "output_tokens": 3, "total_tokens": 13}
    });
    let stream_body = [
        sse("response.stream", first_response),
        sse("response.done", final_response),
    ]
    .concat();

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::XAI_GROK_4_6.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event"));
    }

    assert!(matches!(
        events.as_slice(),
        [
            NormalizedStreamEvent::TextDelta { delta },
            NormalizedStreamEvent::Usage { usage },
            NormalizedStreamEvent::Done { response }
        ]
        if delta == "complete response"
            && usage.prompt_tokens == 10
            && usage.completion_tokens == 3
            && response.content.as_deref() == Some("complete response")
    ));
}

#[tokio::test]
async fn native_stream_fallback_restart_discards_previous_snapshot() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    let old_response = json!({
        "id": "resp_old",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "old"},
                {"type": "tool_use", "id": "call_old", "name": "exec_command"}
            ],
            "finish_reason": "tool_use"
        }]
    });
    let new_response = json!({
        "id": "resp_new",
        "object": "response",
        "model": "xai/grok-4.6",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "new"}],
            "finish_reason": "stop"
        }],
        "usage": {"input_tokens": 3, "output_tokens": 1, "total_tokens": 4}
    });
    let stream_body = [
        sse("response.stream", old_response),
        sse_data(json!({"fallback_restart": true, "model": "xai/grok-4.6", "vendor": "xai"})),
        sse("response.stream", new_response.clone()),
        sse("response.done", new_response),
    ]
    .concat();

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::XAI_GROK_4_6.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event"));
    }

    let text = events
        .iter()
        .filter_map(|event| match event {
            NormalizedStreamEvent::TextDelta { delta } => Some(delta.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "new", "events: {events:?}");
    assert!(matches!(
        events.last(),
        Some(NormalizedStreamEvent::Done { response })
            if response.content.as_deref() == Some("new")
                && response.request_id.as_deref() == Some("resp_new")
    ));
}

#[tokio::test]
async fn native_stream_error_frame_is_returned_as_provider_error() {
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse(
                    "response.error",
                    json!({
                        "error": {
                            "type": "provider_error",
                            "message": "upstream failed",
                            "status_code": 502
                        }
                    }),
                )),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::merge_gateway::XAI_GROK_4_6.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream");

    let error = stream.next().await.expect("error event").expect_err("stream should fail");
    assert!(error.to_string().contains("upstream failed"));
}

#[tokio::test]
async fn legacy_openai_base_url_uses_chat_completions_endpoint() {
    let server = MockServer::start().await;
    let provider = MergeGatewayProvider::from_config(
        Some("test-key".to_string()),
        Some(models::merge_gateway::DEFAULT_ROUTING.to_string()),
        Some(format!("{}/v1/openai", server.uri())),
        None,
        None,
        None,
        None,
    );

    Mock::given(method("POST"))
        .and(path("/v1/openai/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl_1",
            "choices": [{
                "finish_reason": "stop",
                "message": {"content": "legacy hello"}
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = provider
        .generate(LLMRequest {
            model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("legacy generate");

    assert_eq!(response.content.as_deref(), Some("legacy hello"));
    assert_eq!(response.finish_reason, FinishReason::Stop);
}
