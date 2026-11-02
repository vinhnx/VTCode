use super::*;

#[tokio::test]
async fn websocket_stream_deltas_precede_completion_and_legacy_preserves_empty_output() {
    use crate::provider::{LLMStreamEvent, NormalizedStreamEvent};
    for legacy in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Paused {
                events: vec![text_delta("before")],
                gate: Arc::clone(&gate),
                completion: completion("final", json!([])),
            },
        ]])
        .await
        else {
            panic!("local server unavailable");
        };
        let provider = websocket_test_provider(base);
        let response = if legacy {
            let mut stream = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                provider.stream_request(websocket_test_request()),
            )
            .await
            .expect("startup before completion")
            .expect("stream");
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), LLMStreamEvent::Token { delta } if delta == "before")
            );
            gate.notify_one();
            match stream.next().await.unwrap().unwrap() {
                LLMStreamEvent::Completed { response } => *response,
                event => panic!("unexpected event: {event:?}"),
            }
        } else {
            let mut stream = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                provider.stream_normalized_request(websocket_test_request()),
            )
            .await
            .expect("startup before completion")
            .expect("stream");
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::TextDelta { delta } if delta == "before")
            );
            gate.notify_one();
            final_normalized(stream).await
        };
        assert_eq!(response.content.as_deref(), Some("before"));
        assert_eq!(response.request_id.as_deref(), Some("final"));
        assert_eq!(response.usage.unwrap().total_tokens, 26);
        assert_eq!(recorded.lock().unwrap().len(), 2);
        assert!(provider.websocket_session.lock().await.is_some());
        assert!(
            provider.websocket_continuation_snapshot().is_none(),
            "empty authoritative output cannot safely match replay"
        );
        handle.await.unwrap();
    }
}
#[tokio::test]
async fn websocket_stream_reasoning_refusal_and_tools_preserve_ids_and_usage() {
    use crate::provider::{NormalizedStreamEvent, ReasoningSource};
    let tool = json!({"type":"function_call","id":"fc_wire","call_id":"call_real","name":"lookup","arguments":"{\"city\":\"Hue\"}","status":"completed"});
    let events = vec![
        json!({"type":"response.reasoning_summary_text.delta","delta":"summary","item_id":"rs_1","output_index":0,"summary_index":0,"sequence_number":1}),
        json!({"type":"response.reasoning_text.delta","delta":"private","item_id":"rs_1","output_index":0,"content_index":0,"sequence_number":2}),
        json!({"type":"response.refusal.delta","delta":"refused","item_id":"msg_1","output_index":1,"content_index":0,"sequence_number":3}),
        json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc_wire","call_id":"call_real","name":"lookup","arguments":"","status":"in_progress"},"sequence_number":4}),
        json!({"type":"response.function_call_arguments.delta","item_id":"fc_wire","output_index":2,"delta":"{\"city\":","sequence_number":5}),
        json!({"type":"response.function_call_arguments.delta","item_id":"fc_wire","output_index":2,"delta":"\"Hue\"}","sequence_number":6}),
        completion("tools", json!([tool])),
    ];
    let Some((base, _, handle)) = spawn_scripted_websocket_server(vec![vec![
        ScriptedReply::Events(vec![completion("warm", json!([]))]),
        ScriptedReply::Events(events.clone()),
    ]])
    .await
    else {
        panic!("server unavailable");
    };
    let provider = websocket_test_provider(base);
    let mut request = websocket_test_request();
    // Exercise the existing reasoning-emission policy with an o-series
    // fixture; GPT families deliberately suppress public summaries.
    request.model = "o3".to_string();
    provider.set_responses_api_state("o3", super::super::super::super::types::ResponsesApiState::Allowed);
    let mut stream = provider.stream_normalized_request(request).await.unwrap();
    let mut seen = Vec::new();
    while let Some(event) = stream.next().await {
        seen.push(event.unwrap());
    }
    assert!(seen.iter().any(|event| matches!(event, NormalizedStreamEvent::ReasoningDelta { delta, source:ReasoningSource::ProviderSummary } if delta == "summary")));
    assert!(seen.iter().any(|event| matches!(event, NormalizedStreamEvent::ReasoningDelta { delta, source:ReasoningSource::Continuation } if delta == "private")));
    assert!(
        seen.iter()
            .any(|event| matches!(event, NormalizedStreamEvent::TextDelta { delta } if delta == "refused"))
    );
    assert!(
        seen.iter().any(
            |event| matches!(event, NormalizedStreamEvent::ToolCallStart { call_id, .. } if call_id == "call_real")
        )
    );
    assert!(seen.iter().any(|event| matches!(event, NormalizedStreamEvent::ToolCallDelta { call_id, delta } if call_id == "call_real" && delta == "\"Hue\"}")));
    let NormalizedStreamEvent::Done { response } = seen.last().unwrap() else {
        panic!("done");
    };
    assert_eq!(response.content.as_deref(), Some("refused"));
    let calls = response.tool_calls.as_ref().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_real");
    assert_eq!(calls[0].function.as_ref().unwrap().arguments, "{\"city\":\"Hue\"}");
    assert_eq!(response.usage.as_ref().unwrap().prompt_tokens, 17);
    handle.await.unwrap();

    let Some((base, _, handle)) = spawn_scripted_websocket_server(vec![vec![
        ScriptedReply::Events(vec![completion("warm", json!([]))]),
        ScriptedReply::Events(events),
    ]])
    .await
    else {
        panic!("server unavailable");
    };
    let provider = websocket_test_provider(base);
    provider.set_responses_api_state("o3", super::super::super::super::types::ResponsesApiState::Allowed);
    let mut request = websocket_test_request();
    request.model = "o3".to_string();
    let mut stream = provider.stream_request(request).await.unwrap();
    let mut reasoning = String::new();
    let mut response = None;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            crate::provider::LLMStreamEvent::Reasoning { delta } => reasoning.push_str(&delta),
            crate::provider::LLMStreamEvent::Completed { response: done } => response = Some(done),
            _ => {}
        }
    }
    assert_eq!(reasoning, "summaryprivate");
    let response = response.unwrap();
    assert_eq!(response.content.as_deref(), Some("refused"));
    assert_eq!(response.tool_calls.as_ref().unwrap()[0].id, "call_real");
    assert_eq!(response.tool_calls.as_ref().unwrap()[0].function.as_ref().unwrap().arguments, "{\"city\":\"Hue\"}");
    assert_eq!(response.usage.as_ref().unwrap().total_tokens, 26);
    handle.await.unwrap();
}
#[tokio::test]
async fn websocket_stream_tool_followup_sends_only_new_input_on_one_connection() {
    for tier in ["flex", "ultrafast"] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![completion("tool_turn", json!([{"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":"{\"key\":1}"}]))]),
            ScriptedReply::Events(vec![completion("reply", text_output("found"))]),
            ScriptedReply::Events(vec![completion("last", text_output("done"))]),
        ]]).await else { panic!("server unavailable"); };
        let provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.model = "gpt-6-astra".to_string();
        request.service_tier = Some(tier.to_string());
        let response = final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap()).await;
        Arc::make_mut(&mut request.messages)
            .push(ProviderMessage::assistant_with_tools("".to_string(), response.tool_calls.unwrap()));
        Arc::make_mut(&mut request.messages)
            .push(ProviderMessage::tool_response("call_1".to_string(), "result".to_string()));
        let response = final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap()).await;
        Arc::make_mut(&mut request.messages).push(ProviderMessage::assistant(response.content.unwrap()));
        Arc::make_mut(&mut request.messages).push(ProviderMessage::user("next".to_string()));
        final_normalized(provider.stream_normalized_request(request).await.unwrap()).await;
        {
            let requests = recorded.lock().unwrap();
            assert_eq!(requests.len(), 4);
            assert_eq!(requests[0]["generate"], false);
            assert_eq!(requests[1]["previous_response_id"], "warm");
            assert_eq!(requests[1]["input"], json!([]));
            assert_eq!(requests[2]["previous_response_id"], "tool_turn");
            assert_eq!(
                requests[2]["input"],
                json!([{"type":"function_call_output", "call_id":"call_1", "output":"result"}])
            );
            assert_eq!(requests[3]["previous_response_id"], "reply");
            assert_eq!(
                requests[3]["input"],
                json!([{"role":"user", "content":[{"type":"input_text", "text":"next"}]}])
            );
            assert!(
                requests
                    .iter()
                    .all(|event| event["service_tier"] == tier && event["store"] == false)
            );
        }

        handle.await.unwrap();
    }
}
#[test]
fn websocket_history_matches_only_cache_annotations_and_starts_fresh_for_changes() {
    let payload = json!({"model":"gpt-5.6","instructions":"stable","tools":[{"name":"a"}],"input":[{"role":"user","content":[{"type":"input_text","text":"first","prompt_cache_breakpoint":true}]}]});
    let prepared = prepare_websocket_event(&payload, None, true).unwrap();
    let cache = OpenAIResponsesWebSocketContinuationCache::from_response(
        &json!({"id":"warm"}),
        &websocket_test_request(),
        &prepared,
        "gpt-5.6",
    )
    .unwrap();
    let mut next = payload.clone();
    next["input"][0]["content"][0]
        .as_object_mut()
        .unwrap()
        .remove("prompt_cache_breakpoint");
    assert!(
        prepare_websocket_event(&next, Some(&cache), false)
            .unwrap()
            .used_previous_response_id
    );
    for (field, value) in [
        ("model", json!("gpt-6-astra")),
        ("instructions", json!("changed")),
        ("tools", json!([])),
        ("input", json!([])),
        ("input", json!([{"type":"compaction","encrypted_content":"opaque"}])),
    ] {
        let mut changed = next.clone();
        changed[field] = value;
        assert!(
            !prepare_websocket_event(&changed, Some(&cache), false)
                .unwrap()
                .used_previous_response_id,
            "{field}"
        );
    }
    next["input"][0]["content"][0]["text"] = json!("edited");
    assert!(
        !prepare_websocket_event(&next, Some(&cache), false)
            .unwrap()
            .used_previous_response_id
    );
}
#[tokio::test]
async fn websocket_stream_early_drop_and_cancellation_invalidate_and_reconnect() {
    for before_output in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![
            vec![
                ScriptedReply::Events(vec![completion("warm_old", json!([]))]),
                ScriptedReply::Paused {
                    events: if before_output {
                        vec![]
                    } else {
                        vec![text_delta("partial")]
                    },
                    gate: Arc::clone(&gate),
                    completion: completion("cancelled", text_output("partial")),
                },
            ],
            vec![
                ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                ScriptedReply::Events(vec![completion("fresh", text_output("fresh"))]),
            ],
        ])
        .await
        else {
            panic!("server unavailable");
        };
        let provider = websocket_test_provider(base);
        if before_output {
            let startup = provider.stream_normalized_request(websocket_test_request());
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), startup)
                    .await
                    .is_err()
            );
        } else {
            let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
            assert!(stream.next().await.unwrap().is_ok());
            drop(stream);
        }
        assert!(provider.websocket_session.lock().await.is_none());
        assert!(provider.websocket_continuation_snapshot().is_none());
        gate.notify_one();
        let response =
            final_normalized(provider.stream_normalized_request(websocket_test_request()).await.unwrap()).await;
        assert_eq!(response.content.as_deref(), Some("fresh"));
        {
            let requests = recorded.lock().unwrap();
            assert_eq!(requests.len(), 4);
            assert!(requests[2].get("previous_response_id").is_none());
            assert_eq!(requests[2]["generate"], false);
            assert_eq!(requests[3]["previous_response_id"], "warm_new");
        }

        handle.await.unwrap();
    }
}
