use super::*;

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
