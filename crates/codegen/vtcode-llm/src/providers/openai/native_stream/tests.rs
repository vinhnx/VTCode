use super::*;
use crate::provider::{FinishReason, NativeCallBody, NativeProvider, NativeResponse};
use futures::StreamExt;
use serde_json::{Value, json};
use vtcode_computer_use::{Action, MouseButton, Point};

const MODEL: &str = "gpt-6-luna";

/// One complete bound-route turn: reasoning, text, an ordinary function
/// call, and an ordered computer batch. Mirrors the live gpt-6-luna shape.
fn completed_response_payload() -> Value {
    json!({
        "type": "response.completed",
        "sequence_number": 9,
        "response": {
            "id": "resp_native_1",
            "object": "response",
            "created_at": 1,
            "status": "completed",
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "max_output_tokens": null,
            "model": MODEL,
            "usage": {"input_tokens": 11, "output_tokens": 7, "total_tokens": 18},
            "output": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "click then type"}]},
                {"type": "message", "content": [{"type": "output_text", "text": "acting now"}]},
                {"type": "function_call", "id": "fc_1", "call_id": "call_ordinary", "name": "type",
                 "arguments": "{\"text\":\"shell\"}"},
                {"type": "computer_call", "call_id": "cu_click", "status": "completed",
                 "actions": [{"type": "click", "x": 500, "y": 300, "button": "left"}, {"type": "screenshot"}]},
                {"type": "computer_call", "call_id": "cu_type", "status": "completed",
                 "actions": [{"type": "type", "text": "hello"}, {"type": "screenshot"}]}
            ],
            "tools": []
        }
    })
}

fn mixed_turn_sse_frames() -> Vec<String> {
    vec![
        json!({"type": "response.created", "sequence_number": 0, "response": {}}).to_string(),
        json!({"type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "output_index": 0,
               "summary_index": 0, "sequence_number": 1, "delta": "click then "})
        .to_string(),
        json!({"type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "output_index": 0,
               "summary_index": 0, "sequence_number": 2, "delta": "type"})
        .to_string(),
        json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 1,
               "content_index": 0, "sequence_number": 3, "delta": "acting "})
        .to_string(),
        json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 1,
               "content_index": 0, "sequence_number": 4, "delta": "now"})
        .to_string(),
        // A rig-unknown computer item rides output_item.done; the bound
        // consumer must neither reject it nor emit an event for it.
        json!({"type": "response.output_item.done", "sequence_number": 5, "item_id": "cu_click", "output_index": 3,
               "item": {"type": "computer_call", "call_id": "cu_click", "status": "completed",
                        "actions": [{"type": "click", "x": 500, "y": 300, "button": "left"}, {"type": "screenshot"}]}})
        .to_string(),
        completed_response_payload().to_string(),
    ]
}

fn frame(payload: &str) -> String {
    format!("data: {payload}\n\n")
}

async fn pump_chunks(chunks: Vec<Vec<u8>>) -> Result<Vec<NativeStreamEvent>, LLMError> {
    let body = futures::stream::iter(chunks.into_iter().map(Ok::<_, LLMError>));
    let mut stream = pump_native_stream(body, MODEL.to_string());
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event?);
    }
    Ok(events)
}

