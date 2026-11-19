use super::*;
use serde_json::json;
use vtcode_computer_use::{Action, Point};

#[test]
fn computer_use_native_parses_mixed_turns_and_keeps_verbatim_blocks() {
    let response = json!({
        "content": [
            {"type": "text", "text": "acting now"},
            {"type": "thinking", "thinking": "click then type", "signature": "sig-private"},
            {"type": "tool_use", "id": "ordinary", "name": "type", "input": {"text": "shell"}},
            {"type": "tool_use", "id": "native_click", "toolset_name": "computer", "name": "left_click",
             "input": {"coordinate": [500, 300]}},
            {"type": "tool_use", "id": "native_type", "toolset_name": "computer", "name": "type",
             "input": {"text": "payload"}},
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 11, "output_tokens": 7},
    });
    let native = parse_response_native(response.clone(), "claude-sonnet-5".to_string()).unwrap();
    assert_eq!(native.content.as_deref(), Some("acting now"));
    assert_eq!(native.reasoning.as_deref(), Some("click then type"));
    assert_eq!(native.model, "claude-sonnet-5");
    assert_eq!(native.finish_reason, FinishReason::ToolCalls);
    assert_eq!(native.assistant_blocks, response["content"].as_array().unwrap().clone());

    assert_eq!(native.tool_calls.len(), 1);
    assert_eq!(native.tool_calls[0].id, "ordinary");

    assert_eq!(native.native_calls.len(), 2);
    assert_eq!(native.native_calls[0].id, "native_click");
    assert_eq!(native.native_calls[0].provider, NativeProvider::Anthropic);
    let NativeCallBody::Anthropic { name, action } = &native.native_calls[0].body else {
        panic!("expected an Anthropic native body");
    };
    assert_eq!(name, "left_click");
    assert!(matches!(
        action.as_ref().unwrap(),
        Action::Click { at: Some(Point { x: 500, y: 300 }), count: 1, .. }
    ));
    let NativeCallBody::Anthropic { action, .. } = &native.native_calls[1].body else {
        panic!("expected an Anthropic native body");
    };
    assert!(matches!(action.as_ref().unwrap(), Action::Type(text) if text == "payload"));
}

#[test]
fn computer_use_native_retains_correlation_for_malformed_actions() {
    let response = json!({
        "content": [
            {"type": "tool_use", "id": "bad", "toolset_name": "computer", "name": "wait",
             "input": {"duration": "private-sentinel"}},
        ],
        "stop_reason": "tool_use",
    });
    let native = parse_response_native(response, "claude-sonnet-5".to_string()).unwrap();
    assert_eq!(native.native_calls.len(), 1);
    assert_eq!(native.native_calls[0].id, "bad");
    let NativeCallBody::Anthropic { action, .. } = &native.native_calls[0].body else {
        panic!("expected an Anthropic native body");
    };
    let Err(message) = action else {
        panic!("expected a retained decode failure");
    };
    assert_eq!(message, "invalid Anthropic computer action field duration");
}

#[test]
fn computer_use_native_debug_reveals_routing_only() {
    let response = json!({
        "content": [
            {"type": "text", "text": "private-narrative"},
            {"type": "tool_use", "id": "native", "toolset_name": "computer", "name": "type",
             "input": {"text": "private-typed-text"}},
        ],
        "stop_reason": "tool_use",
    });
    let native = parse_response_native(response, "claude-sonnet-5".to_string()).unwrap();
    let debug = format!("{native:?}");
    assert!(debug.contains("native"));
    assert!(!debug.contains("private-narrative"));
    assert!(!debug.contains("private-typed-text"));
}

#[test]
fn computer_use_native_drops_declined_partials_from_calls_and_replay() {
    let response = json!({
        "content": [
            {"type": "tool_use", "id": "declined", "toolset_name": "computer", "name": "screenshot", "input": {}},
            {"type": "fallback", "from": {"model": "a"}, "to": {"model": "b"}},
            {"type": "text", "text": "continued"},
        ],
        "stop_reason": "end_turn",
    });
    let native = parse_response_native(response, "claude-sonnet-5".to_string()).unwrap();
    assert!(native.native_calls.is_empty());
    assert_eq!(native.content.as_deref(), Some("continued"));
    assert_eq!(native.assistant_blocks.len(), 2);
    assert!(native.assistant_blocks.iter().all(|block| block["id"] != "declined"));
}

#[test]
fn computer_use_native_rejects_invalid_call_identity() {
    let response = json!({
        "content": [
            {"type": "tool_use", "id": "", "toolset_name": "computer", "name": "screenshot", "input": {}},
        ],
        "stop_reason": "tool_use",
    });
    let error = parse_response_native(response, "claude-sonnet-5".to_string()).unwrap_err();
    assert!(error.to_string().contains("Invalid Anthropic computer call"));

    let missing = json!({"stop_reason": "end_turn"});
    assert!(parse_response_native(missing, "claude-sonnet-5".to_string()).is_err());
}

#[test]
fn computer_use_native_declaration_preserves_function_tools() {
    let mut without_tools = json!({"model": "claude-sonnet-5"});
    declare(&mut without_tools);
    assert_eq!(without_tools["tools"], json!([{"type": "computer_toolset_20260801"}]));

    let mut with_function = json!({"tools": [{"type": "function", "name": "shell"}]});
    declare(&mut with_function);
    assert_eq!(
        with_function["tools"],
        json!([{"type": "function", "name": "shell"}, {"type": "computer_toolset_20260801"}])
    );
}

#[tokio::test]
async fn computer_use_native_denies_unverified_targets_before_any_io() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::provider::{LLMProvider, LLMRequest, Message};
    use crate::providers::anthropic::provider::AnthropicProvider;
    use vtcode_config::constants::models;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"content": []})))
        .expect(0)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::new_with_client(
        "test-key".into(),
        models::CLAUDE_SONNET_5.into(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    assert!(provider.supports_computer_use());
    let error = LLMProvider::generate_native(
        &provider,
        LLMRequest {
            model: models::CLAUDE_SONNET_5.into(),
            messages: vec![Message::user("fixture".into())].into(),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("not verified"));
    server.verify().await;
}
