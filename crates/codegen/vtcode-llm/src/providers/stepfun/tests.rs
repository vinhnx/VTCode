use super::StepFunProvider;
use crate::provider::{LLMProvider, LLMRequest, Message, NormalizedStreamEvent, ToolCall, ToolChoice, ToolDefinition};
use futures::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::models;
use vtcode_config::types::ReasoningEffortLevel;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

fn build_payload(request: &LLMRequest) -> Value {
    let provider = StepFunProvider::new("test-key".to_string());
    provider.build_payload(request, request.stream).expect("payload should build")
}

fn mock_provider(base_url: &str) -> StepFunProvider {
    let http_client = reqwest::Client::builder().no_proxy().build().expect("test client should build");
    StepFunProvider::new_with_client(
        "test-key".to_string(),
        models::stepfun::STEP_3_7_FLASH.to_string(),
        http_client,
        base_url.to_string(),
        TimeoutsConfig::default(),
    )
}

use crate::providers::test_support::start_mock_server_or_skip;

fn completed_response_body() -> Value {
    json!({
        "id": "resp_step_1",
        "object": "response",
        "created_at": 1772624997,
        "completed_at": 1772624998,
        "model": models::stepfun::STEP_3_7_FLASH,
        "status": "completed",
        "error": null,
        "incomplete_details": null,
        "output": [
            {
                "type": "reasoning",
                "id": "rs_1",
                "summary": [],
                "content": null,
                "encrypted_content": null,
                "status": null
            },
            {
                "type": "message",
                "id": "msg_1",
                "status": "completed",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "hello from stepfun", "annotations": []}]
            },
            {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "get_weather",
                "arguments": "{\"city\":\"Beijing\"}",
                "status": "completed"
            }
        ],
        "usage": {
            "input_tokens": 14,
            "input_tokens_details": {"cached_tokens": 0},
            "output_tokens": 52,
            "output_tokens_details": {"reasoning_tokens": 0, "tool_output_tokens": 0},
            "total_tokens": 66
        }
    })
}

#[tokio::test]
async fn generate_posts_responses_request_and_parses_output() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_partial_json(json!({
            "model": models::stepfun::STEP_3_7_FLASH,
            "instructions": "system guidance",
            "stream": false,
            "max_output_tokens": 512,
            "reasoning": {"effort": "high"},
            "tool_choice": "auto",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "hello"}]}],
            "tools": [{"type": "function", "name": "get_weather"}]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(completed_response_body()))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_provider(&format!("{}/v1", server.uri()));
    let response = provider
        .generate(LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            system_prompt: Some(Arc::from("system guidance")),
            max_tokens: Some(512),
            reasoning_effort: Some(ReasoningEffortLevel::XHigh),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get the weather".to_string(),
                json!({"type": "object"}),
            )])),
            tool_choice: Some(ToolChoice::Auto),
            ..Default::default()
        })
        .await
        .expect("generate should succeed");

    assert_eq!(response.content.as_deref(), Some("hello from stepfun"));
    assert_eq!(response.request_id.as_deref(), Some("resp_step_1"));
    assert_eq!(response.finish_reason, crate::provider::FinishReason::ToolCalls);
    let tool_calls = response.tool_calls.expect("tool calls should exist");
    assert_eq!(tool_calls[0].id, "call_1");
    assert_eq!(tool_calls[0].function.as_ref().map(|f| f.name.as_str()), Some("get_weather"));
    let usage = response.usage.expect("usage should exist");
    assert_eq!(usage.prompt_tokens, 14);
    assert_eq!(usage.completion_tokens, 52);
    assert_eq!(usage.total_tokens, 66);
}

