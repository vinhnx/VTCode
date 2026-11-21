use super::*;
use crate::provider::{FinishReason, NativeCallBody, NativeProvider, NativeResponse, ToolCall};
use futures::StreamExt;
use serde_json::{Value, json};
use vtcode_computer_use::{Action, Point};

const MODEL: &str = "claude-sonnet-5";

/// The same logical turn as the streamed fixture, in the non-streaming
/// response shape the Messages API returns.
fn equivalent_response_payload() -> Value {
    json!({
        "content": [
            {"type": "thinking", "thinking": "click then type", "signature": "sig-native"},
            {"type": "text", "text": "acting now"},
            {"type": "tool_use", "id": "tc_1", "name": "type", "input": {"text": "shell"}},
            {"type": "tool_use", "toolset_name": "computer", "id": "cu_1", "name": "left_click",
             "input": {"coordinate": [500, 300]}},
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 11, "output_tokens": 7},
    })
}

fn mixed_turn_sse_frames() -> Vec<String> {
    vec![
        json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [],
               "model": MODEL, "stop_reason": null, "stop_sequence": null, "stop_details": null,
               "usage": {"input_tokens": 11, "output_tokens": 1}}})
            .to_string(),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}})
            .to_string(),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "click then type"}})
            .to_string(),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig-native"}})
            .to_string(),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}).to_string(),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "acting "}}).to_string(),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "now"}}).to_string(),
        json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "tc_1",
               "name": "type", "input": {}}})
            .to_string(),
        json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta",
               "partial_json": "{\"text\":\"sh"}})
            .to_string(),
        json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta",
               "partial_json": "ell\"}"}})
            .to_string(),
        json!({"type": "content_block_start", "index": 3, "content_block": {"type": "tool_use",
               "toolset_name": "computer", "id": "cu_1", "name": "left_click", "input": {}}})
            .to_string(),
        json!({"type": "content_block_delta", "index": 3, "delta": {"type": "input_json_delta",
               "partial_json": "{\"coordinate\":[500,300]}"}})
            .to_string(),
        json!({"type": "content_block_stop", "index": 3}).to_string(),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use", "stop_sequence": null},
               "usage": {"output_tokens": 7}})
            .to_string(),
        json!({"type": "message_stop"}).to_string(),
    ]
}

fn frame(payload: &str) -> String {
    format!("data: {payload}\n\n")
}

async fn pump_chunks(chunks: Vec<Vec<u8>>) -> Result<Vec<NativeStreamEvent>, LLMError> {
    let body = futures::stream::iter(chunks.into_iter().map(Ok::<_, LLMError>));
    let mut stream = pump_native_stream(body, MODEL.to_string(), Some("req_native_1".to_string()), None);
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event?);
    }
    Ok(events)
}

fn assert_completed_mixed_turn(native: &NativeResponse) {
    // Parity by construction: the streamed decode must match the
    // non-streaming parser for the same logical response, including the
    // verbatim assistant blocks.
    let direct = parse_response_native(equivalent_response_payload(), MODEL.to_string()).expect("fixture parses");
    assert_eq!(native.content.as_deref(), direct.content.as_deref());
    assert_eq!(native.reasoning.as_deref(), direct.reasoning.as_deref());
    assert_eq!(native.finish_reason, direct.finish_reason);
    assert_eq!(native.assistant_blocks, direct.assistant_blocks);
    assert_eq!(native.tool_calls.len(), direct.tool_calls.len());
    assert_eq!(native.native_calls.len(), direct.native_calls.len());

    // Exact decoded content, independently derived from the fixtures.
    assert_eq!(native.content.as_deref(), Some("acting now"));
    assert_eq!(native.reasoning.as_deref(), Some("click then type"));
    assert_eq!(native.finish_reason, FinishReason::ToolCalls);
    assert_eq!(native.request_id.as_deref(), Some("req_native_1"));
    let usage = native.usage.as_ref().expect("usage merged from start and delta");
    assert_eq!((usage.prompt_tokens, usage.completion_tokens, usage.total_tokens), (11, 7, 18));

    let expected_calls = [ToolCall::function(
        "tc_1".to_string(),
        "type".to_string(),
        "{\"text\":\"shell\"}".to_string(),
    )];
    assert_eq!(native.tool_calls.len(), 1);
    assert_eq!(native.tool_calls[0].id, expected_calls[0].id);
    assert_eq!(native.tool_calls[0].function.as_ref().unwrap().name, "type");
    assert_eq!(native.tool_calls[0].function.as_ref().unwrap().arguments, "{\"text\":\"shell\"}");

    assert_eq!(native.native_calls.len(), 1);
    assert_eq!(native.native_calls[0].id, "cu_1");
    assert_eq!(native.native_calls[0].provider, NativeProvider::Anthropic);
    assert_eq!(native.native_calls[0].index, 3);
    let NativeCallBody::Anthropic { name, action } = &native.native_calls[0].body else {
        panic!("expected an Anthropic native body");
    };
    assert_eq!(name, "left_click");
    assert!(matches!(
        action.as_ref().unwrap(),
        Action::Click { at: Some(Point { x: 500, y: 300 }), count: 1, .. }
    ));
}

