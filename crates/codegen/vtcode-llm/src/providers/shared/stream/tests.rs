use super::*;
use serde_json::json;

#[test]
fn finalize_tool_calls_drops_empty_builders() {
    let builders = vec![ToolCallBuilder::default()];
    assert!(finalize_tool_calls(builders).is_none());
}

#[test]
fn finalize_fabricates_unique_ids_across_batches() {
    let idless_builder = || {
        let mut builder = ToolCallBuilder::default();
        builder.apply_delta(&json!({"function": {"name": "foo", "arguments": "{}"}}));
        builder
    };

    let first = finalize_tool_calls(vec![idless_builder(), idless_builder()]).expect("calls expected");
    let second = finalize_tool_calls(vec![idless_builder(), idless_builder()]).expect("calls expected");

    let ids: Vec<&str> = first.iter().chain(second.iter()).map(|call| call.id.as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "fabricated ids must be unique across responses");

    for id in ids {
        let hex = id.strip_prefix("call_").expect("fabricated id prefix");
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}

#[test]
fn finalize_preserves_provider_supplied_id() {
    let mut builder = ToolCallBuilder::default();
    builder.apply_delta(&json!({"id": "provider-id-1", "function": {"name": "foo"}}));
    let call = builder.finalize().expect("call expected");
    assert_eq!(call.id, "provider-id-1");
}

#[test]
fn append_text_with_reasoning_tracks_segments() {
    let telemetry = NoopStreamTelemetry;
    let mut aggregated = String::new();
    let mut reasoning = ReasoningBuffer::default();
    let mut delta = StreamDelta::default();
    append_text_with_reasoning("Hello", &mut aggregated, &mut reasoning, &mut delta, &telemetry);
    assert_eq!(aggregated, "Hello");
    assert_eq!(delta.into_fragments(), vec![StreamFragment::Content("Hello".into())]);
}

#[test]
fn apply_tool_call_delta_updates_builder() {
    let telemetry = NoopStreamTelemetry;
    let mut builders = Vec::new();
    let container = json!({
        "index": 0,
        "function": {"name": "foo", "arguments": "{}"}
    })
    .as_object()
    .cloned()
    .unwrap();
    apply_tool_call_delta_from_content(&mut builders, &container, &telemetry);
    let calls = finalize_tool_calls(builders).expect("call expected");
    let func = calls[0].function.as_ref().expect("function call should be present");
    assert_eq!(func.name, "foo");
}

#[test]
fn apply_tool_call_delta_uses_outer_index_for_nested_delta() {
    let telemetry = NoopStreamTelemetry;
    let mut builders = Vec::new();
    let container = json!({
        "delta": {
            "tool_call": {
                "function": {
                    "name": "foo",
                    "arguments": "{\"value\":1}"
                }
            }
        },
        "index": 1,
        "id": "call-1"
    })
    .as_object()
    .cloned()
    .unwrap();

    apply_tool_call_delta_from_content(&mut builders, &container, &telemetry);

    let calls = finalize_tool_calls(builders).expect("call expected");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call-1");
    let func = calls[0].function.as_ref().expect("function call should be present");
    assert_eq!(func.arguments, "{\"value\":1}");
}

#[test]
fn update_tool_calls_respects_explicit_index() {
    let mut builders = Vec::new();
    let deltas = vec![json!({
        "index": 2,
        "id": "call_3",
        "function": {
            "name": "get_weather",
            "arguments": "{\"city\":\"Beijing\"}"
        }
    })];

    update_tool_calls(&mut builders, &deltas);

    let calls = finalize_tool_calls(builders).expect("call expected");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_3");
    let function = calls[0].function.as_ref().expect("function expected");
    assert_eq!(function.name, "get_weather");
    assert_eq!(function.arguments, "{\"city\":\"Beijing\"}");
}

#[test]
fn stream_aggregator_derives_reasoning_from_details_when_missing() {
    let mut aggregator = StreamAggregator::new("test-model".to_string());
    aggregator.set_reasoning_details(&[json!({
        "type": "reasoning.text",
        "text": "step one"
    })]);

    let response = aggregator.finalize();
    assert_eq!(response.reasoning.as_deref(), Some("step one"));
    assert!(response.reasoning_details.is_some());
}

#[test]
fn handle_chunk_extracts_content_delta() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut aggregator = StreamAggregator::new("test-model".to_string());
    let chunk = json!({
        "choices": [{"delta": {"content": "hello"}}]
    });

    handle_openai_compatible_chunk(&chunk, &mut aggregator, &tx, &[], OpenAiDeltaOrder::ContentFirst, false);

    let event = rx.try_recv().expect("event expected");
    match event.unwrap() {
        LLMStreamEvent::Token { delta } => {
            assert_eq!(delta, "hello");
        }
        other => panic!("expected Token event, got {other:?}"),
    }
}

