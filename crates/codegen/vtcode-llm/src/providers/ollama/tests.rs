use super::*;
use crate::provider::{ContentPart, Message, MessageContent};
use serde_json::json;
use vtcode_config::types::ReasoningEffortLevel;

fn test_provider() -> OllamaProvider {
    OllamaProvider::from_config(
        None,
        Some("test-model".to_string()),
        Some("http://localhost".to_string()),
        None,
        None,
        None,
        None,
    )
}

#[test]
fn convert_tool_calls_fabricates_unique_ids_when_missing() {
    let make_call = |name: &str| OllamaResponseToolCall {
        call_type: Some("function".to_string()),
        function: Some(OllamaResponseFunctionCall {
            name: Some(name.to_string()),
            arguments: Some(json!({})),
            index: None,
        }),
    };

    let first = OllamaProvider::convert_tool_calls(Some(vec![make_call("foo"), make_call("bar")]))
        .expect("conversion should succeed")
        .expect("calls expected");
    let second = OllamaProvider::convert_tool_calls(Some(vec![make_call("foo")]))
        .expect("conversion should succeed")
        .expect("calls expected");

    let ids: Vec<&str> = first.iter().chain(second.iter()).map(|call| call.id.as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "fabricated ids must be unique within and across responses");
    for id in ids {
        assert!(id.starts_with("call_"));
    }
}

#[test]
fn build_payload_includes_images() {
    let provider = test_provider();
    let parts = vec![
        ContentPart::text("see ".to_string()),
        ContentPart::image("BASE64DATA".to_string(), "image/png".to_string()),
    ];
    let request = LLMRequest {
        model: "test-model".to_string(),
        messages: vec![Message::user_with_parts(parts)].into(),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();
    assert_eq!(payload.messages.len(), 1);
    let message = &payload.messages[0];
    assert_eq!(message.content.as_deref(), Some("see "));
    assert_eq!(message.images.as_ref(), Some(&vec!["BASE64DATA".to_string()]));
}

#[test]
fn build_payload_omits_images_when_none_present() {
    let provider = test_provider();
    let content = MessageContent::text("no images".to_string());
    let request = LLMRequest {
        model: "test-model".to_string(),
        messages: vec![Message::user(content.as_text().into_owned())].into(),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();
    assert_eq!(payload.messages.len(), 1);
    let message = &payload.messages[0];
    assert_eq!(message.content.as_deref(), Some("no images"));
    assert!(message.images.is_none());
}

#[test]
fn build_payload_omits_callable_tools_for_tool_choice_none() {
    let provider = test_provider();
    let request = LLMRequest {
        model: "test-model".to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_history".to_owned(),
                    "get_weather".to_owned(),
                    "{}".to_owned(),
                )],
            ),
            Message::tool_response("call_history".to_owned(), "sunny".to_owned()),
            Message::user("Synthesize the result".to_owned()),
        ]
        .into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Get the weather".to_string(),
            json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}),
        )])),
        tool_choice: Some(ToolChoice::None),
        ..Default::default()
    };

    for stream in [false, true] {
        let payload = serde_json::to_value(provider.build_payload(&request, stream).unwrap()).unwrap();
        assert!(payload.get("tools").is_none(), "tool-free synthesis must not advertise callable tools");
        assert_eq!(payload["stream"], stream);
        assert_eq!(payload["messages"][0]["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(payload["messages"][1]["tool_call_id"], "call_history");
        assert_eq!(payload["messages"][1]["content"], "sunny");
        let mut enabled = request.clone();
        enabled.tool_choice = Some(ToolChoice::Auto);
        let payload = serde_json::to_value(provider.build_payload(&enabled, stream).unwrap()).unwrap();
        assert_eq!(payload["tools"][0]["function"]["name"], "get_weather");
    }
}

#[test]
fn build_payload_minimax_tool_followup_omits_tool_call_id() {
    let provider = test_provider();
    let tool_call_id = "direct_exec_command_1".to_string();
    let request = LLMRequest {
        model: models::ollama::MINIMAX_M3_CLOUD.to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    tool_call_id.clone(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo fmt\"}".to_string(),
                )],
            ),
            Message::tool_response(tool_call_id, "{\"output\":\"\",\"exit_code\":0}".to_string()),
        ]
        .into(),
        reasoning_effort: Some(ReasoningEffortLevel::Low),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();
    assert_eq!(payload.messages.len(), 2);
    assert_eq!(payload.messages[1].role, "tool");
    assert_eq!(payload.messages[1].tool_name.as_deref(), Some("exec_command"));
    assert!(payload.messages[1].tool_call_id.is_none());
    assert!(payload.think.is_none());
}

