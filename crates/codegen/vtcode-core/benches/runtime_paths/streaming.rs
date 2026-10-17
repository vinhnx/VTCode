use std::{
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{Router, body::Body, http::Response, routing::post};
use criterion::{BenchmarkId, Criterion, Throughput};
use futures::StreamExt;
use tokio::runtime::Runtime;
use vtcode_config::{CustomProviderApiFormat, CustomProviderConfig};
use vtcode_llm::{
    provider::{LLMProvider, LLMRequest, Message, NormalizedStreamEvent},
    providers::{OpenAIProvider, custom_provider::CustomProviderBackendRouter},
};

use super::LoopbackPeer;

// Compile the exact private production helpers for attribution. These are
// helper microbenchmarks, kept separate from the provider entrypoint below;
// no production visibility is widened and no algorithm is copied here.
#[path = "../../../vtcode-llm/src/providers/shared/sse.rs"]
#[allow(
    dead_code,
    unused_imports,
    unused_results,
    clippy::indexing_slicing,
    reason = "Only the byte pump is measured here; other private entrypoints have unit tests."
)]
mod sse;
#[path = "../../../vtcode-llm/src/providers/shared/utf8.rs"]
#[allow(
    dead_code,
    unused_imports,
    unused_results,
    reason = "Only direct byte decoding is measured in this helper benchmark."
)]
mod utf8;

fn recorded_stream(event_count: usize, crlf: bool) -> Vec<u8> {
    let ending = if crlf { "\r\n\r\n" } else { "\n\n" };
    let mut bytes = Vec::new();
    for index in 0..event_count {
        let payload = serde_json::json!({
            "type": "response.output_text.delta",
            "delta": format!("word-{index} café "),
            "item_id": "msg_1", "output_index": 0, "content_index": 0,
            "sequence_number": index,
        });
        bytes.extend_from_slice(format!("data: {payload}{ending}").as_bytes());
    }
    bytes.extend_from_slice(
        format!("data: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp_fixture\",\"output\":[],\"usage\":{{\"input_tokens\":3,\"output_tokens\":7,\"total_tokens\":10}}}}}}{ending}")
            .as_bytes(),
    );
    bytes
}

fn pump(bytes: &[u8], chunk_size: usize) -> usize {
    let mut decoder = utf8::Utf8StreamDecoder::default();
    let mut buffer = Vec::new();
    let mut offset = 0;
    let mut events = 0;
    for chunk in bytes.chunks(chunk_size) {
        decoder.push_bytes(chunk, &mut buffer);
        while let Some(event) = sse::next_sse_event(&buffer, &mut offset).expect("fixture UTF-8") {
            let _ = black_box(sse::extract_data_payload(event));
            events += 1;
        }
        sse::drain_consumed_sse(&mut buffer, &mut offset);
    }
    assert!(buffer.is_empty());
    events
}

