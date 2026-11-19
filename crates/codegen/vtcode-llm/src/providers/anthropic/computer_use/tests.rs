use super::*;
use vtcode_computer_use::Screenshot;

#[test]
fn computer_use_anthropic_routes_by_toolset_and_keeps_correlation() {
    assert_eq!(declaration(), json!({"type": "computer_toolset_20260801"}));
    let ordinary = json!({"type": "tool_use", "id": "ordinary", "name": "left_click", "input": {}});
    assert!(decode_call(&ordinary).unwrap().is_none());

    let native = json!({"type": "tool_use", "id": "native", "toolset_name": "computer", "name": "left_click_drag",
        "input": {"start_coordinate": [19, 3], "coordinate": [7, 27], "text": "ctrl+shift"}});
    let call = decode_call(&native).unwrap().unwrap();
    let Ok(Action::Drag { path, modifiers }) = &call.action else {
        panic!("expected drag");
    };
    assert_eq!(path, &[Point { x: 19, y: 3 }, Point { x: 7, y: 27 }]);
    assert_eq!(modifiers, &["ctrl", "shift"]);
    assert_eq!(call.id(), "native");
    assert_eq!(
        call.result(ComputerOutcome::Skipped),
        json!({
            "type": "tool_result", "tool_use_id": "native", "toolset_name": "computer",
            "is_error": true, "content": "Not executed: an earlier computer action in this turn failed."
        })
    );
}

#[test]
fn computer_use_anthropic_returns_images_and_redacts_invalid_inputs() {
    let native =
        json!({"type": "tool_use", "id": "cap", "toolset_name": "computer", "name": "screenshot", "input": {}});
    let call = decode_call(&native).unwrap().unwrap();
    let payload = b"\x89PNG\r\n\x1a\nprivate-payload";
    let observation = Observation::Screenshot(Screenshot::png(payload.to_vec()).unwrap());
    let result = call.result(ComputerOutcome::Executed(&observation));
    assert_eq!(result["tool_use_id"], "cap");
    assert_eq!(result["toolset_name"], "computer");
    let source = &result["content"][0]["source"];
    assert_eq!(source["media_type"], "image/png");
    assert_eq!(STANDARD.decode(source["data"].as_str().unwrap()).unwrap(), payload);

    let malformed = json!({"type": "tool_use", "id": "bad", "toolset_name": "computer", "name": "wait",
        "input": {"duration": "private-sentinel"}});
    let call = decode_call(&malformed).unwrap().unwrap();
    let Err(error) = &call.action else {
        panic!("expected malformed duration");
    };
    assert_eq!(error.to_string(), "invalid Anthropic computer action field duration");
    assert_eq!(call.result(ComputerOutcome::Failed)["tool_use_id"], "bad");

    let excessive = json!({"type": "tool_use", "id": "many", "toolset_name": "computer", "name": "scroll",
        "input": {"scroll_direction": "down", "scroll_amount": 100_001}});
    let call = decode_call(&excessive).unwrap().unwrap();
    let Err(error) = &call.action else {
        panic!("expected excessive scroll to fail");
    };
    assert_eq!(error.to_string(), "invalid computer scroll amount");
}

#[test]
fn computer_use_anthropic_ordinary_parser_rejects_native_calls() {
    use super::super::response_parser::parse_response;
    let ordinary = json!({"content": [
        {"type": "tool_use", "id": "custom", "name": "type", "input": {"text": "ordinary"}}
    ], "stop_reason": "tool_use"});
    let response = parse_response(ordinary, "claude-sonnet-5".into()).unwrap();
    assert_eq!(response.tool_calls.unwrap().len(), 1);

    let native = json!({"content": [
        {"type": "tool_use", "id": "native", "toolset_name": "computer", "name": "type",
         "input": {"text": "private-sentinel"}}
    ], "stop_reason": "tool_use"});
    let error = parse_response(native, "claude-sonnet-5".into()).unwrap_err();
    assert!(error.to_string().contains("requires an active computer-use route"));
    assert!(!error.to_string().contains("private-sentinel"));
}

