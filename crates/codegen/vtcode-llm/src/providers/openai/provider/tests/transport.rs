use super::*;

#[tokio::test]
async fn api_key_responses_stream_sends_metadata_and_preserves_usage() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let captured = Arc::new(Mutex::new(None::<Value>));
    let captured_for_mock = Arc::clone(&captured);
    let stream_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello from api key stream\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_api_stream\",\"output\":[],\"usage\":{\"input_tokens\":21,\"output_tokens\":6,\"total_tokens\":27,\"input_tokens_details\":{\"cached_tokens\":8}}}}\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&req.body).expect("valid json");
            *captured_for_mock.lock().expect("not poisoned") = Some(json!({
                "body": body,
                "beta": req.headers.get("openai-beta").and_then(|v| v.to_str().ok()),
                "client_request_id": req.headers.get("x-client-request-id").and_then(|v| v.to_str().ok()),
                "turn_metadata": req.headers.get("x-turn-metadata").and_then(|v| v.to_str().ok()),
            }));
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body)
        })
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::new_with_client(
        "test-key".to_string(),
        None,
        models::openai::GPT_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client"),
        native_openai_mock_base_url(&server),
        TimeoutsConfig::default(),
    );

    let mut stream = provider
        .stream(provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())].into(),
            model: models::openai::GPT_5.to_string(),
            response_store: Some(true),
            metadata: Some(json!({"commit": "abc123"})),
            ..Default::default()
        })
        .await
        .expect("stream should start");
    let mut text = String::new();
    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event should parse") {
            provider::LLMStreamEvent::Token { delta } => text.push_str(&delta),
            provider::LLMStreamEvent::Completed { response } => {
                completed = Some(*response);
                break;
            }
            provider::LLMStreamEvent::Reasoning { .. }
            | provider::LLMStreamEvent::ReasoningSignature { .. }
            | provider::LLMStreamEvent::ReasoningStage { .. } => {}
        }
    }

    let response = completed.expect("completion event should be emitted");
    assert_eq!(text, "hello from api key stream");
    assert_eq!(response.content.as_deref(), Some("hello from api key stream"));
    assert_eq!(response.request_id.as_deref(), Some("resp_api_stream"));
    let usage = response.usage.expect("final usage should be preserved");
    assert_eq!(usage.prompt_tokens, 21);
    assert_eq!(usage.completion_tokens, 6);
    assert_eq!(usage.total_tokens, 27);
    assert_eq!(usage.cached_prompt_tokens, Some(8));

    let captured = captured.lock().expect("not poisoned").clone().expect("request captured");
    assert_eq!(captured.get("beta").and_then(Value::as_str), Some("responses=v1"));
    assert!(
        captured
            .get("client_request_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("vtcode-"))
    );
    assert_eq!(captured.get("turn_metadata").and_then(Value::as_str), Some(r#"{"commit":"abc123"}"#));
    let payload = captured.get("body").expect("body captured");
    assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(true));
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    assert_ne!(payload.get("include").and_then(Value::as_array), Some(&vec![json!("reasoning.encrypted_content")]));
}

#[tokio::test]
async fn generate_coerces_luna_to_responses_streaming() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let captured = Arc::new(Mutex::new(None::<Value>));
    let captured_for_mock = Arc::clone(&captured);

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            *captured_for_mock.lock().expect("not poisoned") =
                Some(serde_json::from_slice(&req.body).expect("valid json"));
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"luna summary\"}\n\n\
                     data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_luna\",\"output\":[]}}\n\n",
                )
        })
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::new_with_client(
        "test-key".to_string(),
        None,
        models::openai::GPT_5_6_LUNA.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client"),
        native_openai_mock_base_url(&server),
        TimeoutsConfig::default(),
    );

    let response = provider
        .generate(provider::LLMRequest {
            messages: vec![provider::Message::user("Summarize this".to_string())].into(),
            model: models::openai::GPT_5_6_LUNA.to_string(),
            ..Default::default()
        })
        .await
        .expect("streaming-required model should generate through Responses SSE");

    assert_eq!(response.content.as_deref(), Some("luna summary"));
    let payload = captured.lock().expect("not poisoned").clone().expect("request captured");
    assert_eq!(payload["stream"], json!(true));
}