#[test]
fn handle_chunk_extracts_reasoning_delta() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut aggregator = StreamAggregator::new("test-model".to_string());
    let chunk = json!({
        "choices": [{"delta": {"reasoning_content": "thinking..."}}]
    });

    handle_openai_compatible_chunk(
        &chunk,
        &mut aggregator,
        &tx,
        &["reasoning_content"],
        OpenAiDeltaOrder::ReasoningFirst,
        false,
    );

    let event = rx.try_recv().expect("event expected");
    match event.unwrap() {
        LLMStreamEvent::Reasoning { delta } => {
            assert_eq!(delta, "thinking...");
        }
        other => panic!("expected Reasoning event, got {other:?}"),
    }
}

#[test]
fn handle_chunk_aggregates_tool_calls() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut aggregator = StreamAggregator::new("test-model".to_string());
    let chunk = json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_1",
                    "function": {"name": "search", "arguments": "{\"q\":\"test\"}"}
                }]
            }
        }]
    });

    handle_openai_compatible_chunk(&chunk, &mut aggregator, &tx, &[], OpenAiDeltaOrder::ContentFirst, false);

    let response = aggregator.finalize();
    let calls = response.tool_calls.expect("tool calls expected");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_1");
    let func = calls[0].function.as_ref().expect("function expected");
    assert_eq!(func.name, "search");
    assert_eq!(func.arguments, "{\"q\":\"test\"}");
}

#[test]
fn handle_chunk_skips_empty_reasoning_and_falls_back() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut aggregator = StreamAggregator::new("test-model".to_string());
    // Evolink-style: reasoning is empty string, reasoning_content has actual content
    let chunk = json!({
        "choices": [{"delta": {"reasoning": "", "reasoning_content": "actual reasoning"}}]
    });

    handle_openai_compatible_chunk(
        &chunk,
        &mut aggregator,
        &tx,
        &["reasoning", "reasoning_content"],
        OpenAiDeltaOrder::ReasoningFirst,
        false,
    );

    let event = rx.try_recv().expect("event expected");
    match event.unwrap() {
        LLMStreamEvent::Reasoning { delta } => {
            assert_eq!(delta, "actual reasoning");
        }
        other => panic!("expected Reasoning event, got {other:?}"),
    }
}

#[test]
fn handle_chunk_passes_include_cache_metrics_to_usage() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut aggregator = StreamAggregator::new("test-model".to_string());
    let chunk = json!({
        "choices": [{"delta": {}}],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150,
            "prompt_cache_hit_tokens": 30,
            "prompt_cache_miss_tokens": 70
        }
    });

    // With include_cache_metrics = false (Evolink/Moonshot/StepFun/ZAI behavior)
    handle_openai_compatible_chunk(&chunk, &mut aggregator, &tx, &[], OpenAiDeltaOrder::ContentFirst, false);

    let response = aggregator.finalize();
    let usage = response.usage.expect("usage expected");
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.cached_prompt_tokens, None);
    assert_eq!(usage.cache_creation_tokens, None);

    // With include_cache_metrics = true (DeepSeek behavior)
    let mut aggregator2 = StreamAggregator::new("test-model".to_string());
    handle_openai_compatible_chunk(&chunk, &mut aggregator2, &tx, &[], OpenAiDeltaOrder::ContentFirst, true);

    let response2 = aggregator2.finalize();
    let usage2 = response2.usage.expect("usage expected");
    assert_eq!(usage2.prompt_tokens, 100);
    assert_eq!(usage2.cached_prompt_tokens, Some(30));
    assert_eq!(usage2.cache_creation_tokens, Some(70));
}