#[tokio::test]
async fn computer_use_anthropic_execution_keeps_mixed_positions_and_stops_on_failure() {
    use std::sync::{Arc, Mutex};

    use tokio_util::sync::CancellationToken;

    struct Fixture(Arc<Mutex<Vec<String>>>);

    impl Driver for Fixture {
        async fn execute(&mut self, action: &Action, _: &CancellationToken) -> Result<Observation> {
            match action {
                Action::Type(text) => {
                    self.0.lock().unwrap().push(text.clone());
                    Ok(Observation::Done)
                }
                Action::Screenshot => bail!("private driver error"),
                _ => panic!("unexpected driver action"),
            }
        }
    }

    let native = |id, name, input| {
        json!({
            "type":"tool_use", "toolset_name":"computer", "id":id, "name":name, "input": input
        })
    };
    let blocks = [
        native("typed", "type", json!({"text":"first"})),
        json!({"type":"tool_use", "id":"ordinary", "name":"type", "input":{"text":"ordinary"}}),
        native("broken", "screenshot", json!({})),
        native("skipped", "type", json!({"text":"must not execute"})),
    ];
    let actions = Arc::new(Mutex::new(Vec::new()));
    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(Fixture(actions.clone()), &turn);
    let results = execute_batch(&mut control, &blocks, Duration::from_secs(1)).await.unwrap();
    let [(0, typed), (2, broken), (3, skipped)] = results.as_slice() else {
        panic!("ordinary result slot was lost");
    };
    assert_eq!(typed["tool_use_id"], "typed");
    assert_eq!(typed["content"], "OK");
    assert_eq!(broken["tool_use_id"], "broken");
    assert_eq!(broken["content"], "Computer action failed.");
    assert_eq!(skipped["tool_use_id"], "skipped");
    assert_eq!(skipped["content"], NOT_EXECUTED);
    assert_eq!(*actions.lock().unwrap(), ["first"]);

    let retry = [native("retry", "type", json!({"text":"retry"}))];
    let results = execute_batch(&mut control, &retry, Duration::from_secs(1)).await.unwrap();
    assert_eq!(results[0].1["is_error"], true);
    assert_eq!(*actions.lock().unwrap(), ["first"]);

    turn.cancel();
    assert!(execute_batch(&mut control, &retry, Duration::from_secs(1)).await.is_err());
}

#[tokio::test]
async fn computer_use_anthropic_discards_prior_screenshot_when_turn_ends_mid_batch() {
    use tokio_util::sync::CancellationToken;

    struct Fixture(CancellationToken);

    impl Driver for Fixture {
        async fn execute(&mut self, action: &Action, _: &CancellationToken) -> Result<Observation> {
            match action {
                Action::Screenshot => Ok(Observation::Screenshot(
                    Screenshot::png(b"\x89PNG\r\n\x1a\nprivate-observation".to_vec()).unwrap(),
                )),
                Action::CursorPosition => {
                    self.0.cancel();
                    Ok(Observation::CursorPosition(Point { x: 27, y: 9 }))
                }
                _ => panic!("unexpected driver action"),
            }
        }
    }

    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(Fixture(turn.clone()), &turn);
    let blocks = [
        json!({"type":"tool_use","toolset_name":"computer","id":"private","name":"screenshot","input":{}}),
        json!({"type":"tool_use","toolset_name":"computer","id":"cancel","name":"cursor_position","input":{}}),
    ];
    let error = execute_batch(&mut control, &blocks, Duration::from_secs(1)).await.unwrap_err();
    assert_eq!(error.to_string(), "computer turn has ended");
    assert!(turn.is_cancelled());
}

#[tokio::test]
async fn computer_use_anthropic_decoded_batch_keeps_positions_and_refuses_mixed() {
    use std::sync::{Arc, Mutex};

    use tokio_util::sync::CancellationToken;

    use crate::provider::NativeProvider;

    struct Fixture(Arc<Mutex<Vec<String>>>);

    impl Driver for Fixture {
        async fn execute(&mut self, action: &Action, _: &CancellationToken) -> Result<Observation> {
            match action {
                Action::Type(text) => {
                    self.0.lock().unwrap().push(text.clone());
                    Ok(Observation::Done)
                }
                _ => panic!("unexpected driver action"),
            }
        }
    }

    let decoded = |index, id: &str, text: &str| NativeCall {
        id: id.to_string(),
        provider: NativeProvider::Anthropic,
        index,
        body: NativeCallBody::Anthropic {
            name: "type".to_string(),
            action: Ok(Action::Type(text.to_string())),
        },
    };
    let actions = Arc::new(Mutex::new(Vec::new()));
    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(Fixture(actions.clone()), &turn);
    let results = execute_decoded(
        &mut control,
        vec![decoded(1, "first", "one"), decoded(4, "second", "two")],
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    let [(1, first), (4, second)] = results.as_slice() else {
        panic!("decoded positions were lost");
    };
    assert_eq!(first["tool_use_id"], "first");
    assert_eq!(first["content"], "OK");
    assert_eq!(second["tool_use_id"], "second");
    assert_eq!(second["content"], "OK");
    assert_eq!(*actions.lock().unwrap(), ["one", "two"]);

    let mixed = vec![NativeCall {
        id: "foreign".to_string(),
        provider: NativeProvider::OpenAi,
        index: 0,
        body: NativeCallBody::OpenAi { replay_item: json!({}), actions: Vec::new() },
    }];
    let error = execute_decoded(&mut control, mixed, Duration::from_secs(1)).await.unwrap_err();
    assert_eq!(error.to_string(), "mixed-provider computer batch");
    assert_eq!(*actions.lock().unwrap(), ["one", "two"]);

    let dupes = vec![decoded(0, "same", "dup-one"), decoded(2, "same", "dup-two")];
    let error = execute_decoded(&mut control, dupes, Duration::from_secs(1)).await.unwrap_err();
    assert!(error.to_string().contains("duplicate"));
    assert_eq!(*actions.lock().unwrap(), ["one", "two"]);
}
