use super::*;
use serde_json::json;
use vtcode_computer_use::{Action, Point};

#[test]
fn computer_use_native_parses_mixed_turns_and_keeps_verbatim_items() {
    let response = json!({
        "id": "resp_live",
        "output": [
            {"type": "message", "content": [
                {"type": "output_text", "text": "acting now"},
                {"type": "refusal", "refusal": "no"},
            ]},
            {"type": "reasoning", "summary": [{"type": "summary_text", "text": "planning now"}]},
            {"type": "function_call", "id": "fc_1", "call_id": "call_ord", "name": "shell",
             "arguments": "{\"cmd\":\"ls\"}"},
            {"type": "computer_call", "id": "cua_item", "call_id": "call_17", "status": "completed",
             "actions": [
                {"type": "move", "x": 29, "y": 17},
                {"type": "scroll", "x": 17, "y": 29, "scroll_x": -11, "scroll_y": 7},
                {"type": "type", "text": "payload"},
             ]},
        ],
        "usage": {"input_tokens": 11, "output_tokens": 7, "total_tokens": 18},
    });
    let native = parse_response_native(response.clone(), "gpt-6-luna".to_string()).unwrap();
    assert_eq!(native.content.as_deref(), Some("acting now[Refusal: no]"));
    assert_eq!(native.reasoning.as_deref(), Some("planning now"));
    assert_eq!(native.model, "gpt-6-luna");
    assert_eq!(native.finish_reason, FinishReason::ToolCalls);
    assert_eq!(native.request_id.as_deref(), Some("resp_live"));
    assert_eq!(native.assistant_blocks, response["output"].as_array().unwrap().clone());
    assert_eq!(native.usage.unwrap().total_tokens, 18);

    assert_eq!(native.tool_calls.len(), 1);
    assert_eq!(native.tool_calls[0].id, "call_ord");

    assert_eq!(native.native_calls.len(), 1);
    assert_eq!(native.native_calls[0].id, "call_17");
    assert_eq!(native.native_calls[0].provider, NativeProvider::OpenAi);
    assert_eq!(native.native_calls[0].index, 3);
    let NativeCallBody::OpenAi { replay_item, actions } = &native.native_calls[0].body else {
        panic!("expected an OpenAI native body");
    };
    assert_eq!(replay_item["id"], "cua_item");
    let [
        Action::Move { at: Point { x: 29, y: 17 }, .. },
        Action::ScrollPixels { x: -11, y: 7, .. },
        Action::Type(text),
        Action::Screenshot,
    ] = actions.as_slice()
    else {
        panic!("unexpected action sequence");
    };
    assert_eq!(text, "payload");
}

#[test]
fn computer_use_native_rejects_malformed_incomplete_and_unsafe_batches() {
    for item in [
        json!({"type": "computer_call", "id": "cua_bad", "call_id": "call_18", "status": "completed",
               "actions": [{"type": "screenshot"}, {"type": "private-sentinel"}]}),
        json!({"type": "computer_call", "id": "cua_early", "call_id": "call_19", "status": "in_progress",
               "actions": [{"type": "screenshot"}]}),
        json!({"type": "computer_call", "id": "cua_check", "call_id": "call_20", "status": "completed",
               "actions": [{"type": "screenshot"}], "pending_safety_checks": [{"id": "check_a"}]}),
    ] {
        let response = json!({"output": [item]});
        assert!(parse_response_native(response, "gpt-6-luna".to_string()).is_err());
    }

    let missing = json!({"id": "resp_empty"});
    assert!(parse_response_native(missing, "gpt-6-luna".to_string()).is_err());
    let empty = json!({"output": []});
    assert!(parse_response_native(empty, "gpt-6-luna".to_string()).is_err());
}

#[test]
fn computer_use_native_debug_reveals_routing_only() {
    let response = json!({
        "output": [
            {"type": "computer_call", "id": "cua_item", "call_id": "call_17", "status": "completed",
             "actions": [{"type": "type", "text": "private-typed-text"}]},
        ],
    });
    let native = parse_response_native(response, "gpt-6-luna".to_string()).unwrap();
    assert_eq!(native.finish_reason, FinishReason::ToolCalls);
    let debug = format!("{native:?}");
    assert!(debug.contains("call_17"));
    assert!(debug.contains("openai:2actions"));
    assert!(!debug.contains("private-typed-text"));
}

#[test]
fn computer_use_native_declaration_preserves_function_tools() {
    let mut without_tools = json!({"model": "gpt-6-luna"});
    declare(&mut without_tools);
    assert_eq!(without_tools["tools"], json!([{"type": "computer"}]));

    let mut with_function = json!({"tools": [{"type": "function", "name": "shell"}]});
    declare(&mut with_function);
    assert_eq!(with_function["tools"], json!([{"type": "function", "name": "shell"}, {"type": "computer"}]));
}

#[tokio::test]
async fn computer_use_native_denies_unverified_targets_before_any_io() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::provider::{LLMProvider, LLMRequest, Message};
    use crate::providers::openai::provider::OpenAIProvider;
    use vtcode_config::constants::models;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"output": []})))
        .expect(0)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::new_with_client(
        "test-key".into(),
        None,
        models::openai::GPT_6_LUNA.into(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
        server.uri(),
        vtcode_config::TimeoutsConfig::default(),
    );
    assert!(provider.supports_computer_use());
    let error = LLMProvider::generate_native(
        &provider,
        LLMRequest {
            model: models::openai::GPT_6_LUNA.into(),
            messages: vec![Message::user("fixture".into())].into(),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("not verified"));
    server.verify().await;
}