#[test]
fn build_payload_non_minimax_tool_followup_keeps_tool_call_id() {
    let provider = test_provider();
    let tool_call_id = "direct_exec_command_1".to_string();
    let request = LLMRequest {
        model: models::ollama::GPT_OSS_20B_CLOUD.to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    tool_call_id.clone(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo fmt\"}".to_string(),
                )],
            ),
            Message::tool_response(tool_call_id.clone(), "{\"output\":\"\",\"exit_code\":0}".to_string()),
        ]
        .into(),
        reasoning_effort: Some(ReasoningEffortLevel::Low),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();
    assert_eq!(payload.messages.len(), 2);
    assert_eq!(payload.messages[1].role, "tool");
    assert_eq!(payload.messages[1].tool_name.as_deref(), Some("exec_command"));
    assert_eq!(payload.messages[1].tool_call_id.as_deref(), Some(tool_call_id.as_str()));
    assert_eq!(payload.think, Some(Value::String("low".to_string())));
}

#[test]
fn build_payload_hoists_history_system_directives_into_system_prompt() {
    let provider = test_provider();
    let request = LLMRequest {
        model: models::ollama::MINIMAX_M3_CLOUD.to_string(),
        system_prompt: Some(std::sync::Arc::from(
            "stable system instructions",
        )),
        messages: vec![
            Message::user("explore architecture".to_string()),
            Message::system(
                "Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration.".to_string(),
            ),
        ].into(),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();
    assert_eq!(payload.messages.len(), 2);
    assert_eq!(payload.messages[0].role, "system");
    assert!(
        payload.messages[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("stable system instructions")
    );
    assert!(
        payload.messages[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("[History Directives]")
    );
    assert!(
        payload.messages[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("Previous turn already completed tool execution")
    );
    assert_eq!(payload.messages[1].role, "user");
    assert_eq!(payload.messages[1].content.as_deref(), Some("explore architecture"));
}

#[test]
fn build_payload_promotes_history_system_directive_without_base_system_prompt() {
    let provider = test_provider();
    let request = LLMRequest {
        model: models::ollama::MINIMAX_M3_CLOUD.to_string(),
        messages: vec![
            Message::system(
                "Repeated read-only exploration hit the per-turn family cap. Scheduling a final recovery pass without more tools.".to_string(),
            ),
            Message::user("summarize the architecture".to_string()),
        ].into(),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();
    assert_eq!(payload.messages.len(), 2);
    assert_eq!(payload.messages[0].role, "system");
    assert!(
        payload.messages[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("[History Directives]")
    );
    assert!(
        payload.messages[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("Repeated read-only exploration hit the per-turn family cap")
    );
    assert_eq!(payload.messages[1].role, "user");
}

#[test]
fn build_payload_recovers_balanced_prefix_from_malformed_history_tool_arguments() {
    let provider = test_provider();
    let request = LLMRequest {
        model: "test-model".to_string(),
        messages: vec![Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                "tool_call_0".to_string(),
                "apply_patch".to_string(),
                "{\"action\":\"read\",\"path\":\"docs/ARCHITECTURE.md\",\"offset\":1,\"limit\":100}{\"action\":\"read\",\"path\":\"README.md\"}"
                    .to_string(),
            )],
        )].into(),
        ..Default::default()
    };

    let payload = provider
        .build_payload(&request, false)
        .expect("payload should recover malformed history tool arguments");

    let tool_calls = payload.messages[0].tool_calls.as_ref().expect("tool calls should be present");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(
        tool_calls[0].function.arguments,
        Some(json!({
            "action": "read",
            "path": "docs/ARCHITECTURE.md",
            "offset": 1,
            "limit": 100
        }))
    );
}

#[test]
fn build_payload_rehydrates_glm_interleaved_history_into_content() {
    let provider = test_provider();
    let request = LLMRequest {
        model: models::ollama::GLM_5_3_CLOUD.to_string(),
        messages: vec![Message::assistant("done".to_string()).with_reasoning(Some("trace".to_string()))].into(),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();

    assert_eq!(payload.messages[0].content.as_deref(), Some("<think>trace</think>done"));
    assert!(payload.messages[0].thinking.is_none());
}

#[test]
fn build_payload_replays_assistant_reasoning_as_ollama_thinking() {
    let provider = test_provider();
    let request = LLMRequest {
        model: models::ollama::GPT_OSS_20B.to_string(),
        messages: vec![
            Message::assistant("need a tool".to_string()).with_reasoning(Some("reasoning trace".to_string())),
        ]
        .into(),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();

    assert_eq!(payload.messages[0].content.as_deref(), Some("need a tool"));
    assert_eq!(payload.messages[0].thinking.as_deref(), Some("reasoning trace"));
}

#[test]
fn build_payload_includes_apply_patch_as_normal_tool() {
    let provider = test_provider();
    let request = LLMRequest {
        model: "test-model".to_string(),
        messages: vec![Message::user("patch this file".to_string())].into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition::apply_patch("Apply VT Code patches".to_string())])),
        ..Default::default()
    };

    let payload = provider.build_payload(&request, false).unwrap();
    let tools = payload.tools.expect("tools should be present");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_name(), "apply_patch");
}

#[test]
fn reasoning_only_responses_never_promote_private_content_in_either_wire_mode() {
    for (content, native_thinking, expected_public, expected_reasoning) in [
        (None, Some("native private reasoning"), None, "native private reasoning"),
        (Some("<think>private reasoning</think>"), None, None, "private reasoning"),
        (
            Some("<think>private reasoning</think>Public final answer"),
            None,
            Some("Public final answer"),
            "private reasoning",
        ),
        (
            Some("<think>tagged private reasoning</think>"),
            Some("native private reasoning"),
            None,
            "native private reasoning",
        ),
    ] {
        let parsed: OllamaChatResponse = serde_json::from_value(json!({
            "message": {"role": "assistant", "content": content, "thinking": native_thinking},
            "done": true, "done_reason": "stop",
        }))
        .expect("Ollama response fixture");
        let response =
            OllamaProvider::response_from_chat_payload("test-model".into(), parsed).expect("nonstream response");
        assert_eq!(response.content.as_deref(), expected_public);
        assert_eq!(response.reasoning.as_deref(), Some(expected_reasoning));

        let mut aggregator = crate::providers::shared::StreamAggregator::new("test-model".into());
        if let Some(thinking) = native_thinking {
            aggregator.handle_reasoning(thinking);
        }
        let mut rendered_public = String::new();
        for chunk in content.unwrap_or_default().as_bytes().chunks(7) {
            for event in aggregator.handle_content(std::str::from_utf8(chunk).expect("ASCII fixture")) {
                if let LLMStreamEvent::Token { delta } = event {
                    rendered_public.push_str(&delta);
                }
            }
        }
        let mut streamed = aggregator.finalize();
        OllamaProvider::normalize_response_reasoning(&mut streamed);
        assert_eq!(streamed.content.as_deref(), expected_public);
        assert_eq!(streamed.reasoning.as_deref(), Some(expected_reasoning));
        assert_eq!(rendered_public.trim(), expected_public.unwrap_or_default());
        assert!(!rendered_public.contains("private reasoning"));
    }
}

#[tokio::test]
async fn streaming_completion_keeps_tagged_reasoning_out_of_public_content() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};
    let Some(server) = crate::providers::test_support::start_mock_server_or_skip().await else {
        return;
    };
    let provider =
        OllamaProvider::from_config(None, Some("test-model:cloud".into()), Some(server.uri()), None, None, None, None);
    for public_tail in ["", "Public final answer"] {
        let frames = [
            json!({"message": {"role": "assistant", "content": "<thi"}, "done": false}),
            json!({"message": {"role": "assistant", "content": "nk>private reasoning</think>"}, "done": false}),
            json!({"message": {"role": "assistant", "content": public_tail}, "done": true, "done_reason": "stop"}),
        ];
        let body = frames.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n") + "\n";
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/x-ndjson"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        let mut stream = LLMProvider::stream(
            &provider,
            LLMRequest {
                model: "test-model:cloud".into(),
                messages: vec![Message::user("Synthesize saved evidence".into())].into(),
                tool_choice: Some(ToolChoice::None),
                ..Default::default()
            },
        )
        .await
        .expect("mock Ollama stream");
        let mut public_tokens = String::new();
        let mut completed = None;
        while let Some(event) = stream.next().await {
            match event.expect("Ollama streaming event") {
                LLMStreamEvent::Token { delta } => public_tokens.push_str(&delta),
                LLMStreamEvent::Completed { response } => completed = Some(response),
                _ => {}
            }
        }
        assert_eq!(public_tokens, public_tail);
        let response = completed.expect("terminal completed response");
        assert_eq!(response.content.as_deref(), (!public_tail.is_empty()).then_some(public_tail));
        assert_eq!(response.reasoning.as_deref(), Some("private reasoning"));
    }
}

