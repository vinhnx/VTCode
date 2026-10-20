use super::*;
use crate::provider::NormalizedStreamEvent;
use futures::StreamExt;
use wiremock::matchers::{body_json, body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::providers::test_support::start_mock_server_or_skip;

fn test_provider(base_url: &str) -> OpenResponsesProvider {
    let http_client = reqwest::Client::builder().no_proxy().build().expect("test client should build");
    OpenResponsesProvider::new_with_client(
        String::new(),
        "gpt-5".to_string(),
        http_client,
        base_url.to_string(),
        TimeoutsConfig::default(),
    )
}

#[test]
fn payload_preserves_explicit_none_in_both_transports() {
    use crate::provider::{ToolChoice, ToolDefinition};
    use std::sync::Arc;

    let provider = test_provider("https://example.test/v1");
    for stream in [false, true] {
        let mut request = LLMRequest {
            model: "gpt-5".to_owned(),
            messages: vec![Message::user("Synthesize saved evidence".to_owned())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "lookup".to_owned(),
                "Look up a value".to_owned(),
                json!({"type": "object"}),
            )])),
            tool_choice: Some(ToolChoice::None),
            ..Default::default()
        };
        for native in [false, true] {
            let build = |request: &LLMRequest| {
                if native {
                    provider.build_native_payload(request, stream)
                } else {
                    provider.build_payload(request, stream)
                }
            };
            let disabled = build(&request).expect("disabled payload");
            assert_eq!(disabled["tool_choice"], "none");
            assert!(disabled["tools"].as_array().is_some_and(|tools| !tools.is_empty()));
            assert_eq!(disabled["stream"], stream);
            request.tool_choice = Some(ToolChoice::Auto);
            let enabled = build(&request).expect("enabled payload");
            assert_eq!(enabled["tool_choice"], "auto");
            assert_eq!(disabled["tools"], enabled["tools"]);
            request.tool_choice = Some(ToolChoice::function("lookup".to_owned()));
            let forced = build(&request).expect("forced function payload");
            let expected = if native {
                json!({"type": "function", "name": "lookup"})
            } else {
                json!({"type": "function", "function": {"name": "lookup"}})
            };
            assert_eq!(forced["tool_choice"], expected);
            request.tool_choice = Some(ToolChoice::None);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NativeEntrypoint {
    Generate,
    Stream,
    NormalizedStream,
}

const NATIVE_ENTRYPOINTS: [NativeEntrypoint; 3] = [
    NativeEntrypoint::Generate,
    NativeEntrypoint::Stream,
    NativeEntrypoint::NormalizedStream,
];

async fn collect_entrypoint_text(
    provider: &OpenResponsesProvider,
    entrypoint: NativeEntrypoint,
) -> Result<String, LLMError> {
    let request = LLMRequest {
        messages: vec![Message::user("Check dispatch.".to_string())].into(),
        ..Default::default()
    };
    let mut text = String::new();
    let mut completed = false;
    match entrypoint {
        NativeEntrypoint::Generate => {
            return provider
                .generate(request)
                .await
                .map(|response| response.content.unwrap_or_default());
        }
        NativeEntrypoint::Stream => {
            let mut stream = provider.stream(request).await?;
            while let Some(event) = stream.next().await {
                match event? {
                    LLMStreamEvent::Token { delta } => text.push_str(&delta),
                    LLMStreamEvent::Completed { response } => {
                        completed = true;
                        assert_eq!(response.content.as_deref(), Some(text.as_str()));
                    }
                    _ => {}
                }
            }
        }
        NativeEntrypoint::NormalizedStream => {
            let mut stream = provider.stream_normalized(request).await?;
            while let Some(event) = stream.next().await {
                match event? {
                    NormalizedStreamEvent::TextDelta { delta } => text.push_str(&delta),
                    NormalizedStreamEvent::Done { .. } => completed = true,
                    _ => {}
                }
            }
        }
    }
    assert!(completed, "{entrypoint:?} must finish before its text is accepted");
    Ok(text)
}

#[tokio::test]
async fn native_dispatch_preserves_auth_default_model_and_stream_mode_for_all_entrypoints() {
    for entrypoint in NATIVE_ENTRYPOINTS {
        let server = MockServer::start().await;
        let mut provider = test_provider(&format!("{}/v1/", server.uri()));
        provider.api_key = "dispatch-fixture-token".to_string();
        let streaming = !matches!(entrypoint, NativeEntrypoint::Generate);
        let output = json!({
            "id": "resp_dispatch", "status": "completed",
            "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "native reply"}]}]
        });
        let response = if streaming {
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"type": "response.output_text.delta", "item_id": "msg_native", "output_index": 0,
                        "content_index": 0, "sequence_number": 1, "delta": "native reply"}),
                ))
        } else {
            ResponseTemplate::new(200).set_body_json(output)
        };
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .and(wiremock::matchers::header("authorization", "Bearer dispatch-fixture-token"))
            .and(body_partial_json(json!({"model": "gpt-5", "stream": streaming})))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;

        let text = collect_entrypoint_text(&provider, entrypoint)
            .await
            .expect("native request succeeds");
        assert_eq!(text, "native reply", "{entrypoint:?}");
        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 1, "native success must not send a legacy request");
    }
}