#[tokio::test]
async fn chatgpt_responses_stream_accepts_empty_final_output_after_text_delta() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let captured = Arc::new(Mutex::new(None::<Value>));
    let captured_for_mock = Arc::clone(&captured);
    let stream_body = concat!(
        "data: {\"type\":\"response.queued\",\"response\":{\"id\":\"resp_chatgpt_stream\",\"status\":\"queued\"}}\n\n",
        "data: {\"type\":\"response.file_search_call.searching\",\"item_id\":\"fs_1\",\"output_index\":0}\n\n",
        "data: {\"type\":\"response.code_interpreter_call_code.delta\",\"item_id\":\"ci_1\",\"output_index\":1,\"sequence_number\":1,\"delta\":\"print(1)\"}\n\n",
        "data: {\"type\":\"response.mcp_call_arguments.delta\",\"item_id\":\"mcp_1\",\"call_id\":\"call_mcp\",\"output_index\":2,\"sequence_number\":2,\"delta\":\"{\\\"query\\\":\\\"vtcode\\\"}\"}\n\n",
        "data: {\"type\":\"response.image_generation_call.partial_image\",\"item_id\":\"img_1\",\"output_index\":3,\"sequence_number\":3,\"partial_image_b64\":\"ZmFrZQ==\"}\n\n",
        "data: {\"type\":\"response.custom_tool_call_input.done\",\"item_id\":\"custom_1\",\"call_id\":\"call_custom\",\"output_index\":4,\"sequence_number\":4,\"input\":\"{\\\"cmd\\\":\\\"test\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello from stream\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_chatgpt_stream\",\"output\":[],\"usage\":{\"input_tokens\":13,\"output_tokens\":4,\"total_tokens\":17}}}\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&req.body).expect("valid json");
            *captured_for_mock.lock().expect("not poisoned") = Some(json!({
                "body": body,
                "account": req.headers.get("chatgpt-account-id").and_then(|v| v.to_str().ok()),
                "originator": req.headers.get("originator").and_then(|v| v.to_str().ok()),
                "beta": req.headers.get("openai-beta").and_then(|v| v.to_str().ok()),
                "has_session_id": req.headers.contains_key("session_id"),
            }));
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(stream_body)
        })
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::new_with_client(
        String::new(),
        Some(sample_chatgpt_auth_handle()),
        models::openai::GPT_5_6_SOL.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client"),
        chatgpt_mock_base_url(&server),
        TimeoutsConfig::default(),
    );

    let mut stream = provider
        .stream(provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())].into(),
            model: models::openai::GPT_5_6_SOL.to_string(),
            ..Default::default()
        })
        .await
        .expect("stream should start");
    let mut text = String::new();
    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event should parse") {
            provider::LLMStreamEvent::Token { delta } => text.push_str(&delta),
            provider::LLMStreamEvent::Completed { response } => {
                completed = Some(*response);
                break;
            }
            provider::LLMStreamEvent::Reasoning { .. }
            | provider::LLMStreamEvent::ReasoningSignature { .. }
            | provider::LLMStreamEvent::ReasoningStage { .. } => {}
        }
    }

    let response = completed.expect("completion event should be emitted");
    assert_eq!(text, "hello from stream");
    assert_eq!(response.content.as_deref(), Some("hello from stream"));
    assert_eq!(response.request_id.as_deref(), Some("resp_chatgpt_stream"));
    let usage = response.usage.expect("final usage should be preserved");
    assert_eq!(usage.prompt_tokens, 13);
    assert_eq!(usage.completion_tokens, 4);
    assert_eq!(usage.total_tokens, 17);

    let payload = captured.lock().expect("not poisoned").clone().expect("payload captured");
    assert_eq!(payload.get("account").and_then(Value::as_str), Some("acc_123"));
    assert_eq!(payload.get("originator").and_then(Value::as_str), Some("codex_cli_rs"));
    assert_eq!(payload.get("beta").and_then(Value::as_str), Some("responses=v1"));
    assert_eq!(
        payload.get("has_session_id").and_then(Value::as_bool),
        Some(false),
        "ChatGPT Responses requests must not include a session_id header"
    );
    let payload = payload.get("body").expect("body captured");
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    assert_eq!(payload.get("include").and_then(Value::as_array), Some(&vec![json!("reasoning.encrypted_content")]));
}