#[tokio::test]
async fn generate_surfaces_provider_error_body() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {"code": "invalid_request", "message": "bad input"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_provider(&format!("{}/v1", server.uri()));
    let error = provider
        .generate(LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect_err("HTTP 400 should fail");

    assert!(error.to_string().contains("StepFun"), "error should name the provider: {error}");
    assert!(error.to_string().contains("400"), "error should include the status: {error}");
}

#[tokio::test]
async fn stream_normalized_tolerates_stepfun_reasoning_boundaries() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };

    // Mirrors the documented StepFun SSE shape, including the non-summary
    // `response.reasoning_part.added`/`.done` boundaries.
    let sse_body = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_step\",\"object\":\"response\",\"created_at\":1772624997,\"model\":\"step-3.7-flash\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"sequence_number\":1,\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[],\"content\":null,\"encrypted_content\":null,\"status\":\"in_progress\"}}\n\n",
        "event: response.reasoning_part.added\n",
        "data: {\"type\":\"response.reasoning_part.added\",\"sequence_number\":2,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"part\":{\"type\":\"reasoning_text\",\"text\":\"\"}}\n\n",
        "event: response.reasoning_text.delta\n",
        "data: {\"type\":\"response.reasoning_text.delta\",\"sequence_number\":3,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"delta\":\"thinking\"}\n\n",
        "event: response.reasoning_text.done\n",
        "data: {\"type\":\"response.reasoning_text.done\",\"sequence_number\":4,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"text\":\"thinking\"}\n\n",
        "event: response.reasoning_part.done\n",
        "data: {\"type\":\"response.reasoning_part.done\",\"sequence_number\":5,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"part\":{\"type\":\"reasoning_text\",\"text\":\"thinking\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"sequence_number\":6,\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[],\"content\":null,\"encrypted_content\":null,\"status\":\"completed\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"sequence_number\":7,\"output_index\":1,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"in_progress\",\"content\":[]}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"sequence_number\":8,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":9,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"sequence_number\":10,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"text\":\"Hello\"}\n\n",
        "event: response.content_part.done\n",
        "data: {\"type\":\"response.content_part.done\",\"sequence_number\":11,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"Hello\",\"annotations\":[]}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"sequence_number\":12,\"output_index\":1,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\",\"annotations\":[]}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"sequence_number\":13,\"response\":{\"id\":\"resp_step\",\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"total_tokens\":12}}}\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_provider(&format!("{}/v1", server.uri()));
    let mut stream = provider
        .stream_normalized(LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .await
        .expect("stream_normalized should start");

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut done = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event should not error") {
            NormalizedStreamEvent::TextDelta { delta } => text.push_str(&delta),
            NormalizedStreamEvent::ReasoningDelta { delta, .. } => reasoning.push_str(&delta),
            NormalizedStreamEvent::Done { response } => {
                done = Some(response);
                break;
            }
            _ => {}
        }
    }

    assert_eq!(text, "Hello");
    assert_eq!(reasoning, "thinking");
    let response = done.expect("stream should complete");
    assert_eq!(response.content.as_deref(), Some("Hello"));
    assert_eq!(response.request_id.as_deref(), Some("resp_step"));
    assert_eq!(response.usage.map(|usage| usage.total_tokens), Some(12));
}

#[test]
fn step_5_preview_uses_1m_context_and_reasoning_effort() {
    let provider = StepFunProvider::new("test-key".to_string());
    assert_eq!(provider.effective_context_size(models::stepfun::STEP_5_PREVIEW), 1_048_576);
    assert_eq!(provider.effective_context_size(models::stepfun::STEP_3_7_FLASH), 262_144);
    assert!(provider.supports_reasoning(models::stepfun::STEP_5_PREVIEW));
    assert!(provider.supports_reasoning_effort(models::stepfun::STEP_5_PREVIEW));
    assert!(provider.supports_vision(models::stepfun::STEP_5_PREVIEW));
}

#[test]
fn payload_maps_reasoning_effort_and_suppresses_sampling() {
    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        reasoning_effort: Some(ReasoningEffortLevel::XHigh),
        temperature: Some(0.5),
        ..Default::default()
    });

    assert_eq!(payload["reasoning"]["effort"], "high");
    assert!(payload.get("temperature").is_none());
    assert!(payload.get("top_p").is_none());
}