#[tokio::test]
async fn native_dispatch_does_not_fallback_on_other_http_errors_for_any_entrypoint() {
    for status in [400, 401, 429, 500] {
        for entrypoint in NATIVE_ENTRYPOINTS {
            let server = MockServer::start().await;
            let provider = test_provider(&server.uri());
            Mock::given(method("POST"))
                .and(path("/responses"))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                    "error": {"message": "native endpoint diagnostic"}
                })))
                .expect(1)
                .mount(&server)
                .await;

            let error = collect_entrypoint_text(&provider, entrypoint)
                .await
                .expect_err("HTTP error surfaces");
            assert!(matches!(&error, LLMError::Provider { .. }), "retain the existing error variant");
            let diagnostic = error.to_string();
            assert!(diagnostic.contains(&format!("HTTP {status}")), "{diagnostic}");
            assert!(diagnostic.contains("native endpoint diagnostic"), "{diagnostic}");
            let requests = server.received_requests().await.expect("recorded requests");
            assert_eq!(requests.len(), 1, "{entrypoint:?} must not replay status {status}");
            assert_eq!(requests[0].url.path(), "/responses");
        }
    }
}

#[tokio::test]
async fn native_dispatch_404_uses_matching_legacy_adapter_once_for_all_entrypoints() {
    for entrypoint in NATIVE_ENTRYPOINTS {
        let server = MockServer::start().await;
        let provider = test_provider(&server.uri());
        let streaming = !matches!(entrypoint, NativeEntrypoint::Generate);
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let response = if streaming {
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"legacy recovery\"}}]}\n\ndata: [DONE]\n\n",
                )
        } else {
            ResponseTemplate::new(200).set_body_json(json!({
                "id": "legacy_dispatch",
                "choices": [{"finish_reason": "stop", "message": {"content": "legacy recovery"}}]
            }))
        };
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({"model": "gpt-5", "stream": streaming})))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;

        let text = collect_entrypoint_text(&provider, entrypoint)
            .await
            .expect("legacy adapter succeeds");
        assert_eq!(text, "legacy recovery", "{entrypoint:?}");
        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url.path(), "/responses");
        assert_eq!(requests[1].url.path(), "/chat/completions");
    }
}

#[test]
fn native_payload_includes_responses_continuity_fields() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let mut request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    request.previous_response_id = Some("resp_prev_1".to_string());
    request.response_store = Some(false);
    request.responses_include = Some(vec![
        "reasoning.encrypted_content".to_string(),
        "output_text.annotations".to_string(),
    ]);

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");

    assert_eq!(payload.get("previous_response_id").and_then(Value::as_str), Some("resp_prev_1"));
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    let include = payload.get("include").and_then(Value::as_array).expect("include must exist");
    assert_eq!(include.len(), 2);
}

#[test]
fn native_payload_serializes_compact_temperature() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        temperature: Some(0.7),
        ..Default::default()
    };

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");

    assert_eq!(payload.get("temperature").and_then(Value::as_f64), Some(0.7));
    assert_eq!(
        payload.get("temperature").expect("temperature present").to_string(),
        "0.7",
        "wire form must be compact, not the f32->f64 widening tail"
    );
}