#[tokio::test]
async fn explicit_openai_chat_format_keeps_chat_completions_path() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion_response_body("chat override")))
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::from_custom_config(
        "custom".to_string(),
        "Custom".to_string(),
        Some("test-key".to_string()),
        Some(models::openai::GPT_5_6.to_string()),
        Some(native_openai_mock_base_url(&server)),
        None,
        None,
        None,
        None,
        None,
        Some(vec![models::openai::GPT_5_6.to_string()]),
    )
    .with_api_format_override(Some(CustomProviderApiFormat::OpenAIChat));

    let response = provider
        .generate(sample_request(models::openai::GPT_5_6))
        .await
        .expect("chat override should succeed");

    assert_eq!(response.content.as_deref(), Some("chat override"));
}

#[tokio::test]
async fn explicit_openai_responses_format_keeps_responses_path() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(responses_api_response_body("responses override")))
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::from_custom_config(
        "custom".to_string(),
        "Custom".to_string(),
        Some("test-key".to_string()),
        Some(models::openai::GPT_5_6.to_string()),
        Some(native_openai_mock_base_url(&server)),
        None,
        None,
        None,
        None,
        None,
        Some(vec![models::openai::GPT_5_6.to_string()]),
    )
    .with_api_format_override(Some(CustomProviderApiFormat::OpenAIResponses));

    let response = provider
        .generate(sample_request(models::openai::GPT_5_6))
        .await
        .expect("responses override should succeed");

    assert_eq!(response.content.as_deref(), Some("responses override"));
}

#[tokio::test]
async fn auto_openai_format_retains_model_aware_responses_behavior() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"auto responses\"}\n\n\
             data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_auto\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::from_custom_config(
        "custom".to_string(),
        "Custom".to_string(),
        Some("test-key".to_string()),
        Some(models::openai::GPT_5_6_SOL.to_string()),
        Some(native_openai_mock_base_url(&server)),
        None,
        None,
        None,
        None,
        None,
        Some(vec![models::openai::GPT_5_6_SOL.to_string()]),
    );

    let response = provider
        .generate(sample_request(models::openai::GPT_5_6_SOL))
        .await
        .expect("auto behavior should still use responses path");

    assert_eq!(response.content.as_deref(), Some("auto responses"));
}

#[test]
fn chatgpt_backend_disables_chat_completions_fallback() {
    let provider = chatgpt_backend_provider(models::openai::GPT_5_6);
    assert!(provider.is_chatgpt_backend());
    assert!(!provider.allows_chat_completions_fallback());
}

#[test]
fn openai_models_support_streaming() {
    for model in [
        models::openai::GPT,
        models::openai::GPT_5,
        models::openai::GPT_5_6_SOL,
        models::openai::GPT_5_6_SOL,
        models::openai::GPT_5_MINI,
        models::openai::GPT_5_NANO,
    ] {
        let provider = test_provider("http://test", model);
        assert!(provider.supports_streaming(), "Model {model} should support streaming");
    }
}

#[test]
fn native_stream_required_models_disable_non_streaming() {
    assert!(models::openai::STREAMING_REQUIRED_MODELS.contains(&models::openai::GPT_5_6_LUNA));
    for model in models::openai::STREAMING_REQUIRED_MODELS {
        let provider = test_provider("http://test", model);
        assert!(!provider.supports_non_streaming(model), "Model {model} should require streaming");
    }
}

