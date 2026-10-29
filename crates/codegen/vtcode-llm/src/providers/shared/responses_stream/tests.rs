use super::{ResponsesNormalizedStreamOptions, ResponsesNormalizedStreamProcessor, provider_error};
use crate::provider::{FinishReason, LLMResponse, NormalizedStreamEvent, ReasoningSource, ToolCall};
use serde_json::{Value, json};

fn options() -> ResponsesNormalizedStreamOptions {
    ResponsesNormalizedStreamOptions {
        provider_name: "TestProvider",
        model: "gpt-5".to_string(),
        emit_reasoning: true,
        include_cached_prompt_metrics: false,
    }
}

#[test]
fn normalized_stream_rejects_exhausted_tool_call_index() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
    let error = processor
        .handle_payload(json!({
            "type":"response.function_call_arguments.delta", "call_id":"too-large",
            "item_id":"fc-overflow", "sequence_number":1,
            "output_index":usize::MAX, "delta":"{"
        }))
        .unwrap_err();
    assert!(error.to_string().contains("tool call index space exhausted"), "{error}");
    assert!(error.to_string().contains("TestProvider"));
}

fn parse_response(value: Value) -> Result<LLMResponse, crate::provider::LLMError> {
    let content = value
        .get("output")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("content"))
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    Ok(LLMResponse {
        content,
        model: "gpt-5".to_string(),
        finish_reason: FinishReason::Stop,
        ..Default::default()
    })
}

fn response_fixture(status: &str, output: Value, usage: Value) -> Value {
    json!({
        "id": "resp_test",
        "object": "response",
        "created_at": 1,
        "status": status,
        "error": null,
        "incomplete_details": null,
        "instructions": null,
        "max_output_tokens": null,
        "model": "gpt-5",
        "usage": usage,
        "output": output,
        "tools": []
    })
}

fn completed_response_fixture(output: Value) -> Value {
    response_fixture("completed", output, Value::Null)
}

fn text_delta_fixture(delta: &str) -> Value {
    json!({
        "type": "response.output_text.delta",
        "item_id": "msg_1",
        "output_index": 0,
        "content_index": 0,
        "sequence_number": 1,
        "delta": delta
    })
}

fn refusal_delta_fixture(delta: &str) -> Value {
    json!({
        "type": "response.refusal.delta",
        "item_id": "msg_1",
        "output_index": 0,
        "content_index": 0,
        "sequence_number": 1,
        "delta": delta
    })
}

#[test]
fn reasoning_events_preserve_public_summary_classification() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

    let summary_events = processor
        .handle_payload(json!({
            "type": "response.reasoning_summary_text.delta",
            "item_id": "rs_1",
            "output_index": 0,
            "summary_index": 0,
            "sequence_number": 1,
            "delta": "public summary"
        }))
        .expect("summary event should parse");
    assert!(matches!(
        summary_events.as_slice(),
        [NormalizedStreamEvent::ReasoningDelta { delta, source }]
            if delta == "public summary" && *source == ReasoningSource::ProviderSummary
    ));

    let continuation_events = processor
        .handle_payload(json!({
            "type": "response.reasoning_text.delta",
            "item_id": "rs_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "delta": "continuation detail"
        }))
        .expect("continuation event should parse");
    assert!(matches!(
        continuation_events.as_slice(),
        [NormalizedStreamEvent::ReasoningDelta { delta, source }]
            if delta == "continuation detail" && *source == ReasoningSource::Continuation
    ));
}

#[test]
fn hidden_reasoning_is_retained_when_public_emission_is_disabled() {
    let mut options = options();
    options.emit_reasoning = false;
    let mut processor = ResponsesNormalizedStreamProcessor::new(options, parse_response);

    let events = processor
        .handle_payload(json!({
            "type": "response.reasoning_text.delta",
            "item_id": "rs_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 1,
            "delta": "continuation detail"
        }))
        .expect("continuation event should parse");
    assert!(events.is_empty(), "continuation details must remain hidden");

    let finished = processor.finish().expect("processor should retain hidden reasoning");
    assert!(matches!(
        finished.as_slice(),
        [NormalizedStreamEvent::Done { response }]
            if response.reasoning.as_deref() == Some("continuation detail")
    ));
}