#[tokio::test]
async fn forwards_deltas_and_matches_non_streaming_parse() {
    let frames = mixed_turn_sse_frames();
    let events = pump_chunks(frames.iter().map(|payload| frame(payload).into_bytes()).collect())
        .await
        .expect("stream decodes");

    // Ping and block-stop events stay silent; the deltas and the completion
    // surface in wire order.
    assert_eq!(events.len(), 4, "one reasoning delta, two text deltas, one completion");
    assert!(matches!(&events[0], NativeStreamEvent::ReasoningDelta { delta } if delta == "click then type"));
    assert!(matches!(&events[1], NativeStreamEvent::TextDelta { delta } if delta == "acting "));
    assert!(matches!(&events[2], NativeStreamEvent::TextDelta { delta } if delta == "now"));
    let NativeStreamEvent::Completed(native) = &events[3] else {
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
    let tail = buffer.split_off(buffer.len() / 4);
    let events = pump_chunks(vec![buffer, tail]).await.expect("framed stream decodes");
    assert_eq!(events.len(), 4);
    let NativeStreamEvent::Completed(native) = &events[3] else {
        panic!("expected a completed response");
    };
    assert_completed_mixed_turn(native);
}

#[tokio::test]
async fn error_events_fail_the_stream() {
    let payload = json!({"type": "error", "error": {"type": "overloaded_error", "message": "provider outage"}});
    let error = pump_chunks(vec![frame(&payload.to_string()).into_bytes()])
        .await
        .expect_err("error events must fail the stream");
    assert!(error.to_string().contains("provider outage"), "{error}");
}

#[tokio::test]
async fn stream_without_message_stop_fails() {
    let payload = json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [],
                    "model": MODEL, "stop_reason": null, "stop_sequence": null, "stop_details": null,
                    "usage": {"input_tokens": 11, "output_tokens": 1}}});
    let error = pump_chunks(vec![frame(&payload.to_string()).into_bytes()])
        .await
        .expect_err("a stream without message_stop must fail");
    assert!(error.to_string().contains("without message_stop"), "{error}");
}

#[tokio::test]
async fn delta_for_unknown_block_index_fails_the_stream() {
    // Execution-driving accumulator: a delta for a never-started block is a
    // protocol violation whose silent drop would assemble a truncated or
    // empty tool input. Fail closed and name the offending index. The
    // matching positive — the same delta after its start — decodes in
    // `forwards_deltas_and_matches_non_streaming_parse`.
    let frames = [
        json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [],
               "model": MODEL, "stop_reason": null, "stop_sequence": null, "stop_details": null,
               "usage": {"input_tokens": 11, "output_tokens": 1}}})
        .to_string(),
        json!({"type": "content_block_delta", "index": 5, "delta": {"type": "text_delta", "text": "orphan"}})
            .to_string(),
    ];
    let error = pump_chunks(frames.iter().map(|payload| frame(payload).into_bytes()).collect())
        .await
        .expect_err("a delta without a started block must fail the stream");
    assert!(error.to_string().contains("unknown block index 5"), "{error}");
}