#[test]
fn chatgpt_backend_keeps_streaming_for_codex_and_disables_non_streaming() {
    let provider = chatgpt_backend_provider(models::openai::GPT_5_CODEX);
    assert!(provider.supports_streaming());
    assert!(!provider.supports_non_streaming(models::openai::GPT_5_CODEX));
}

#[tokio::test]
async fn responses_request_retries_with_fallback_model_after_not_found() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = test_provider(&server.uri(), models::openai::GPT_6_ASTRA);
    let seen_models = Arc::new(Mutex::new(Vec::new()));
    let seen_for_mock = Arc::clone(&seen_models);

    Mock::given(method("POST")).and(path("/responses"))
        .respond_with(move |req: &wiremock::Request| {
            let payload: Value = serde_json::from_slice(&req.body).expect("valid json");
            let model = payload.get("model").and_then(Value::as_str).expect("model required");
            seen_for_mock.lock().expect("not poisoned").push(model.to_string());
            match model {
                models::openai::GPT_6_ASTRA => ResponseTemplate::new(404).set_body_string("model_not_found"),
                models::openai::GPT_5_6_SOL => ResponseTemplate::new(200).set_body_json(json!({
                    "id": "resp_fallback", "status": "completed",
                    "output": [{"type":"message","role":"assistant","content":[{"type":"output_text","text":"fallback response"}]}]
                })),
                other => ResponseTemplate::new(500).set_body_string(format!("unexpected: {other}")),
            }
        }).expect(2).mount(&server).await;

    let response = provider
        .generate(provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())].into(),
            model: models::openai::GPT_6_ASTRA.to_string(),
            ..Default::default()
        })
        .await
        .expect("fallback should succeed");
    assert_eq!(response.content.as_deref(), Some("fallback response"));
    assert_eq!(
        seen_models.lock().expect("not poisoned").as_slice(),
        &[
            models::openai::GPT_6_ASTRA.to_string(),
            models::openai::GPT_5_6_SOL.to_string()
        ]
    );
}

#[tokio::test]
async fn responses_request_retries_without_flex_service_tier() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        None,
        Some(models::openai::GPT_6_ASTRA.to_string()),
        Some(native_openai_mock_base_url(&server)),
        None,
        None,
        None,
        Some(flex_openai_config()),
        None,
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_mock = Arc::clone(&seen);

    Mock::given(method("POST")).and(path("/responses"))
        .respond_with(mock_service_tier_fallback(seen_for_mock, json!({
            "id": "resp_retry", "status": "completed",
            "output": [{"type":"message","role":"assistant","content":[{"type":"output_text","text":"retry without flex succeeded"}]}]
        }))).expect(2).mount(&server).await;

    let response = provider
        .generate(provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())].into(),
            model: models::openai::GPT_6_ASTRA.to_string(),
            ..Default::default()
        })
        .await
        .expect("retry without flex should succeed");
    assert_eq!(response.content.as_deref(), Some("retry without flex succeeded"));
    assert_eq!(seen.lock().expect("not poisoned").as_slice(), &[Some("flex".to_string()), None]);
}

#[tokio::test]
async fn responses_requests_include_client_request_id_and_debug_metadata() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = test_provider(&server.uri(), models::openai::GPT_5);
    Mock::given(method("POST")).and(path("/responses"))
        .respond_with(|req: &wiremock::Request| {
            let req_id = req.headers.get("x-client-request-id").and_then(|v| v.to_str().ok())
                .expect("x-client-request-id required");
            assert!(req_id.starts_with("vtcode-"));
            ResponseTemplate::new(400)
                .insert_header("x-request-id", "req_123")
                .insert_header("retry-after", "15")
                .set_body_string(r#"{"error":{"message":"Bad request","type":"invalid_request_error","param":"text.verbosity","code":"unsupported_parameter"}}"#)
        }).expect(1).mount(&server).await;
    let err = provider
        .generate(provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_string())].into(),
            model: models::openai::GPT_5.to_string(),
            ..Default::default()
        })
        .await
        .expect_err("should surface error");
    let text = err.to_string();
    assert!(
        text.contains("request_id=req_123")
            && text.contains("client_request_id=vtcode-")
            && text.contains("retry_after=15")
            && text.contains("type=invalid_request_error")
    );
}