#[test]
fn text_delta_and_completed_yield_text_then_done() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

    let events = processor
        .handle_payload(text_delta_fixture("hello"))
        .expect("text delta should parse");
    assert!(matches!(
        events.as_slice(),
        [NormalizedStreamEvent::TextDelta { delta }] if delta == "hello"
    ));

    let completed_events = processor
        .handle_payload(json!({
            "type": "response.completed",
            "sequence_number": 2,
            "response": completed_response_fixture(json!([{
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "hello"}]
                }]))
        }))
        .expect("completed event should parse");
    assert!(completed_events.is_empty());

    let finished = processor.finish().expect("finish should succeed");
    assert!(matches!(
        finished.as_slice(),
        [NormalizedStreamEvent::Done { response }]
            if response.content.as_deref() == Some("hello")
    ));
}

#[test]
fn empty_final_response_uses_streamed_text_and_preserves_metadata() {
    let mut options = options();
    options.include_cached_prompt_metrics = true;
    let mut processor = ResponsesNormalizedStreamProcessor::new(options, |value| {
        let output = value
            .get("output")
            .and_then(Value::as_array)
            .ok_or_else(|| provider_error("TestProvider", "missing output"))?;
        if output.is_empty() {
            return Err(provider_error("TestProvider", "No output in response"));
        }
        parse_response(value)
    });

    processor
        .handle_payload(text_delta_fixture("streamed answer"))
        .expect("text delta should parse");
    processor
        .handle_payload(json!({
            "type": "response.completed",
            "sequence_number": 2,
            "response": {
                "id": "resp_streamed",
                "object": "response",
                "created_at": 1,
                "status": "completed",
                "error": null,
                "incomplete_details": null,
                "instructions": null,
                "max_output_tokens": null,
                "model": "gpt-5",
                "usage": {
                    "input_tokens": 11,
                    "output_tokens": 7,
                    "total_tokens": 18,
                    "input_tokens_details": {
                        "cached_tokens": 5
                    }
                },
                "output": [],
                "tools": []
            }
        }))
        .expect("completed event should parse");

    let finished = processor.finish().expect("finish should succeed");
    let [
        NormalizedStreamEvent::Usage { usage },
        NormalizedStreamEvent::Done { response },
    ] = finished.as_slice()
    else {
        panic!("expected usage then done");
    };
    assert_eq!(usage.prompt_tokens, 11);
    assert_eq!(usage.completion_tokens, 7);
    assert_eq!(usage.total_tokens, 18);
    assert_eq!(usage.cached_prompt_tokens, Some(5));
    assert_eq!(response.content.as_deref(), Some("streamed answer"));
    assert_eq!(response.request_id.as_deref(), Some("resp_streamed"));
}

#[test]
fn tool_call_deltas_emit_start_and_finish_with_assembled_tool_call() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |_| {
        Ok(LLMResponse { model: "gpt-5".to_string(), ..Default::default() })
    });

    let started = processor
        .handle_payload(json!({
            "type": "response.output_item.added",
            "item_id": "call_1",
            "output_index": 0,
            "sequence_number": 1,
            "item": {
                "type": "function_call",
                "id": "call_1",
                "call_id": "call_1",
                "name": "search_workspace",
                "arguments": "",
                "status": "in_progress"
            }
        }))
        .expect("output item metadata should parse");
    assert!(matches!(
        started.as_slice(),
        [NormalizedStreamEvent::ToolCallStart { call_id, name }]
            if call_id == "call_1" && name.as_deref() == Some("search_workspace")
    ));

    let first = processor
        .handle_payload(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "call_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "delta": "{\"query\":\"vt"
        }))
        .expect("first tool delta should parse");
    assert!(matches!(
        first.as_slice(),
        [NormalizedStreamEvent::ToolCallDelta { call_id: delta_call_id, delta }]
        if delta_call_id == "call_1"
            && delta == "{\"query\":\"vt"
    ));

    let second = processor
        .handle_payload(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "call_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 3,
            "delta": "code\"}"
        }))
        .expect("second tool delta should parse");
    assert!(matches!(
        second.as_slice(),
        [NormalizedStreamEvent::ToolCallDelta { call_id, delta }]
            if call_id == "call_1" && delta == "code\"}"
    ));

    let finished = processor.finish().expect("finish should succeed");
    let response = match finished.as_slice() {
        [NormalizedStreamEvent::Done { response }] => response,
        _ => panic!("expected done event"),
    };
    let tool_calls = response.tool_calls.as_ref().expect("tool call should be assembled");
    assert_eq!(
        tool_calls,
        &vec![ToolCall::function(
            "call_1".to_string(),
            "search_workspace".to_string(),
            "{\"query\":\"vtcode\"}".to_string(),
        )]
    );
}

