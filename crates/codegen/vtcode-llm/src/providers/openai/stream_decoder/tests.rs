use super::{
    ChatStreamEvent, ChatToolCallState, ResponsesToolCallState, chat_event_to_normalized,
    final_response_output_is_empty, merge_final_response_metadata, streamed_response_is_usable,
};
use crate::provider::{LLMResponse, NormalizedStreamEvent, ReasoningSource, ToolCall};
use crate::providers::shared::StreamAggregator;
use serde_json::json;

#[test]
fn responses_final_metadata_parses_cached_prompt_tokens_when_enabled() {
    let mut response = LLMResponse::default();
    merge_final_response_metadata(
        &mut response,
        &json!({
            "id": "resp_stream",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 5,
                "total_tokens": 17,
                "input_tokens_details": {
                    "cached_tokens": 9
                }
            }
        }),
        true,
    );

    assert_eq!(response.request_id.as_deref(), Some("resp_stream"));
    let usage = response.usage.expect("usage should be populated");
    assert_eq!(usage.prompt_tokens, 12);
    assert_eq!(usage.completion_tokens, 5);
    assert_eq!(usage.total_tokens, 17);
    assert_eq!(usage.cached_prompt_tokens, Some(9));
}

#[test]
fn empty_final_response_can_use_streamed_tool_call_delta() {
    let response = LLMResponse {
        tool_calls: Some(vec![ToolCall::function(
            "call_1".to_string(),
            "search_workspace".to_string(),
            "{\"query\":\"vtcode\"}".to_string(),
        )]),
        ..Default::default()
    };

    assert!(final_response_output_is_empty(&json!({"output": []})));
    assert!(streamed_response_is_usable(&response));
}

#[test]
fn chat_normalization_preserves_reasoning_source() {
    let raw_events = chat_event_to_normalized(ChatStreamEvent::ReasoningDelta {
        delta: "tagged trace".to_string(),
        source: ReasoningSource::Unknown,
    });
    assert!(matches!(
        raw_events.as_slice(),
        [NormalizedStreamEvent::ReasoningDelta { delta, source }]
            if delta == "tagged trace" && *source == ReasoningSource::Unknown
    ));

    let summary_events = chat_event_to_normalized(ChatStreamEvent::ReasoningDelta {
        delta: "provider summary".to_string(),
        source: ReasoningSource::ProviderSummary,
    });
    assert!(matches!(
        summary_events.as_slice(),
        [NormalizedStreamEvent::ReasoningDelta { delta, source }]
            if delta == "provider summary" && *source == ReasoningSource::ProviderSummary
    ));
}

#[test]
fn chat_normalization_preserves_completed_usage_before_done() {
    let events = chat_event_to_normalized(ChatStreamEvent::Completed {
        response: Box::new(LLMResponse {
            usage: Some(crate::provider::Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            }),
            ..Default::default()
        }),
    });

    assert!(matches!(
        events.as_slice(),
        [NormalizedStreamEvent::Usage { .. }, NormalizedStreamEvent::Done { .. }]
    ));
}

#[test]
fn chat_tool_deltas_emit_structured_events_and_reuse_the_final_id() {
    let mut aggregator = StreamAggregator::new("gpt-5".to_string());
    let mut tool_call_state = ChatToolCallState::default();

    let first = tool_call_state.handle_deltas(
        &mut aggregator,
        &[json!({
            "index": 0,
            "id": "call_1",
            "function": {
                "name": "search_workspace",
                "arguments": "{\"query\":\"vt"
            }
        })],
    );
    assert!(matches!(
        first.as_slice(),
        [
            ChatStreamEvent::ToolCallStart { call_id, name },
            ChatStreamEvent::ToolCallDelta { call_id: delta_call_id, delta }
        ] if call_id == "call_1"
            && delta_call_id == "call_1"
            && name.as_deref() == Some("search_workspace")
            && delta == "{\"query\":\"vt"
    ));

    let second = tool_call_state.handle_deltas(
        &mut aggregator,
        &[json!({
            "index": 0,
            "function": {
                "arguments": "code\"}"
            }
        })],
    );
    assert!(matches!(
        second.as_slice(),
        [ChatStreamEvent::ToolCallDelta { call_id, delta }]
            if call_id == "call_1" && delta == "code\"}"
    ));

    let calls = aggregator.finalize().tool_calls.expect("tool call expected");
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].tool_name(), Some("search_workspace"));
    assert_eq!(calls[0].raw_input(), Some("{\"query\":\"vtcode\"}"));
}