#[tokio::test]
async fn responses_parameter_rejection_preserves_route_for_generation_and_both_streaming_apis() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    for body in [
        "Unsupported parameter: max_output_tokens",
        "Invalid API parameter: reasoning.effort",
        "Responses API rejected an unsupported parameter: reasoning.effort",
        r#"{"error":{"code":"1210","message":"Invalid request parameter"}}"#,
    ] {
        for entrypoint in ["generation", "legacy", "normalized"] {
            let server = MockServer::start().await;
            let requests = Arc::new(AtomicUsize::new(0));
            let seen = Arc::clone(&requests);
            Mock::given(method("POST"))
                .and(path("/responses"))
                .respond_with(move |_: &wiremock::Request| {
                    if seen.fetch_add(1, Ordering::Relaxed) == 0 {
                        ResponseTemplate::new(400).set_body_string(body)
                    } else {
                        ResponseTemplate::new(200).set_body_json(json!({
                            "id": "resp_after_parameter_fix",
                            "output": [{"type": "message", "content": [{"type": "output_text", "text": "Responses still works"}]}]
                        }))
                    }
                })
                .expect(2)
                .mount(&server)
                .await;
            let provider = test_provider(&server.uri(), models::openai::GPT_6_LUNA);
            let request = provider::LLMRequest {
                model: models::openai::GPT_6_LUNA.to_string(),
                messages: vec![provider::Message::user("bounded fixture".into())].into(),
                ..Default::default()
            };
            let error = match entrypoint {
                "generation" => provider.generate(request.clone()).await.err(),
                "legacy" => provider.stream(request.clone()).await.err(),
                _ => provider.stream_normalized(request.clone()).await.err(),
            }
            .expect("parameter rejection must be surfaced");
            assert!(error.to_string().contains(body), "retain the rejected parameter's diagnostic");
            let response = provider.generate(request).await.expect("next request must still use Responses");
            assert_eq!(response.content.as_deref(), Some("Responses still works"));
            let recorded = server.received_requests().await.expect("recorded requests");
            assert_eq!(recorded.len(), 2, "a parameter error must not trigger a Chat request");
            assert!(recorded.iter().all(|request| request.url.path() == "/responses"));
        }
    }
}