#[test]
fn tool_call_delta_without_ids_fabricates_and_reuses_id() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |_| {
        Ok(LLMResponse { model: "gpt-5".to_string(), ..Default::default() })
    });

    let delta_payload = |fragment: &str| {
        json!({
            "type": "response.function_call_arguments.delta",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 1,
            "delta": fragment
        })
    };

    let first = processor
        .handle_payload(delta_payload("{\"query\":"))
        .expect("first tool delta should parse");
    let first_id = match first.as_slice() {
        [
            NormalizedStreamEvent::ToolCallStart { call_id, .. },
            NormalizedStreamEvent::ToolCallDelta { call_id: delta_id, .. },
        ] => {
            assert_eq!(call_id, delta_id);
            call_id.clone()
        }
        other => panic!("unexpected events: {other:?}"),
    };
    assert!(first_id.starts_with("call_"));

    let second = processor
        .handle_payload(delta_payload("\"vtcode\"}"))
        .expect("second tool delta should parse");
    assert!(matches!(
        second.as_slice(),
        [NormalizedStreamEvent::ToolCallDelta { call_id, .. }] if *call_id == first_id
    ));
}

#[test]
fn tool_call_delta_without_ids_fabricates_distinct_ids_per_processor() {
    let make_id = || {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |_| {
            Ok(LLMResponse { model: "gpt-5".to_string(), ..Default::default() })
        });
        let events = processor
            .handle_payload(json!({
                "type": "response.function_call_arguments.delta",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 1,
                "delta": "{}"
            }))
            .expect("tool delta should parse");
        match events.as_slice() {
            [NormalizedStreamEvent::ToolCallStart { call_id, .. }, ..] => call_id.clone(),
            other => panic!("unexpected events: {other:?}"),
        }
    };

    assert_ne!(make_id(), make_id(), "fabricated ids must differ across responses");
}

#[test]
fn tool_call_deltas_use_provider_call_id_when_item_id_differs() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |_| {
        Ok(LLMResponse { model: "gpt-5".to_string(), ..Default::default() })
    });

    let started = processor
        .handle_payload(json!({
            "type": "response.output_item.added",
            "item_id": "fc_1",
            "output_index": 0,
            "sequence_number": 1,
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "search_workspace",
                "arguments": "",
                "status": "in_progress"
            }
        }))
        .expect("output item metadata should parse");
    assert!(matches!(
        started.as_slice(),
        [NormalizedStreamEvent::ToolCallStart { call_id, name }]
            if call_id == "call_1" && name.as_deref() == Some("search_workspace")
    ));

    let first = processor
        .handle_payload(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "delta": "{\"query\":\"vt"
        }))
        .expect("first tool delta should parse");
    assert!(matches!(
        first.as_slice(),
        [NormalizedStreamEvent::ToolCallDelta { call_id, delta }]
            if call_id == "call_1" && delta == "{\"query\":\"vt"
    ));

    processor
        .handle_payload(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 3,
            "delta": "code\"}"
        }))
        .expect("second tool delta should parse");

    let finished = processor.finish().expect("finish should succeed");
    let response = match finished.as_slice() {
        [NormalizedStreamEvent::Done { response }] => response,
        _ => panic!("expected done event"),
    };
    let tool_calls = response.tool_calls.as_ref().expect("tool call should be assembled");
    assert_eq!(
        tool_calls,
        &vec![ToolCall::function(
            "call_1".to_string(),
            "search_workspace".to_string(),
            "{\"query\":\"vtcode\"}".to_string(),
        )]
    );
}