#[test]
fn responses_tool_call_state_uses_provider_call_id_when_item_id_differs() {
    let mut aggregator = StreamAggregator::new("gpt-5".to_string());
    let mut tool_call_state = ResponsesToolCallState::default();

    tool_call_state
        .capture_metadata(
            &mut aggregator,
            &json!({
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "search_workspace"
            }),
            Some(0),
        )
        .unwrap();
    tool_call_state
        .handle_arguments_delta(
            &mut aggregator,
            &json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "delta": "{\"query\":\"vtcode\"}"
            }),
        )
        .expect("tool delta should parse");

    let response = aggregator.finalize();
    assert_eq!(
        response.tool_calls.as_ref(),
        Some(&vec![ToolCall::function(
            "call_1".to_string(),
            "search_workspace".to_string(),
            "{\"query\":\"vtcode\"}".to_string(),
        )])
    );
}

#[test]
fn responses_tool_call_metadata_rejects_exhausted_index_before_aggregation() {
    let mut aggregator = StreamAggregator::new("gpt-5".to_string());
    let mut state = ResponsesToolCallState::default();
    let error = state
        .capture_metadata(
            &mut aggregator,
            &json!({"type":"function_call", "call_id":"too-large", "name":"search_workspace"}),
            Some(usize::MAX),
        )
        .unwrap_err();
    assert!(error.to_string().contains("tool call index space exhausted"));
    assert!(aggregator.finalize().tool_calls.is_none());
}

#[test]
fn responses_tool_call_state_reuses_fabricated_id_across_deltas() {
    let mut aggregator = StreamAggregator::new("gpt-5".to_string());
    let mut tool_call_state = ResponsesToolCallState::default();

    let delta = |fragment: &str| {
        json!({
            "type": "response.function_call_arguments.delta",
            "output_index": 0,
            "delta": fragment
        })
    };
    tool_call_state
        .handle_arguments_delta(&mut aggregator, &delta("{\"query\":"))
        .expect("tool delta should parse");
    tool_call_state
        .handle_arguments_delta(&mut aggregator, &delta("\"vtcode\"}"))
        .expect("tool delta should parse");

    // Supply the name the way metadata would, without an id, so finalize
    // keeps the builder created by the id-less deltas.
    aggregator.handle_tool_calls(&[json!({
        "index": 0,
        "function": {"name": "search_workspace"}
    })]);

    let calls = aggregator.finalize().tool_calls.expect("tool call expected");
    assert_eq!(calls.len(), 1, "both deltas must land on one builder");
    assert!(calls[0].id.starts_with("call_"));
    assert_ne!(calls[0].id, "call_0");
    let function = calls[0].function.as_ref().expect("function expected");
    assert_eq!(function.arguments, "{\"query\":\"vtcode\"}");
}

#[test]
fn responses_tool_call_state_fabricates_distinct_ids_per_decoder() {
    let mut ids = Vec::new();
    for _ in 0..2 {
        let mut aggregator = StreamAggregator::new("gpt-5".to_string());
        let mut tool_call_state = ResponsesToolCallState::default();
        tool_call_state
            .handle_arguments_delta(
                &mut aggregator,
                &json!({
                    "type": "response.function_call_arguments.delta",
                    "output_index": 0,
                    "delta": "{}"
                }),
            )
            .expect("tool delta should parse");
        aggregator.handle_tool_calls(&[json!({
            "index": 0,
            "function": {"name": "search_workspace"}
        })]);
        let calls = aggregator.finalize().tool_calls.expect("tool call expected");
        ids.push(calls[0].id.clone());
    }
    assert_ne!(ids[0], ids[1], "fabricated ids must differ across responses");
}