#[tokio::test]
async fn process_openai_stream_fails_closed_on_boundaryless_tail_over_cap() {
    use crate::providers::shared::MAX_SSE_TAIL_BYTES;
    use futures::stream;

    // The real growth shape: small boundary-less chunks accumulating past
    // the cap across loop iterations, tripping on the third chunk.
    let byte_stream = stream::iter(vec![
        Ok::<bytes::Bytes, !>(bytes::Bytes::from("x".repeat(MAX_SSE_TAIL_BYTES / 2))),
        Ok::<bytes::Bytes, !>(bytes::Bytes::from("y".repeat(MAX_SSE_TAIL_BYTES / 2))),
        Ok::<bytes::Bytes, !>(bytes::Bytes::from_static(b"zz")),
    ]);
    let result = process_openai_stream(byte_stream, "test-provider", "test-model".to_string(), |_| Ok(())).await;
    let err = result.expect_err("boundary-less tail over the cap must fail closed");
    match err {
        LLMError::Provider { message, .. } => {
            assert!(message.contains("test-provider"), "{message}");
            assert!(message.contains("without an event boundary"), "{message}");
        }
        other => panic!("expected Provider error, got {other:?}"),
    }
}

#[tokio::test]
async fn process_openai_stream_accepts_boundaryless_tail_under_cap() {
    use crate::providers::shared::MAX_SSE_TAIL_BYTES;
    use futures::stream;

    let tail = "x".repeat(MAX_SSE_TAIL_BYTES - 1);
    let byte_stream = stream::iter(vec![Ok::<bytes::Bytes, !>(bytes::Bytes::from(tail))]);
    let mut calls = 0;
    let result = process_openai_stream(byte_stream, "test-provider", "test-model".to_string(), |_| {
        calls += 1;
        Ok(())
    })
    .await;
    assert!(result.is_ok(), "under-cap tail at EOF must not error: {result:?}");
    assert_eq!(calls, 0, "boundary-less tail carries no complete event");
}

#[tokio::test]
async fn process_openai_stream_parses_single_event_over_the_legacy_cap() {
    use futures::stream;

    // Regression: one complete event larger than the historical 1 MiB
    // cap must parse once its boundary arrives, not abort the stream.
    // The bound guards unbounded retention, not event size.
    let legacy_cap = 1_048_576;
    let padding = "x".repeat(legacy_cap + 1024);
    let payload = format!("{{\"choices\":[{{\"delta\":{{\"content\":\"{padding}\"}}}}]}}");
    let byte_stream = stream::iter(vec![
        Ok::<bytes::Bytes, !>(bytes::Bytes::from(format!("data: {payload}"))),
        Ok::<bytes::Bytes, !>(bytes::Bytes::from_static(b"\n\n")),
    ]);
    let mut seen = Vec::new();
    let result = process_openai_stream(byte_stream, "test-provider", "test-model".to_string(), |value| {
        seen.push(value);
        Ok(())
    })
    .await;
    assert!(result.is_ok(), "a large-but-complete event must not fail closed: {result:?}");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["choices"][0]["delta"]["content"].as_str().unwrap().len(), legacy_cap + 1024);
}

#[tokio::test]
async fn process_openai_stream_measures_the_retained_tail_after_drain() {
    use crate::providers::shared::MAX_SSE_TAIL_BYTES;
    use futures::stream;

    // Pre-drain peak tops the cap, but the completed event drains and the
    // retained tail is empty: the bound guards retention, not one event.
    let first = "x".repeat(MAX_SSE_TAIL_BYTES - 100);
    let second = format!("{}\n\n", "y".repeat(200));
    let byte_stream = stream::iter(vec![
        Ok::<bytes::Bytes, !>(bytes::Bytes::from(first)),
        Ok::<bytes::Bytes, !>(bytes::Bytes::from(second)),
    ]);
    let result = process_openai_stream(byte_stream, "test-provider", "test-model".to_string(), |_| Ok(())).await;
    assert!(result.is_ok(), "drained tail must not trip the bound: {result:?}");
}

#[tokio::test]
async fn process_openai_stream_still_delivers_small_events() {
    use futures::stream;

    let byte_stream = stream::iter(vec![Ok::<bytes::Bytes, !>(bytes::Bytes::from_static(
        b"data: {\"choices\": [{\"delta\": {\"content\": \"hi\"}}]}\n\n",
    ))]);
    let mut seen = Vec::new();
    let result = process_openai_stream(byte_stream, "test-provider", "test-model".to_string(), |value| {
        seen.push(value);
        Ok(())
    })
    .await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["choices"][0]["delta"]["content"], "hi");
}