#[tokio::test]
async fn streaming_service_tier_retry_is_cached_for_both_entrypoints_and_http_apis() {
    for responses in [false, true] {
        for normalized in [false, true] {
            for tier in ["flex", "ultrafast"] {
                let server = MockServer::start().await;
                let route = if responses { "/responses" } else { "/chat/completions" };
                let body = if responses {
                    concat!(
                        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":1,\"delta\":\"ok\"}\n\n",
                        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_tierless\",\"output\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
                    )
                } else {
                    concat!(
                        "data: {\"id\":\"chat_tierless\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
                        "data: {\"id\":\"chat_tierless\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
                        "data: [DONE]\n\n",
                    )
                };
                Mock::given(method("POST"))
                    .and(path(route))
                    .respond_with(move |request: &wiremock::Request| {
                        let payload: Value = serde_json::from_slice(&request.body).unwrap();
                        if payload.get("service_tier").is_some() {
                            ResponseTemplate::new(400).set_body_json(json!({"error": {
                                "param":"service_tier", "code":"invalid_request_error",
                                "message":"The requested service tier is not allowed for this project"
                            }}))
                        } else {
                            ResponseTemplate::new(200)
                                .insert_header("content-type", "text/event-stream")
                                .set_body_string(body)
                        }
                    })
                    .expect(3)
                    .mount(&server)
                    .await;
                let mut provider = OpenAIProvider::new_with_client(
                    "test-key".to_string(),
                    None,
                    "gpt-6-astra".to_string(),
                    reqwest::Client::builder().no_proxy().build().unwrap(),
                    native_openai_mock_base_url(&server),
                    TimeoutsConfig::default(),
                );
                provider.responses_url = Arc::from(format!("{}/responses", server.uri()));
                provider.chat_completions_url = Arc::from(format!("{}/chat/completions", server.uri()));
                if !responses {
                    provider.set_responses_api_state("gpt-6-astra", ResponsesApiState::Disabled);
                }
                for _ in 0..2 {
                    let request = provider::LLMRequest {
                        model: "gpt-6-astra".to_string(),
                        service_tier: Some(tier.to_string()),
                        messages: vec![provider::Message::user("hello".to_string())].into(),
                        metadata: Some(json!({"turn":"tier-test"})),
                        ..Default::default()
                    };
                    let mut completed = None;
                    if normalized {
                        let mut stream = provider.stream_normalized_request(request).await.unwrap();
                        while let Some(event) = stream.next().await {
                            if let NormalizedStreamEvent::Done { response } = event.unwrap() {
                                completed = Some(response);
                            }
                        }
                    } else {
                        let mut stream = provider.stream_request(request).await.unwrap();
                        while let Some(event) = stream.next().await {
                            if let provider::LLMStreamEvent::Completed { response } = event.unwrap() {
                                completed = Some(response);
                            }
                        }
                    }
                    let completed = completed.expect("completion");
                    assert_eq!(completed.content.as_deref(), Some("ok"));
                    assert_eq!(completed.usage.as_ref().unwrap().total_tokens, 5);
                }
                let requests = server.received_requests().await.unwrap();
                assert_eq!(requests.len(), 3);
                let payloads: Vec<Value> = requests
                    .iter()
                    .map(|request| serde_json::from_slice(&request.body).unwrap())
                    .collect();
                assert_eq!(payloads[0]["service_tier"], tier);
                assert!(
                    payloads[1..]
                        .iter()
                        .all(|payload| payload.get("service_tier").is_none() && payload["stream"] == true)
                );
                assert!(
                    requests
                        .iter()
                        .all(|request| request.headers["authorization"] == "Bearer test-key"
                            && request.headers["x-turn-metadata"] == r#"{"turn":"tier-test"}"#)
                );
                assert_ne!(requests[0].headers["x-client-request-id"], requests[1].headers["x-client-request-id"]);
                if responses {
                    assert!(requests.iter().all(|request| request.headers["openai-beta"] == "responses=v1"));
                }
            }
        }
    }
}

#[tokio::test]
async fn streaming_service_tier_retry_is_bounded_when_default_is_also_rejected() {
    for normalized in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(400).set_body_json(
                json!({"error":{"param":"service_tier","message":"Unsupported service_tier: ultrafast"}}),
            ))
            .expect(2)
            .mount(&server)
            .await;
        let provider = OpenAIProvider::new_with_client(
            "test-key".to_string(),
            None,
            "gpt-6-astra".to_string(),
            reqwest::Client::builder().no_proxy().build().unwrap(),
            native_openai_mock_base_url(&server),
            TimeoutsConfig::default(),
        );
        let request = provider::LLMRequest {
            model: "gpt-6-astra".to_string(),
            service_tier: Some("ultrafast".to_string()),
            messages: vec![provider::Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        let error = if normalized {
            provider.stream_normalized_request(request).await.err().unwrap()
        } else {
            provider.stream_request(request).await.err().unwrap()
        };
        assert!(error.to_string().contains("Unsupported service_tier"));
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test]
async fn streaming_service_tier_retry_preserves_transport_fallback_and_required_routes() {
    for normalized in [false, true] {
        for required in [false, true] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/responses"))
                .respond_with(|request: &wiremock::Request| {
                    let payload: Value = serde_json::from_slice(&request.body).unwrap();
                    if payload.get("service_tier").is_some() {
                        ResponseTemplate::new(400).set_body_json(
                            json!({"error": {"param":"service_tier", "message":"Unsupported service_tier: ultrafast"}}),
                        )
                    } else {
                        ResponseTemplate::new(404)
                            .insert_header("x-request-id", "retry_transport_error")
                            .set_body_json(
                                json!({"error": {"message":"Responses API is not supported by this endpoint"}}),
                            )
                    }
                })
                .expect(2)
                .mount(&server)
                .await;
            Mock::given(method("POST")).and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(concat!(
                    "data: {\"id\":\"chat_reply\",\"choices\":[{\"delta\":{\"content\":\"fallback reply\"},\"finish_reason\":null}]}\n\n",
                    "data: {\"id\":\"chat_reply\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2,\"total_tokens\":9}}\n\n",
                    "data: [DONE]\n\n",
                ))).expect(if required {0} else {1}).mount(&server).await;
            let mut provider = OpenAIProvider::new_with_client(
                "test-key".to_string(),
                None,
                "gpt-6-astra".to_string(),
                reqwest::Client::builder().no_proxy().build().unwrap(),
                native_openai_mock_base_url(&server),
                TimeoutsConfig::default(),
            );
            provider.responses_url = Arc::from(format!("{}/responses", server.uri()));
            provider.chat_completions_url = Arc::from(format!("{}/chat/completions", server.uri()));
            provider.set_responses_api_state(
                "gpt-6-astra",
                if required {
                    ResponsesApiState::Required
                } else {
                    ResponsesApiState::Allowed
                },
            );
            let request = provider::LLMRequest {
                model: "gpt-6-astra".to_string(),
                service_tier: Some("ultrafast".to_string()),
                metadata: Some(json!({"turn":"fallback"})),
                messages: vec![provider::Message::user("hello".to_string())].into(),
                ..Default::default()
            };
            let mut completed = None;
            let error = if normalized {
                match provider.stream_normalized_request(request).await {
                    Ok(mut stream) => {
                        while let Some(event) = stream.next().await {
                            if let NormalizedStreamEvent::Done { response } = event.unwrap() {
                                completed = Some(response);
                            }
                        }
                        None
                    }
                    Err(error) => Some(error),
                }
            } else {
                match provider.stream_request(request).await {
                    Ok(mut stream) => {
                        while let Some(event) = stream.next().await {
                            if let provider::LLMStreamEvent::Completed { response } = event.unwrap() {
                                completed = Some(response);
                            }
                        }
                        None
                    }
                    Err(error) => Some(error),
                }
            };
            if required {
                let error = error.unwrap().to_string();
                assert!(error.contains("Responses API is not supported"));
                assert!(error.contains("retry_transport_error"));
                assert!(completed.is_none());
            } else {
                assert!(error.is_none(), "allowed Responses route should fall back: {error:?}");
                let completed = completed.unwrap();
                assert_eq!(completed.content.as_deref(), Some("fallback reply"));
                assert_eq!(completed.usage.as_ref().unwrap().total_tokens, 9);
                assert_eq!(provider.responses_api_state("gpt-6-astra"), ResponsesApiState::Disabled);
            }
            let requests = server.received_requests().await.unwrap();
            assert_eq!(requests.len(), if required { 2 } else { 3 });
            assert_eq!(requests[0].url.path(), "/responses");
            assert_eq!(requests[1].url.path(), "/responses");
            let payloads: Vec<Value> = requests
                .iter()
                .map(|request| serde_json::from_slice(&request.body).unwrap())
                .collect();
            assert_eq!(payloads[0]["service_tier"], "ultrafast");
            assert!(payloads[1..].iter().all(|payload| payload.get("service_tier").is_none()));
            assert!(
                requests
                    .iter()
                    .all(|request| request.headers["authorization"] == "Bearer test-key"
                        && request.headers["x-turn-metadata"] == r#"{"turn":"fallback"}"#)
            );
            assert_ne!(requests[0].headers["x-client-request-id"], requests[1].headers["x-client-request-id"]);
        }
    }
}