#[tokio::test]
async fn malformed_tool_input_fails_assembly_with_the_call_id() {
    // Truncated tool input must not silently become `{}` and execute a
    // native call with arguments the model never sent. The boundary pair:
    // a tool_use with no input deltas at all is the legitimate empty input
    // and still assembles (covered by the declined-screenshot fixture).
    let frames = [
        json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [],
               "model": MODEL, "stop_reason": null, "stop_sequence": null, "stop_details": null,
               "usage": {"input_tokens": 11, "output_tokens": 1}}})
        .to_string(),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use",
               "toolset_name": "computer", "id": "cu_bad", "name": "left_click", "input": {}}})
        .to_string(),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta",
               "partial_json": "{\"coordinate\":[500,"}})
        .to_string(),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use", "stop_sequence": null}}).to_string(),
        json!({"type": "message_stop"}).to_string(),
    ];
    let error = pump_chunks(frames.iter().map(|payload| frame(payload).into_bytes()).collect())
        .await
        .expect_err("unparseable accumulated tool input must fail the response");
    assert!(error.to_string().contains("cu_bad"), "{error}");
    assert!(error.to_string().contains("input JSON failed to parse"), "{error}");
}

#[tokio::test]
async fn zero_input_tool_use_still_assembles_an_empty_object() {
    // No `input_json_delta` for a tool_use is the zero-input call: the
    // accumulator must keep decoding it as `{}`, not fail like the
    // malformed-JSON case above.
    let frames = [
        json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [],
               "model": MODEL, "stop_reason": null, "stop_sequence": null, "stop_details": null,
               "usage": {"input_tokens": 11, "output_tokens": 1}}})
        .to_string(),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use",
               "toolset_name": "computer", "id": "cu_empty", "name": "screenshot", "input": {}}})
        .to_string(),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use", "stop_sequence": null}}).to_string(),
        json!({"type": "message_stop"}).to_string(),
    ];
    let events = pump_chunks(frames.iter().map(|payload| frame(payload).into_bytes()).collect())
        .await
        .expect("a zero-input native call must decode");
    let NativeStreamEvent::Completed(native) = &events[0] else {
        panic!("expected a completed response");
    };
    assert_eq!(native.native_calls.len(), 1);
    let NativeCallBody::Anthropic { name, action } = &native.native_calls[0].body else {
        panic!("expected an Anthropic native body");
    };
    assert_eq!(name, "screenshot");
    assert!(
        matches!(action.as_ref().expect("zero input decodes"), Action::Screenshot),
        "zero input must decode the bare screenshot action"
    );
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

#[tokio::test]
async fn fallback_boundary_declines_prior_native_calls_like_non_streaming() {
    let frames = [
        json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [],
               "model": MODEL, "stop_reason": null, "stop_sequence": null, "stop_details": null,
               "usage": {"input_tokens": 11, "output_tokens": 1}}})
        .to_string(),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use",
               "toolset_name": "computer", "id": "cu_declined", "name": "screenshot", "input": {}}})
        .to_string(),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "fallback",
               "from": {"model": "a"}, "to": {"model": "b"}}})
        .to_string(),
        json!({"type": "content_block_start", "index": 2, "content_block": {"type": "text", "text": ""}}).to_string(),
        json!({"type": "content_block_delta", "index": 2, "delta": {"type": "text_delta", "text": "continued"}})
            .to_string(),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": null}}).to_string(),
        json!({"type": "message_stop"}).to_string(),
    ];
    let events = pump_chunks(frames.iter().map(|payload| frame(payload).into_bytes()).collect())
        .await
        .expect("stream decodes");

    assert!(matches!(&events[0], NativeStreamEvent::TextDelta { delta } if delta == "continued"));
    let NativeStreamEvent::Completed(native) = &events[1] else {
        panic!("expected a completed response");
    };
    // Parity by construction: the same declined-partials rule the
    // non-streaming parser applies must drop the refused model's native
    // call from both the calls and the replay blocks.
    let direct = parse_response_native(
        json!({
            "content": [
                {"type": "tool_use", "toolset_name": "computer", "id": "cu_declined", "name": "screenshot", "input": {}},
                {"type": "fallback", "from": {"model": "a"}, "to": {"model": "b"}},
                {"type": "text", "text": "continued"},
            ],
            "stop_reason": "end_turn",
        }),
        MODEL.to_string(),
    )
    .expect("fixture parses");
    assert_eq!(native.content.as_deref(), direct.content.as_deref());
    assert_eq!(native.assistant_blocks, direct.assistant_blocks);
    assert!(native.native_calls.is_empty(), "a declined native call must not execute");
    assert_eq!(native.content.as_deref(), Some("continued"));
    assert_eq!(native.assistant_blocks.len(), 2);
    assert_eq!(events.len(), 2, "the declined blocks emit no deltas after the boundary");
}