#[tokio::test]
async fn tool_free_stream_failure_does_not_dispatch_an_uncharged_nonstream_fallback() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn drain_request(socket: &mut tokio::net::TcpStream) {
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = socket.read(&mut chunk).await.expect("read mock HTTP request");
            assert!(read > 0, "request must contain complete headers and body");
            request.extend_from_slice(&chunk[..read]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&request[..end]).expect("HTTP headers");
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("content length"))
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + content_length {
                    return;
                }
            }
        }
    }

    for choice in [ToolChoice::None, ToolChoice::Auto] {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("mock transport listener");
        let address = listener.local_addr().expect("listener address");
        let requests = std::sync::Arc::new(AtomicUsize::new(0));
        let captured = requests.clone();
        let server = tokio::spawn(async move {
            for index in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("accept mock request");
                drain_request(&mut socket).await;
                captured.fetch_add(1, Ordering::SeqCst);
                if index == 0 {
                    // Successful headers followed by a truncated body fail
                    // only when the stream polls its first body chunk.
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n").await.expect("write truncated response");
                } else {
                    let body = r#"{"message":{"role":"assistant","content":"Fallback final"},"done":true,"done_reason":"stop"}"#;
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(header.as_bytes()).await.expect("write fallback headers");
                    socket.write_all(body.as_bytes()).await.expect("write fallback body");
                }
                socket.shutdown().await.expect("finish mock response");
            }
        });
        let provider = OllamaProvider::from_config(
            None,
            Some("test-model:cloud".into()),
            Some(format!("http://{address}")),
            None,
            None,
            None,
            None,
        );
        let mut stream = LLMProvider::stream(
            &provider,
            LLMRequest {
                model: "test-model:cloud".into(),
                messages: vec![Message::user("Synthesize saved evidence".into())].into(),
                tool_choice: Some(choice.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("successful mock response headers");
        let mut failed = false;
        let mut completed = None;
        while let Some(event) = stream.next().await {
            match event {
                Err(_) => failed = true,
                Ok(LLMStreamEvent::Completed { response }) => completed = Some(response),
                _ => {}
            }
        }
        server.abort();
        let _ = server.await;
        if matches!(choice, ToolChoice::None) {
            assert!(failed && completed.is_none());
            assert_eq!(requests.load(Ordering::SeqCst), 1, "recovery must return failure to charged runloop admission");
        } else {
            assert!(!failed);
            assert_eq!(completed.expect("enabled fallback response").content.as_deref(), Some("Fallback final"));
            assert_eq!(requests.load(Ordering::SeqCst), 2, "ordinary requests retain the existing transport fallback");
        }
    }
}

#[test]
fn response_payload_preserves_reasoning_details() {
    let parsed = OllamaChatResponse {
        message: Some(OllamaResponseMessage {
            role: Some("assistant".to_string()),
            content: Some("answer".to_string()),
            thinking: None,
            reasoning_details: Some(vec![json!({
                "type": "reasoning.text",
                "text": "step one"
            })]),
            tool_calls: None,
        }),
        done: true,
        done_reason: Some("stop".to_string()),
        prompt_eval_count: Some(1),
        eval_count: Some(2),
        error: None,
    };

    let response =
        OllamaProvider::response_from_chat_payload("test-model".to_string(), parsed).expect("response should parse");
    assert_eq!(response.reasoning.as_deref(), Some("step one"));
    assert!(response.reasoning_details.is_some());

    let first_detail = response
        .reasoning_details
        .as_ref()
        .and_then(|details| details.first())
        .expect("reasoning detail should exist");
    let parsed_detail: Value = serde_json::from_str(first_detail).expect("reasoning detail should be json");
    assert_eq!(parsed_detail["type"], "reasoning.text");
}

#[test]
fn tags_response_accepts_partial_model_summaries() {
    let parsed: OllamaTagsResponse = serde_json::from_value(json!({
        "models": [
            { "model": "qwen3:8b" }
        ]
    }))
    .expect("partial model summaries should parse");

    let names: Vec<String> = parsed
        .models
        .into_iter()
        .filter_map(|model| model.name.or(model.model))
        .collect();
    assert_eq!(names, vec!["qwen3:8b".to_string()]);
}