#[test]
fn custom_tool_input_stream_events_wait_for_completed_response_replay() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |value| {
        let custom_call = value
            .get("output")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .expect("custom tool output should exist");

        Ok(LLMResponse {
            model: "gpt-5".to_string(),
            finish_reason: FinishReason::ToolCalls,
            tool_calls: Some(vec![ToolCall::custom(
                custom_call
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                custom_call.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                custom_call.get("input").and_then(Value::as_str).unwrap_or_default().to_string(),
            )]),
            ..Default::default()
        })
    });

    let delta_events = processor
        .handle_payload(json!({
            "type": "response.custom_tool_call_input.delta",
            "sequence_number": 1,
            "item_id": "ct_1",
            "call_id": "call_patch_1",
            "output_index": 0,
            "delta": "*** Begin"
        }))
        .expect("custom tool input delta should parse");
    assert!(
        delta_events.is_empty(),
        "custom input deltas are preserved internally but are not runtime dispatch events"
    );

    let done_events = processor
        .handle_payload(json!({
            "type": "response.custom_tool_call_input.done",
            "sequence_number": 2,
            "item_id": "ct_1",
            "call_id": "call_patch_1",
            "output_index": 0,
            "input": "*** Begin Patch\n*** End Patch\n"
        }))
        .expect("custom tool input done should parse");
    assert!(done_events.is_empty(), "custom tool dispatch remains owned by response.completed replay");

    let completed_events = processor
        .handle_payload(json!({
            "type": "response.completed",
            "sequence_number": 3,
            "response": completed_response_fixture(json!([{
                "type": "custom_tool_call",
                "id": "ct_1",
                "call_id": "call_patch_1",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** End Patch\n"
            }]))
        }))
        .expect("completed event should parse");
    assert!(completed_events.is_empty());

    let finished = processor.finish().expect("finish should succeed");
    let response = match finished.as_slice() {
        [NormalizedStreamEvent::Done { response }] => response,
        _ => panic!("expected final done event"),
    };
    let tool_calls = response
        .tool_calls
        .as_ref()
        .expect("custom tool call should be replayed from final response");
    assert_eq!(tool_calls.len(), 1);
    assert!(tool_calls[0].is_custom());
    assert_eq!(tool_calls[0].id, "call_patch_1");
    assert_eq!(tool_calls[0].tool_name(), Some("apply_patch"));
    assert_eq!(tool_calls[0].raw_input(), Some("*** Begin Patch\n*** End Patch\n"));
}

#[test]
fn refusal_delta_streams_visible_output() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

    let events = processor
        .handle_payload(refusal_delta_fixture("I can't help with that"))
        .expect("refusal delta should parse");
    assert!(matches!(
        events.as_slice(),
        [NormalizedStreamEvent::TextDelta { delta }]
            if delta == "I can't help with that"
    ));

    let finished = processor.finish().expect("finish should succeed");
    assert!(matches!(
        finished.as_slice(),
        [NormalizedStreamEvent::Done { response }]
            if response.content.as_deref() == Some("I can't help with that")
    ));
}

#[test]
fn failed_incomplete_and_error_events_surface_backend_message() {
    for payload in [
        json!({
            "type": "response.failed",
            "sequence_number": 1,
            "response": {
                "id": "resp_failed",
                "object": "response",
                "created_at": 1,
                "status": "failed",
                "error": {"code": "failed", "message": "failed"},
                "incomplete_details": null,
                "instructions": null,
                "max_output_tokens": null,
                "model": "gpt-5",
                "usage": null,
                "output": [],
                "tools": []
            }
        }),
        json!({
            "type": "response.incomplete",
            "sequence_number": 1,
            "response": {
                "id": "resp_incomplete",
                "object": "response",
                "created_at": 1,
                "status": "incomplete",
                "error": {"code": "incomplete", "message": "incomplete"},
                "incomplete_details": {"reason": "incomplete"},
                "instructions": null,
                "max_output_tokens": null,
                "model": "gpt-5",
                "usage": null,
                "output": [],
                "tools": []
            }
        }),
        json!({"type": "error", "error": {"message": "errored"}}),
    ] {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
        let error = processor.handle_payload(payload).expect_err("error payload should fail");
        assert!(
            error.to_string().contains("failed")
                || error.to_string().contains("incomplete")
                || error.to_string().contains("errored")
        );
    }
}

