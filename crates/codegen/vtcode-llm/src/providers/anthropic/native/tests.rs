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
    assert_eq!((native.native_calls[0].index, native.native_calls[1].index), (3, 4));
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
async fn computer_use_native_parse_and_execute_agree_on_actions() {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;
    use vtcode_computer_use::{Driver, Observation, TurnControl};

    use super::super::computer_use::execute_batch;

    struct RecordingDriver(Arc<Mutex<Vec<String>>>);

    impl Driver for RecordingDriver {
        async fn execute(&mut self, action: &Action, _: &CancellationToken) -> anyhow::Result<Observation> {
            let tag = match action {
                Action::Click { at: Some(point), .. } => format!("click:{},{}", point.x, point.y),
                Action::Type(text) => format!("type:{text}"),
                _ => panic!("unexpected driver action"),
            };
            self.0.lock().unwrap().push(tag);
            Ok(Observation::Done)
        }
    }

    let response = json!({
        "content": [
            {"type": "text", "text": "acting now"},
            {"type": "tool_use", "id": "ordinary", "name": "shell", "input": {}},
            {"type": "tool_use", "id": "native_click", "toolset_name": "computer", "name": "left_click",
             "input": {"coordinate": [19, 3]}},
            {"type": "tool_use", "id": "native_type", "toolset_name": "computer", "name": "type",
             "input": {"text": "second"}},
        ],
        "stop_reason": "tool_use",
    });
    let native = parse_response_native(response, "claude-sonnet-5".to_string()).unwrap();
    assert_eq!(native.native_calls.len(), 2);

    let recorded = Arc::new(Mutex::new(Vec::new()));
    let mut control = TurnControl::after_approval(RecordingDriver(recorded.clone()), &CancellationToken::new());
    let results = execute_batch(&mut control, &native.assistant_blocks, Duration::from_secs(5))
        .await
        .unwrap();
    let [(first, _), (second, _)] = results.as_slice() else {
        panic!("expected two native results");
    };
    assert_eq!((*first, *second), (2, 3));
    assert_eq!(*recorded.lock().unwrap(), ["click:19,3", "type:second"]);
}

#[tokio::test]
async fn computer_use_native_accepts_arbitrary_model_and_endpoint() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::provider::{LLMProvider, LLMRequest, Message};
    use crate::providers::anthropic::provider::AnthropicProvider;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "acting now"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // Anthropic has no probe-matrix entry at all: a declared Anthropic route
    // must dispatch the native turn for any model on any endpoint.
    let model = "claude-future-1";
    let provider = AnthropicProvider::new_with_client(
        "test-key".into(),
        model.into(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    assert!(provider.supports_computer_use());
    let response = LLMProvider::generate_native(
        &provider,
        LLMRequest {
            model: model.into(),
            messages: vec![Message::user("fixture".into())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("arbitrary model on a declared route must be admitted");
    assert_eq!(response.content.as_deref(), Some("acting now"));
    server.verify().await;
}