#[test]
fn unknown_effort_omits_reasoning_but_still_suppresses_sampling() {
    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        temperature: Some(0.5),
        reasoning_effort: Some(ReasoningEffortLevel::Unknown),
        ..Default::default()
    });

    assert!(payload.get("reasoning").is_none());
    assert!(payload.get("temperature").is_none());

    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        temperature: Some(0.5),
        reasoning_effort: Some(ReasoningEffortLevel::Low),
        ..Default::default()
    });

    assert_eq!(payload["reasoning"]["effort"], "low");
    assert!(payload.get("temperature").is_none());
}

#[test]
fn golden_payload_basic_shape() {
    use crate::provider::ToolChoice;

    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        system_prompt: Some(Arc::from("system guidance")),
        max_tokens: Some(512),
        temperature: Some(0.5),
        top_p: Some(0.25),
        stream: true,
        tool_choice: Some(ToolChoice::Auto),
        metadata: Some(json!({"user_id": "user-42"})),
        ..Default::default()
    });

    assert_eq!(payload["model"], models::stepfun::STEP_3_7_FLASH);
    assert_eq!(payload["instructions"], "system guidance");
    let input = payload["input"].as_array().unwrap();
    assert_eq!(input.len(), 1);
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[0]["content"][0]["type"], "input_text");
    assert_eq!(input[0]["content"][0]["text"], "hello");
    assert_eq!(payload["max_output_tokens"], 512);
    assert_eq!(payload["temperature"], 0.5);
    assert_eq!(payload["top_p"], 0.25);
    assert_eq!(payload["stream"], true);
    // StepFun does not accept user metadata or chat-completions fields.
    assert!(payload.get("messages").is_none());
    assert!(payload.get("user_id").is_none());
    assert!(payload.get("max_tokens").is_none());
}

#[test]
fn payload_inlines_vision_parts_as_input_images() {
    use crate::provider::ContentPart;

    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![Message {
            role: crate::provider::MessageRole::User,
            content: crate::provider::MessageContent::Parts(vec![
                ContentPart::Text { text: "describe".to_string() },
                ContentPart::Image {
                    data: "abc".to_string(),
                    mime_type: "image/png".to_string(),
                    content_type: "image".to_string(),
                    detail: None,
                    image_url: None,
                },
            ]),
            ..Default::default()
        }]
        .into(),
        ..Default::default()
    });

    let content = payload["input"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "input_text");
    assert_eq!(content[1]["type"], "input_image");
    assert_eq!(content[1]["image_url"], "data:image/png;base64,abc");
}

#[test]
fn payload_flattens_tools_into_responses_shape() {
    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Get the weather".to_string(),
            json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}),
        )])),
        tool_choice: Some(ToolChoice::Auto),
        ..Default::default()
    });

    let tool = &payload["tools"][0];
    assert_eq!(tool["type"], "function");
    assert_eq!(tool["name"], "get_weather");
    assert_eq!(tool["description"], "Get the weather");
    assert!(tool.get("function").is_none(), "Responses tools are flat, not nested");
    assert_eq!(payload["tool_choice"], "auto");
}

#[test]
fn payload_omits_callable_tools_for_tool_choice_none() {
    for stream in [false, true] {
        let mut request = LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get the weather".to_string(),
                json!({"type": "object"}),
            )])),
            tool_choice: Some(ToolChoice::None),
            stream,
            ..Default::default()
        };
        let payload = build_payload(&request);
        assert!(payload.get("tools").is_none());
        assert!(payload.get("tool_choice").is_none());
        assert_eq!(payload["stream"], stream);
        request.tool_choice = Some(ToolChoice::Auto);
        let enabled = build_payload(&request);
        assert_eq!(enabled["tools"][0]["name"], "get_weather");
        assert_eq!(enabled["tool_choice"], "auto");
    }
}

#[test]
fn payload_replays_tool_calls_and_outputs() {
    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![
            Message::user("weather?".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "get_weather".to_string(),
                    "{\"city\":\"Beijing\"}".to_string(),
                )],
            ),
            Message::tool_response("call_1".to_string(), "{\"temperature\":22}".to_string()),
        ]
        .into(),
        ..Default::default()
    });

    let input = payload["input"].as_array().unwrap();
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[1]["type"], "function_call");
    assert_eq!(input[1]["call_id"], "call_1");
    assert_eq!(input[1]["name"], "get_weather");
    assert_eq!(input[1]["arguments"], "{\"city\":\"Beijing\"}");
    assert_eq!(input[2]["type"], "function_call_output");
    assert_eq!(input[2]["call_id"], "call_1");
    assert_eq!(input[2]["output"], "{\"temperature\":22}");
}

