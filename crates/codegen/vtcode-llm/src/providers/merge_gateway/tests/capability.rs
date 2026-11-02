use super::*;

#[test]
fn non_streaming_capability_is_pinned_for_stream_timeout_fallback() {
    // The harness's stream-timeout retry falls back to non-streaming only
    // when this capability is advertised; losing it silently re-streams
    // every retry into the first-token timeout watchdog. Streaming-only
    // routes (`zai/`) are the intentional exception: they reject
    // non-streaming with `streaming_only`, so advertising the fallback would
    // re-hit the known-failing mode.
    let provider = test_provider("http://127.0.0.1:1");
    for model in models::merge_gateway::SUPPORTED_MODELS {
        if is_streaming_only_model(model) {
            assert!(
                !LLMProvider::supports_non_streaming(&provider, model),
                "streaming-only route {model} must not advertise non-streaming"
            );
        } else {
            assert!(
                LLMProvider::supports_non_streaming(&provider, model),
                "route {model} must advertise non-streaming fallback capability"
            );
        }
    }
}

#[test]
fn streaming_only_detection_matches_gateway_body() {
    let body = r#"{"error":{"type":"provider_error","message":"Only streaming requests are supported. Set stream to true.","code":"streaming_only","param":"stream","source":"provider","provider":"zai"}}"#;
    assert!(is_streaming_only_rejection(StatusCode::BAD_REQUEST, body));
    assert!(is_streaming_only_rejection(StatusCode::UNPROCESSABLE_ENTITY, body));
    assert!(!is_streaming_only_rejection(StatusCode::BAD_REQUEST, r#"{"error":{"code":"invalid_parameter"}}"#));
    assert!(!is_streaming_only_rejection(StatusCode::INTERNAL_SERVER_ERROR, body));
    assert!(!is_streaming_only_rejection(StatusCode::BAD_REQUEST, ""));
    // Disjoint from the other classifiers.
    assert!(!is_capability_unavailable(StatusCode::BAD_REQUEST, body));
    assert!(!is_merge_tier_pricing_rejection(StatusCode::BAD_REQUEST, body));
}

#[test]
fn streaming_only_model_matches_zai_prefix() {
    assert!(is_streaming_only_model(models::merge_gateway::ZAI_GLM_5_3_FLASH));
    assert!(is_streaming_only_model(models::merge_gateway::ZAI_GLM_5_3_FLASHX));
    assert!(is_streaming_only_model("zai/glm-future"));
    assert!(!is_streaming_only_model(models::merge_gateway::OPENAI_GPT_6_1_SOL));
    assert!(!is_streaming_only_model(models::merge_gateway::DEFAULT_ROUTING));
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
                    "message": "Model 'deepseek/deepseek-v4.1-flash-1017' has no vendor that supports the requested capabilities (['reasoning', 'tools']).",
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
async fn native_generate_retries_with_streaming_on_streaming_only() {
    use std::sync::Mutex;

    // `zai/` routes reject non-streaming `/responses` with 400
    // `streaming_only`. `generate` must retry once via streaming and return
    // the collected response instead of surfacing the transport mismatch.
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_mock = Arc::clone(&seen);

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            let payload: Value = serde_json::from_slice(&req.body).expect("valid json body");
            let streaming = payload.get("stream").and_then(Value::as_bool).unwrap_or(false);
            seen_for_mock.lock().expect("mutex not poisoned").push(streaming);
            if streaming {
                let body = [sse(
                    "response.completed",
                    json!({
                        "response": {
                            "id": "resp_stream_fallback",
                            "object": "response",
                            "created_at": "2026-03-23T12:03:00Z",
                            "model": "zai/glm-5.3-flash",
                            "output": [{
                                "type": "message",
                                "id": "msg_1",
                                "role": "assistant",
                                "content": [{"type": "text", "text": "Hello"}],
                                "finish_reason": "stop"
                            }],
                            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
                        }
                    }),
                )]
                .concat();
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body)
            } else {
                ResponseTemplate::new(400).set_body_json(json!({
                    "error": {
                        "type": "provider_error",
                        "message": "Only streaming requests are supported. Set stream to true.",
                        "code": "streaming_only",
                        "param": "stream",
                        "source": "provider",
                        "provider": "zai"
                    }
                }))
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let response = provider
        .generate(LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
            ..Default::default()
        })
        .await
        .expect("streaming-only retry should succeed");
    assert_eq!(response.content.as_deref(), Some("Hello"));
    assert_eq!(seen.lock().expect("mutex not poisoned").as_slice(), &[false, true]);
}

#[tokio::test]
async fn native_stream_does_not_downgrade_streaming_only_routes() {
    // A `capability_unavailable` streaming failure on a streaming-only route
    // must surface directly: downgrading to non-streaming is known to fail
    // with `streaming_only`, so the transport must not ping-pong.
    let server = MockServer::start().await;
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "type": "invalid_request_error",
                "message": "Model 'zai/glm-5.3-flash' has no vendor that supports the requested capabilities (['streaming_tools', 'tools']).",
                "source": "gateway",
                "code": "capability_unavailable",
                "param": "model"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = provider
        .stream_normalized(LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get weather".to_string(),
                json!({"type": "object", "properties": {}}),
            )])),
            ..Default::default()
        })
        .await;
    let err = match result {
        Ok(_) => panic!("streaming-only capability failure must not downgrade"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("capability"), "got: {err}");
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