#[test]
fn native_payload_includes_context_management() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let mut request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    request.context_management = Some(serde_json::json!([{
        "type": "compaction",
        "compact_threshold": 200000
    }]));

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");
    let management = payload
        .get("context_management")
        .and_then(Value::as_array)
        .expect("context management should exist");
    assert_eq!(management.len(), 1);
}

#[test]
fn openresponses_provider_reports_compaction_support() {
    let provider = test_provider("https://api.openresponses.com/v1");
    assert!(provider.supports_responses_compaction("gpt-5"));
}

#[test]
fn openresponses_provider_disables_compaction_for_unknown_endpoint() {
    let provider = test_provider("https://api.example.com/v1");
    assert!(!provider.supports_responses_compaction("gpt-5"));
}

#[tokio::test]
async fn compact_history_request_matches_openresponses_compact_schema() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    Mock::given(method("POST"))
        .and(path("/v1/responses/compact"))
        .and(body_json(serde_json::json!({
            "model": "gpt-5",
            "input": [{
                "type": "message",
                "id": "msg_0",
                "status": "completed",
                "role": "user",
                "content": [{"type": "input_text", "text": "compact this"}]
            }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "cmp_streaming",
            "object": "response.compaction",
            "output": [{
                "type": "compaction",
                "id": "cmp_1",
                "encrypted_content": "opaque_state"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = test_provider(&format!("{}/v1", server.uri()));
    let compacted = provider
        .compact_history_request("gpt-5", &[Message::user("compact this".to_string())])
        .await
        .expect("compaction request should succeed");

    let preserved_type = compacted[0]
        .reasoning_details
        .as_ref()
        .and_then(|items| items.first())
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str);
    assert_eq!(preserved_type, Some("compaction"));
}

#[tokio::test]
async fn compact_history_request_forwards_supported_options() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    Mock::given(method("POST"))
        .and(path("/v1/responses/compact"))
        .and(body_partial_json(json!({
            "instructions": "keep decisions",
            "service_tier": "priority",
            "prompt_cache_key": "session-1"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmp_options",
            "object": "response.compaction",
            "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "compacted"}]}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = test_provider(&format!("{}/v1", server.uri()));
    provider
        .compact_history_request_with_options(
            "gpt-5",
            &[Message::user("compact this".to_string())],
            &ResponsesCompactionOptions {
                instructions: Some("  keep decisions  ".to_string()),
                service_tier: Some(" priority ".to_string()),
                prompt_cache_key: Some(" session-1 ".to_string()),
                ..ResponsesCompactionOptions::default()
            },
        )
        .await
        .expect("compaction request should succeed");
}

#[test]
fn native_response_preserves_opaque_compaction_item_for_replay() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let compaction_item = json!({
        "type": "compaction",
        "id": "cmp_1",
        "encrypted_content": "opaque_state"
    });
    let response = json!({
        "id": "resp_1",
        "status": "completed",
        "output": [
            compaction_item.clone(),
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "continued"}]
            }
        ]
    });

    let parsed = OpenResponsesProvider::parse_native_response_payload(response, "gpt-5".to_string())
        .expect("native response should parse");
    let reasoning_details = parsed.reasoning_details.clone().expect("compaction item should be preserved");
    assert_eq!(serde_json::from_str::<Value>(&reasoning_details[0]).unwrap(), compaction_item);
    let reasoning_values = reasoning_details
        .iter()
        .map(|item| serde_json::from_str(item).expect("reasoning detail should be valid JSON"))
        .collect();

    let payload = provider
        .build_native_payload(
            &LLMRequest {
                model: "gpt-5".to_string(),
                messages: vec![
                    Message::assistant(parsed.content.unwrap_or_default())
                        .with_reasoning_details(Some(reasoning_values)),
                ]
                .into(),
                ..Default::default()
            },
            false,
        )
        .expect("native payload should replay compaction item");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should be an array");

    assert_eq!(input[0], compaction_item);
    assert_eq!(input[1].get("type").and_then(Value::as_str), Some("message"));
    assert_eq!(input[1].get("role").and_then(Value::as_str), Some("assistant"));
}

#[test]
fn native_payload_preserves_opaque_reasoning_details_items() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let message = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!({
        "type": "compaction",
        "id": "cmp_1",
        "status": "completed",
        "encrypted_content": "opaque_state"
    })]));
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![message].into(),
        ..Default::default()
    };

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should be an array");

    assert_eq!(input.len(), 1);
    assert_eq!(input[0].get("type").and_then(Value::as_str), Some("compaction"));
    assert_eq!(input[0].get("encrypted_content").and_then(Value::as_str), Some("opaque_state"));
}