pub(super) fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_framing_helper");
    for event_count in [8, 256, 4_096] {
        for (name, crlf, chunk_size) in [
            ("lf_burst", false, usize::MAX),
            ("crlf_burst", true, usize::MAX),
            ("fragmented", false, 17),
        ] {
            let bytes = recorded_stream(event_count, crlf);
            assert_eq!(pump(&bytes, chunk_size), event_count + 1);
            let _ = group.throughput(Throughput::Bytes(bytes.len() as u64));
            let _ = group.bench_function(BenchmarkId::new(name, event_count), |b| {
                b.iter(|| black_box(pump(black_box(&bytes), chunk_size)))
            });
        }
    }
    group.finish();

    let runtime = Runtime::new().expect("benchmark runtime");
    let mut group = c.benchmark_group("responses_provider_loopback");
    for event_count in [8, 256, 4_096] {
        let bytes: Arc<[u8]> = recorded_stream(event_count, false).into();
        let fixture = Arc::clone(&bytes);
        let peer = LoopbackPeer::start(
            &runtime,
            Router::new().route(
                "/v1/responses",
                post(move || {
                    let fixture = Arc::clone(&fixture);
                    async move {
                        Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::from(fixture.to_vec()))
                            .expect("fixture response")
                    }
                }),
            ),
        );
        let provider = OpenAIProvider::from_config(
            Some("fixture-key".to_string()),
            None,
            Some("gpt-5".to_string()),
            Some(format!("{}/v1", peer.url)),
            None,
            None,
            None,
            None,
            None,
        );
        let request = LLMRequest {
            messages: Arc::new(vec![Message::user("offline fixture".to_string())]),
            model: "gpt-5".to_string(),
            stream: true,
            ..Default::default()
        };
        let expected = (0..event_count).map(|index| format!("word-{index} café ")).collect::<String>();
        let consume = || {
            let future = async {
                let mut stream = provider.stream_normalized(request.clone()).await.expect("start fixture stream");
                let mut text = String::new();
                let mut completed = false;
                while let Some(event) = stream.next().await {
                    match event.expect("fixture stream event") {
                        NormalizedStreamEvent::TextDelta { delta } => text.push_str(&delta),
                        NormalizedStreamEvent::Done { response } => {
                            assert_eq!(response.content.as_deref(), Some(expected.as_str()));
                            completed = true;
                        }
                        _ => {}
                    }
                }
                assert!(completed);
                assert_eq!(text, expected);
                black_box(text)
            };
            #[cfg(feature = "profiling")]
            let future = hotpath::future!(future, label = "responses_fixture_consume");
            runtime.block_on(future)
        };
        drop(consume());
        let _ = group.throughput(Throughput::Bytes(bytes.len() as u64));
        let _ = group.bench_function(BenchmarkId::from_parameter(event_count), |b| b.iter(&consume));
    }
    group.finish();

    let mut group = c.benchmark_group("responses_boundaries");
    for mode in [
        "reasoning_tools_fragmented",
        "gpt_reasoning_suppressed",
        "malformed_then_recover",
        "incomplete_then_recover",
    ] {
        let requires_recovery = matches!(mode, "malformed_then_recover" | "incomplete_then_recover");
        // GPT routes deliberately suppress summary deltas; use the existing
        // o-series emission policy for the public-summary workload.
        let model = if mode == "gpt_reasoning_suppressed" {
            "gpt-5"
        } else {
            "o3"
        };
        let events = [
            serde_json::json!({"type":"response.reasoning_summary_text.delta","delta":"summary café","item_id":"rs_1","output_index":0,"summary_index":0,"sequence_number":1}),
            serde_json::json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":"","status":"in_progress"},"sequence_number":2}),
            serde_json::json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":1,"delta":"{\"city\":","sequence_number":3}),
            serde_json::json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":1,"delta":"\"café\"}","sequence_number":4}),
            serde_json::json!({"type":"response.completed","response":{"id":"resp_edges","output":[{"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":"{\"city\":\"café\"}","status":"completed"}],"usage":{"input_tokens":3,"output_tokens":7,"total_tokens":10}}}),
        ];
        let bytes: Arc<[u8]> = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>()
            .into_bytes()
            .into();
        let calls = Arc::new(AtomicUsize::new(0));
        let peer = LoopbackPeer::start(&runtime, Router::new().route("/v1/responses", post(move || {
            let bytes = Arc::clone(&bytes);
            let call = calls.fetch_add(1, Ordering::Relaxed);
            async move {
                let payload = if requires_recovery && call.is_multiple_of(2) {
                    if mode == "malformed_then_recover" { b"data: not-json\n\n".to_vec() }
                    else { b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":1}\n\n".to_vec() }
                } else { bytes.to_vec() };
                let chunks = payload.chunks(7).map(|chunk| Ok::<_, std::convert::Infallible>(axum::body::Bytes::copy_from_slice(chunk))).collect::<Vec<_>>();
                Response::builder().header("content-type", "text/event-stream")
                    .body(Body::from_stream(futures::stream::iter(chunks))).expect("fragmented fixture response")
            }
        })));
        let base_url = format!("{}/v1", peer.url);
        let provider = CustomProviderBackendRouter::from_config(
            CustomProviderConfig {
                name: "fixture".into(),
                display_name: "Offline Responses fixture".into(),
                api_format: CustomProviderApiFormat::OpenAIResponses,
                model: model.into(),
                models: vec![model.into()],
                base_url: base_url.clone(),
                ..Default::default()
            },
            Some("fixture-key".into()),
            Some(model.into()),
            base_url,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let request = LLMRequest {
            messages: Arc::new(vec![Message::user("offline boundaries".into())]),
            model: model.into(),
            stream: true,
            ..Default::default()
        };
        let consume = || {
            runtime.block_on(async {
                let attempts = if requires_recovery { 2 } else { 1 };
                for attempt in 0..attempts {
                    let mut stream = provider
                        .stream_normalized(request.clone())
                        .await
                        .expect("boundary stream startup");
                    let mut reasoning = String::new();
                    let mut arguments = String::new();
                    let mut done = false;
                    let mut failed = false;
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(NormalizedStreamEvent::ReasoningDelta { delta, source }) => {
                                assert!(source.is_public_summary());
                                reasoning.push_str(&delta);
                            }
                            Ok(NormalizedStreamEvent::ToolCallDelta { call_id, delta }) => {
                                assert_eq!(call_id, "call_1");
                                arguments.push_str(&delta);
                            }
                            Ok(NormalizedStreamEvent::Done { response }) => {
                                // HTTP compatibility preserves usable partial output at EOF.
                                if mode == "incomplete_then_recover" && attempt == 0 {
                                    assert_eq!(response.content.as_deref(), Some("partial"));
                                    assert!(response.usage.is_none());
                                    assert!(response.tool_calls.is_none());
                                    done = true;
                                    continue;
                                }
                                let tool = response
                                    .tool_calls
                                    .as_ref()
                                    .and_then(|tools| tools.first())
                                    .expect("completed tool call");
                                assert_eq!(tool.id, "call_1");
                                assert_eq!(
                                    tool.function.as_ref().expect("function call").arguments,
                                    "{\"city\":\"café\"}"
                                );
                                assert_eq!(response.usage.expect("completion usage").total_tokens, 10);
                                done = true;
                            }
                            Err(_) => failed = true,
                            _ => {}
                        }
                    }
                    if mode == "malformed_then_recover" && attempt == 0 {
                        assert!(failed);
                        assert!(!done);
                    } else if mode == "incomplete_then_recover" && attempt == 0 {
                        assert!(!failed);
                        assert!(done);
                    } else {
                        assert!(!failed);
                        assert!(done);
                        assert_eq!(
                            reasoning,
                            if mode == "gpt_reasoning_suppressed" {
                                ""
                            } else {
                                "summary café"
                            }
                        );
                        assert_eq!(arguments, "{\"city\":\"café\"}");
                    }
                }
            })
        };
        consume();
        let _ = group.bench_function(mode, |b| b.iter(&consume));
    }
    group.finish();
}