#[test]
fn payload_synthesizes_aborted_output_for_orphan_call() {
    let payload = build_payload(&LLMRequest {
        model: models::stepfun::STEP_3_7_FLASH.to_string(),
        messages: vec![
            Message::user("weather?".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_orphan".to_string(),
                    "get_weather".to_string(),
                    "{}".to_string(),
                )],
            ),
        ]
        .into(),
        ..Default::default()
    });

    let input = payload["input"].as_array().unwrap();
    assert_eq!(input[1]["type"], "function_call");
    assert_eq!(input[2]["type"], "function_call_output");
    assert_eq!(input[2]["call_id"], "call_orphan");
    assert_eq!(input[2]["output"], "aborted");
}

#[test]
fn parse_response_extracts_reasoning_text_and_tool_calls() {
    let response = json!({
        "id": "resp_1",
        "status": "completed",
        "output": [
            {"type": "reasoning", "id": "rs_1", "summary": [], "content": null, "encrypted_content": null},
            {"type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
             "content": [{"type": "output_text", "text": "done", "annotations": []}]},
            {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "get_weather",
             "arguments": "{\"city\":\"Beijing\"}", "status": "completed"}
        ],
        "usage": {
            "input_tokens": 14,
            "output_tokens": 52,
            "total_tokens": 66,
            "input_tokens_details": {"cached_tokens": 0},
            "output_tokens_details": {"reasoning_tokens": 0, "tool_output_tokens": 0}
        }
    });

    let parsed = StepFunProvider::parse_response(response, models::stepfun::STEP_3_7_FLASH.to_string())
        .expect("response should parse");

    assert_eq!(parsed.content.as_deref(), Some("done"));
    assert_eq!(parsed.request_id.as_deref(), Some("resp_1"));
    let tool_calls = parsed.tool_calls.expect("tool calls should exist");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call_1");
    assert_eq!(tool_calls[0].function.as_ref().map(|f| f.name.as_str()), Some("get_weather"));
    assert_eq!(parsed.finish_reason, crate::provider::FinishReason::ToolCalls);
    let usage = parsed.usage.expect("usage should exist");
    assert_eq!(usage.prompt_tokens, 14);
    assert_eq!(usage.completion_tokens, 52);
    assert_eq!(usage.total_tokens, 66);
}

#[test]
fn parse_response_maps_incomplete_status_to_length() {
    let response = json!({
        "id": "resp_incomplete",
        "status": "incomplete",
        "incomplete_details": {"reason": "max_output_tokens"},
        "output": [
            {"type": "reasoning", "id": "rs_1", "summary": [], "content": "thinking", "encrypted_content": null}
        ]
    });

    let parsed = StepFunProvider::parse_response(response, models::stepfun::STEP_3_7_FLASH.to_string())
        .expect("response should parse");
    assert_eq!(parsed.finish_reason, crate::provider::FinishReason::Length);
    assert_eq!(parsed.reasoning.as_deref(), Some("thinking"));
}

#[test]
fn parse_response_surfaces_failed_status() {
    let response = json!({
        "id": "resp_failed",
        "status": "failed",
        "error": {"code": "server_error", "message": "backend failed"},
        "output": []
    });

    let error = StepFunProvider::parse_response(response, models::stepfun::STEP_3_7_FLASH.to_string())
        .expect_err("failed response should error");
    assert!(error.to_string().contains("backend failed"));
}

#[test]
fn validate_request_rejects_unknown_models() {
    let provider = StepFunProvider::new("test-key".to_string());
    let error = provider
        .validate_request(&LLMRequest {
            model: "not-a-stepfun-model".to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        })
        .expect_err("unknown model should be rejected");
    assert!(error.to_string().contains("not-a-stepfun-model"));
}