#[test]
fn native_payload_normalizes_stringified_reasoning_details_items() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let message = Message::assistant(String::new()).with_reasoning_details(Some(vec![
        json!(r#"{"type":"compaction","id":"cmp_1","encrypted_content":"opaque_state"}"#),
        json!("not-json"),
    ]));
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![message].into(),
        ..Default::default()
    };

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should be an array");

    assert_eq!(input.len(), 1);
    assert_eq!(input[0].get("type").and_then(Value::as_str), Some("compaction"));
}

#[test]
fn native_payload_emits_tool_response_only_as_function_call_output() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "shell".to_string(),
                    "{\"command\":\"pwd\"}".to_string(),
                )],
            ),
            Message::tool_response("call_1".to_string(), "/tmp/work".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should be an array");

    assert!(input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("function_call_output")
            && item.get("call_id").and_then(Value::as_str) == Some("call_1")
    }));
    assert!(!input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("message")
            && item.get("role").and_then(Value::as_str) == Some("user")
            && item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|part| part.get("text").and_then(Value::as_str) == Some("/tmp/work"))
    }));
}

#[test]
fn native_payload_preserves_multimodal_tool_output_items() {
    let provider = test_provider("https://api.openresponses.com/v1");
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "view_image".to_string(),
                    "{\"path\":\"./img.png\"}".to_string(),
                )],
            ),
            Message::tool_response(
                "call_1".to_string(),
                r#"[{"type":"input_text","text":"inline image note"},{"type":"input_image","image_url":"data:image/png;base64,abc"}]"#
                    .to_string(),
            ),
        ].into(),
        ..Default::default()
    };

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input should be an array");

    let function_call_output = input
        .iter()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some("call_1")
        })
        .expect("function_call_output item should exist");

    let output_items = function_call_output
        .get("output")
        .and_then(Value::as_array)
        .expect("multimodal output should be serialized as an array");
    assert_eq!(output_items.len(), 2);
    assert_eq!(output_items[0]["type"], "input_text");
    assert_eq!(output_items[0]["text"], "inline image note");
    assert_eq!(output_items[1]["type"], "input_image");
    assert_eq!(output_items[1]["image_url"], "data:image/png;base64,abc");
}

#[test]
fn native_payload_drops_unsupported_svg_image_parts() {
    // Regression: an SVG auto-attached from a quoted path in a WebMCP diff
    // must not reach the wire — providers fail the whole request with 400
    // `invalid_value` when any `input_image` carries an unsupported type.
    let provider = test_provider("https://api.openresponses.com/v1");
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![Message::user_with_parts(vec![
            crate::provider::ContentPart::text("see the logo".to_string()),
            crate::provider::ContentPart::image("PHN2Zz48L3N2Zz4=".to_string(), "image/svg+xml".to_string()),
        ])]
        .into(),
        ..Default::default()
    };

    let payload = provider
        .build_native_payload(&request, false)
        .expect("native payload should serialize");
    let serialized = serde_json::to_string(&payload).expect("payload should serialize");
    assert!(!serialized.contains("image/svg+xml"), "SVG image must be dropped from the wire payload");
    assert!(!serialized.contains("input_image"), "no image part should remain");
    assert!(serialized.contains("see the logo"), "the text part must survive");
}

#[tokio::test]
async fn generate_falls_back_to_chat_completions_when_native_endpoint_is_missing() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl_fallback",
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "content": "fallback completion"
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let response = provider
        .generate(LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("fallback generate should succeed");

    assert_eq!(response.content.as_deref(), Some("fallback completion"));
}