#[test]
fn documented_non_runtime_events_are_ignored() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
    let events = processor
        .handle_payload(json!({
            "type": "response.file_search_call.searching",
            "query": "needle"
        }))
        .expect("documented status event should be ignored");
    assert!(events.is_empty());
    processor
        .handle_payload(json!({
            "type": "response.code_interpreter_call_code.delta",
            "item_id": "ci_1",
            "output_index": 0,
            "sequence_number": 2,
            "delta": "print(1)"
        }))
        .expect("documented code interpreter value-bearing event should be ignored downstream");

    let finished = processor.finish().expect("finish should succeed");
    assert!(matches!(finished.as_slice(), [NormalizedStreamEvent::Done { .. }]));
}

#[test]
fn completed_compaction_item_is_preserved_without_replaying_added_snapshot() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
    processor
        .handle_payload(json!({
            "type": "response.output_item.added",
            "item": {"type": "compaction", "id": "cmp_1", "encrypted_content": "partial_state"}
        }))
        .expect("added compaction item should parse");
    let completed_item = json!({
        "type": "compaction",
        "id": "cmp_1",
        "encrypted_content": "opaque_state"
    });
    processor
        .handle_payload(json!({
            "type": "response.output_item.done",
            "item": completed_item.clone()
        }))
        .expect("completed compaction item should parse");

    let finished = processor.finish().expect("stream should finish");
    let NormalizedStreamEvent::Done { response } = &finished[0] else {
        panic!("expected normalized stream completion");
    };
    let details = response
        .reasoning_details
        .as_ref()
        .expect("compaction detail should be retained");
    assert_eq!(details.len(), 1);
    assert_eq!(serde_json::from_str::<Value>(&details[0]).unwrap(), completed_item);
}

#[test]
fn stepfun_reasoning_part_boundaries_are_tolerated() {
    // StepFun emits `response.reasoning_part.added`/`.done` (not the
    // `reasoning_summary_part` variants). They are status markers and must
    // not abort the normalized stream.
    let mut options = options();
    options.provider_name = "StepFun";
    let mut processor = ResponsesNormalizedStreamProcessor::new(options, parse_response);

    for payload in [
        json!({
            "type": "response.reasoning_part.added",
            "sequence_number": 3,
            "item_id": "rs_1",
            "output_index": 0,
            "content_index": 0,
            "part": {"type": "reasoning_text", "text": ""}
        }),
        json!({
            "type": "response.reasoning_text.delta",
            "sequence_number": 4,
            "item_id": "rs_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "User asked for a greeting."
        }),
        json!({
            "type": "response.reasoning_part.done",
            "sequence_number": 5,
            "item_id": "rs_1",
            "output_index": 0,
            "content_index": 0,
            "part": {"type": "reasoning_text", "text": "User asked for a greeting."}
        }),
        text_delta_fixture("Hello"),
    ] {
        processor
            .handle_payload(payload)
            .expect("StepFun boundary events should be tolerated");
    }

    processor
        .handle_payload(json!({
            "type": "response.completed",
            "sequence_number": 6,
            "response": completed_response_fixture(json!([{
                "type": "message",
                "id": "msg_1",
                "status": "completed",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello"}]
            }]))
        }))
        .expect("completed event should parse");

    let finished = processor.finish().expect("finish should succeed");
    assert!(matches!(
        finished.as_slice(),
        [NormalizedStreamEvent::Done { response }] if response.content.as_deref() == Some("Hello")
    ));
}

#[test]
fn missing_delta_reports_provider_error() {
    let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
    let error = processor
        .handle_payload(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 1
        }))
        .expect_err("missing delta should fail");
    assert!(error.to_string().contains("TestProvider"));
    assert!(error.to_string().contains("invalid stream payload"));
}
