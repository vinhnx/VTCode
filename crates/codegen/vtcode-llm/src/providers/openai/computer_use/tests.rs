use super::*;

#[test]
fn computer_use_openai_keeps_item_identity_and_orders_actions() {
    assert_eq!(declaration(), json!({"type": "computer"}));
    let ordinary = json!({"type": "function_call", "name": "computer", "call_id": "ordinary"});
    assert!(decode_call(&ordinary).unwrap().is_none());

    let native = json!({"type":"computer_call","id":"cu_item","call_id":"call_17","status":"completed","actions":[
        {"type":"move","x":29,"y":17,"keys":["SHIFT"]},
        {"type":"scroll","x":17,"y":29,"scroll_x":-11,"scroll_y":7},
        {"type":"type","text":"payload"}
    ]});
    let call = decode_call(&native).unwrap().unwrap();
    assert_eq!(call.id(), "call_17");
    assert_eq!(call.replay_item()["id"], "cu_item");
    let [
        Action::Move { at: Point { x: 29, y: 17 }, modifiers },
        Action::ScrollPixels { at: Point { x: 17, y: 29 }, x: -11, y: 7, .. },
        Action::Type(text),
        Action::Screenshot,
    ] = call.actions.as_slice()
    else {
        panic!("unexpected action sequence");
    };
    assert_eq!(modifiers, &["SHIFT"]);
    assert_eq!(text, "payload");

    let bytes = b"\x89PNG\r\n\x1a\nwire-fixture";
    let result = call.result(&Screenshot::png(bytes.to_vec()).unwrap());
    assert_eq!(result["call_id"], "call_17");
    assert_eq!(result["type"], "computer_call_output");
    assert_eq!(result["output"]["type"], "computer_screenshot");
    let data = result["output"]["image_url"]
        .as_str()
        .unwrap()
        .strip_prefix("data:image/png;base64,")
        .unwrap();
    assert_eq!(STANDARD.decode(data).unwrap(), bytes);
}

#[test]
fn computer_use_openai_rejects_unapproved_checks_and_entire_malformed_batches() {
    let blocked = json!({"type":"computer_call","call_id":"call_17","status":"completed",
        "actions":[{"type":"screenshot"}],"pending_safety_checks":[{"id":"check_a"}]});
    assert!(decode_call(&blocked).is_err());

    let malformed = json!({"type":"computer_call","call_id":"call_18","status":"completed",
        "actions":[{"type":"screenshot"},{"type":"private-sentinel"}]});
    let Err(error) = decode_call(&malformed) else {
        panic!("expected invalid batch");
    };
    assert_eq!(error.to_string(), "invalid OpenAI computer action");
}

#[test]
fn computer_use_openai_ordinary_parser_rejects_private_items() {
    use super::super::responses_api::parse_responses_payload;
    for item in [
        json!({"type":"computer_call", "call_id":"call_1", "actions":[{"type":"type","text":"private-sentinel"}]}),
        json!({"type":"computer_call_output", "call_id":"call_2", "output":{"image_url":"private-sentinel"}}),
    ] {
        let err = parse_responses_payload(json!({"output":[item]}), "gpt-6.2-sol".into(), false).unwrap_err();
        assert!(err.to_string().contains("requires an active computer-use route"));
        assert!(!err.to_string().contains("private-sentinel"));
    }
}

#[tokio::test]
async fn computer_use_openai_execution_revokes_when_the_final_image_is_missing() {
    use tokio_util::sync::CancellationToken;

    struct Fixture;

    impl Driver for Fixture {
        async fn execute(&mut self, _: &Action, _: &CancellationToken) -> Result<Observation> {
            Ok(Observation::Done)
        }
    }

    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(Fixture, &turn);
    let item = json!({"type":"computer_call","call_id":"first","status":"completed",
        "actions":[{"type":"type","text":"private"}]});
    let call = decode_call(&item).unwrap().unwrap();
    let error = execute_call(&mut control, &call, Duration::from_secs(1)).await.unwrap_err();
    assert_eq!(error.to_string(), "OpenAI computer batch did not return its final screenshot");

    let item = json!({"type":"computer_call","call_id":"second","status":"completed",
        "actions":[{"type":"screenshot"}]});
    let call = decode_call(&item).unwrap().unwrap();
    let error = execute_call(&mut control, &call, Duration::from_secs(1)).await.unwrap_err();
    assert_eq!(error.to_string(), "computer control has ended");
    assert!(!turn.is_cancelled());
}