#[tokio::test]
async fn stream_falls_back_to_chat_completions_when_native_endpoint_is_missing() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"fallback stream\"}}]}\n\n\
data: [DONE]\n\n",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream(LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("fallback stream should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event should parse") {
            LLMStreamEvent::Completed { response } => completed = Some(response),
            LLMStreamEvent::Token { .. }
            | LLMStreamEvent::Reasoning { .. }
            | LLMStreamEvent::ReasoningSignature { .. }
            | LLMStreamEvent::ReasoningStage { .. } => {}
        }
    }

    let response = completed.expect("stream should finish with a completed response");
    assert_eq!(response.content.as_deref(), Some("fallback stream"));
}

#[tokio::test]
async fn native_stream_decodes_only_fields_consumed_by_the_hot_path() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"native stream\",\"ignored\":{\"nested\":true}}\n\n\
data: {\"type\":\"response.reasoning_content.delta\",\"delta\":\"think\"}\n\n\
data: [DONE]\n\n",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream(LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("native stream should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event should parse") {
            LLMStreamEvent::Completed { response } => completed = Some(response),
            LLMStreamEvent::Token { .. }
            | LLMStreamEvent::Reasoning { .. }
            | LLMStreamEvent::ReasoningSignature { .. }
            | LLMStreamEvent::ReasoningStage { .. } => {}
        }
    }

    let response = completed.expect("stream should finish with a completed response");
    assert_eq!(response.content.as_deref(), Some("native stream"));
}

#[tokio::test]
async fn native_stream_preserves_opaque_compaction_output_items() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = test_provider(&server.uri());
    let compaction_item = json!({
        "type": "compaction",
        "id": "cmp_1",
        "encrypted_content": "opaque_state"
    });
    let added_event = json!({
        "type": "response.output_item.added",
        "item": {
            "type": "compaction",
            "id": "cmp_1",
            "encrypted_content": "partial_state"
        }
    });
    let done_event = json!({
        "type": "response.output_item.done",
        "item": compaction_item.clone()
    });

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("data: {added_event}\n\ndata: {done_event}\n\ndata: [DONE]\n\n")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream(LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("continue".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("native stream should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        if let LLMStreamEvent::Completed { response } = event.expect("stream event should parse") {
            completed = Some(response);
        }
    }

    let response = completed.expect("stream should finish with a completed response");
    let details = response.reasoning_details.expect("compaction item should be preserved");
    assert_eq!(details.len(), 1, "added and done events must not duplicate the item");
    assert_eq!(serde_json::from_str::<Value>(&details[0]).unwrap(), compaction_item);
}

#[tokio::test]
async fn stream_normalized_emits_tool_call_start_and_delta_events() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = test_provider(&server.uri());

    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"type\":\"response.output_item.added\",\"item_id\":\"call_1\",\"output_index\":0,\"sequence_number\":1,\"item\":{\"type\":\"function_call\",\"id\":\"call_1\",\"call_id\":\"call_1\",\"name\":\"search_workspace\",\"arguments\":\"\",\"status\":\"in_progress\"}}\n\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"call_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":2,\"delta\":\"{\\\"pattern\\\":\\\"ph\"}\n\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"call_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":3,\"delta\":\"ase\\\"}\"}\n\n\
data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"sequence_number\":4,\"delta\":\"done\"}\n\n\
data: [DONE]\n\n",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("normalized stream should succeed");

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream event should parse"));
    }

    assert!(matches!(
        events.as_slice(),
        [
            NormalizedStreamEvent::ToolCallStart { call_id, name },
            NormalizedStreamEvent::ToolCallDelta { call_id: first_delta_id, delta: first_delta },
            NormalizedStreamEvent::ToolCallDelta { call_id: second_delta_id, delta: second_delta },
            NormalizedStreamEvent::TextDelta { delta },
            NormalizedStreamEvent::Done { .. }
        ]
        if call_id == "call_1"
            && name.as_deref() == Some("search_workspace")
            && first_delta_id == "call_1"
            && first_delta == "{\"pattern\":\"ph"
            && second_delta_id == "call_1"
            && second_delta == "ase\"}"
            && delta == "done"
    ));
}