fn assert_completed_mixed_turn(native: &NativeResponse) {
    // Parity by construction: every shared field must match what the
    // non-streaming parser returns for the same response object.
    let direct = parse_response_native(completed_response_payload()["response"].clone(), MODEL.to_string())
        .expect("fixture parses");
    assert_eq!(native.content.as_deref(), direct.content.as_deref());
    assert_eq!(native.reasoning.as_deref(), direct.reasoning.as_deref());
    assert_eq!(native.finish_reason, direct.finish_reason);
    assert_eq!(native.request_id, direct.request_id);
    assert_eq!(native.tool_calls.len(), direct.tool_calls.len());
    assert_eq!(native.native_calls.len(), direct.native_calls.len());
    assert_eq!(native.assistant_blocks.len(), direct.assistant_blocks.len());

    // Exact decoded content, independently derived from the fixture.
    assert_eq!(native.content.as_deref(), Some("acting now"));
    assert_eq!(native.reasoning.as_deref(), Some("click then type"));
    assert_eq!(native.finish_reason, FinishReason::ToolCalls);
    assert_eq!(native.request_id.as_deref(), Some("resp_native_1"));
    let usage = native.usage.as_ref().expect("fixture carries usage");
    assert_eq!((usage.prompt_tokens, usage.completion_tokens, usage.total_tokens), (11, 7, 18));

    assert_eq!(native.tool_calls.len(), 1);
    assert_eq!(native.tool_calls[0].id, "call_ordinary");

    assert_eq!(native.native_calls.len(), 2);
    assert_eq!(native.native_calls[0].provider, NativeProvider::OpenAi);
    assert_eq!((native.native_calls[0].index, native.native_calls[1].index), (3, 4));
    assert_eq!(native.native_calls[0].id, "cu_click");
    assert_eq!(native.native_calls[1].id, "cu_type");

    let NativeCallBody::OpenAi { replay_item, actions } = &native.native_calls[0].body else {
        panic!("expected an OpenAI native body");
    };
    assert_eq!(replay_item, &completed_response_payload()["response"]["output"][3]);
    assert!(matches!(
        actions.as_slice(),
        [
            Action::Click {
                at: Some(Point { x: 500, y: 300 }),
                button: MouseButton::Left,
                count: 1,
                ..
            },
            Action::Screenshot
        ]
    ));
    let NativeCallBody::OpenAi { replay_item, actions } = &native.native_calls[1].body else {
        panic!("expected an OpenAI native body");
    };
    assert_eq!(replay_item, &completed_response_payload()["response"]["output"][4]);
    assert!(matches!(actions.as_slice(), [Action::Type(text), Action::Screenshot] if text == "hello"));
}

#[tokio::test]
async fn forwards_deltas_and_decodes_completed_mixed_turn() {
    let frames = mixed_turn_sse_frames();
    let events = pump_chunks(frames.iter().map(|payload| frame(payload).into_bytes()).collect())
        .await
        .expect("stream decodes");

    // Lifecycle and rig-gap events stay silent; the four deltas and the
    // completion surface in wire order.
    assert_eq!(events.len(), 5, "two reasoning deltas, two text deltas, one completion");
    assert!(matches!(&events[0], NativeStreamEvent::ReasoningDelta { delta } if delta == "click then "));
    assert!(matches!(&events[1], NativeStreamEvent::ReasoningDelta { delta } if delta == "type"));
    assert!(matches!(&events[2], NativeStreamEvent::TextDelta { delta } if delta == "acting "));
    assert!(matches!(&events[3], NativeStreamEvent::TextDelta { delta } if delta == "now"));
    let NativeStreamEvent::Completed(native) = &events[4] else {
        panic!("expected a completed response");
    };
    assert_completed_mixed_turn(native);
}

#[tokio::test]
async fn frames_split_across_chunks_still_decode() {
    let mut buffer = mixed_turn_sse_frames()
        .iter()
        .map(|payload| frame(payload))
        .collect::<String>()
        .into_bytes();
    let tail = buffer.split_off(buffer.len() / 3);
    let events = pump_chunks(vec![buffer, tail]).await.expect("framed stream decodes");
    assert_eq!(events.len(), 5);
    let NativeStreamEvent::Completed(native) = &events[4] else {
        panic!("expected a completed response");
    };
    assert_completed_mixed_turn(native);
}

#[tokio::test]
async fn error_events_fail_the_stream() {
    let payload = json!({"type": "error", "sequence_number": 2,
                         "error": {"code": "server_error", "message": "provider outage"}, "param": null});
    let error = pump_chunks(vec![frame(&payload.to_string()).into_bytes()])
        .await
        .expect_err("error events must fail the stream");
    assert!(error.to_string().contains("provider outage"), "{error}");
}

#[tokio::test]
async fn stream_without_completion_fails() {
    let payload = json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0,
                         "content_index": 0, "sequence_number": 1, "delta": "partial"});
    let error = pump_chunks(vec![frame(&payload.to_string()).into_bytes()])
        .await
        .expect_err("a stream without response.completed must fail");
    assert!(error.to_string().contains("without a completed response"), "{error}");
}

#[tokio::test]
async fn unterminated_frame_fails_the_tail_bound() {
    use crate::providers::shared::MAX_SSE_TAIL_BYTES;

    let payload = vec![b'a'; MAX_SSE_TAIL_BYTES + 1];
    let error = pump_chunks(vec![payload])
        .await
        .expect_err("a peer that never emits a boundary must fail closed");
    assert!(error.to_string().contains("without an event boundary"), "{error}");
}
