use super::*;

#[test]
fn harness_streaming_bridge_emits_incremental_agent_and_reasoning_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    // The second output delta must cross MIN_OUTPUT_UPDATE_BYTES (1024) for the
    // throttled incremental `item.updated` event to fire.
    let tail = " world".repeat(200);
    let full_output = format!("hello{tail}");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_123", 1, 1);
    bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("think".to_string()));
    bridge.on_progress(StreamProgressEvent::OutputDelta("hello".to_string()));
    bridge.on_progress(StreamProgressEvent::OutputDelta(tail.clone()));
    bridge.complete_open_items();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let mut saw_assistant_started = false;
    let mut saw_assistant_updated = false;
    let mut saw_assistant_completed = false;
    let mut saw_reasoning_started = false;
    let mut saw_reasoning_completed = false;

    for line in payload.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("json");
        let event = value.get("event").expect("event");
        let event_type = event.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();
        let item_type = event
            .get("item")
            .and_then(|item| item.get("type"))
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();
        let item_text = event
            .get("item")
            .and_then(|item| item.get("text"))
            .and_then(|text| text.as_str())
            .unwrap_or_default();

        if event_type == "item.started" && item_type == "agent_message" {
            saw_assistant_started = item_text == "hello";
        }
        if event_type == "item.updated" && item_type == "agent_message" {
            saw_assistant_updated = item_text == full_output;
        }
        if event_type == "item.completed" && item_type == "agent_message" {
            saw_assistant_completed = item_text == full_output;
        }
        if event_type == "item.started" && item_type == "reasoning" {
            saw_reasoning_started = item_text == "think";
        }
        if event_type == "item.completed" && item_type == "reasoning" {
            let stage = event
                .get("item")
                .and_then(|item| item.get("stage"))
                .and_then(|stage| stage.as_str())
                .unwrap_or_default();
            saw_reasoning_completed = item_text == "think" && stage == "analysis";
        }
    }

    assert!(saw_assistant_started);
    assert!(saw_assistant_updated);
    assert!(saw_assistant_completed);
    assert!(saw_reasoning_started);
    assert!(saw_reasoning_completed);
}
#[test]
fn harness_streaming_bridge_throttles_reasoning_update_events() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_789", 2, 1);
    bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("seed".to_string()));
    for _ in 0..12 {
        bridge.on_progress(StreamProgressEvent::ReasoningDelta("tiny".to_string()));
    }
    bridge.on_progress(StreamProgressEvent::ReasoningStage("diagnosing".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("x".repeat(200)));
    bridge.on_progress(StreamProgressEvent::ReasoningStage("final".to_string()));
    bridge.complete_open_items();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let reasoning_updates = payload
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| {
            value
                .get("event")
                .and_then(|event| event.get("type"))
                .and_then(|kind| kind.as_str())
                == Some("item.updated")
                && value
                    .get("event")
                    .and_then(|event| event.get("item"))
                    .and_then(|item| item.get("type"))
                    .and_then(|kind| kind.as_str())
                    == Some("reasoning")
        })
        .count();

    assert!(reasoning_updates <= 2, "expected throttled reasoning updates, got {reasoning_updates}");
    assert!(reasoning_updates >= 1, "expected at least one meaningful reasoning update, got {reasoning_updates}");
}
#[test]
fn harness_streaming_bridge_abort_closes_open_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_456", 3, 2);
    bridge.on_progress(StreamProgressEvent::OutputDelta("partial".to_string()));
    bridge.abort();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let completed_count = payload
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| {
            value
                .get("event")
                .and_then(|event| event.get("type"))
                .and_then(|kind| kind.as_str())
                == Some("item.completed")
        })
        .count();
    assert_eq!(completed_count, 1, "abort should close active stream item");
}
#[test]
fn harness_streaming_bridge_emits_tool_invocation_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_tool", 4, 1);
    bridge.on_progress(StreamProgressEvent::ToolCallStarted {
        call_id: "call_1".to_string(),
        name: Some("shell".to_string()),
    });
    bridge.on_progress(StreamProgressEvent::ToolCallDelta {
        call_id: "call_1".to_string(),
        delta: "{\"cmd\":\"ec".to_string(),
    });
    bridge.on_progress(StreamProgressEvent::ToolCallDelta {
        call_id: "call_1".to_string(),
        delta: "ho hi\"}".to_string(),
    });
    bridge.complete_open_items();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let mut saw_started = false;
    let mut saw_updated = false;
    let mut saw_completed = false;

    for line in payload.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("json");
        let event = value.get("event").expect("event");
        let event_type = event.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();
        let item = event.get("item").expect("item");
        let item_type = item.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();

        if item_type != "tool_invocation" {
            continue;
        }

        let tool_name = item.get("tool_name").and_then(|name| name.as_str()).unwrap_or_default();
        let tool_call_id = item.get("tool_call_id").and_then(|id| id.as_str()).unwrap_or_default();
        let status = item.get("status").and_then(|status| status.as_str()).unwrap_or_default();
        let arguments = item.get("arguments");

        if event_type == "item.started" {
            saw_started = tool_name == "shell" && tool_call_id == "call_1" && status == "in_progress";
        }
        if event_type == "item.updated" {
            saw_updated = tool_name == "shell"
                && tool_call_id == "call_1"
                && status == "in_progress"
                && arguments.and_then(|value| value.get("cmd")).and_then(|cmd| cmd.as_str()) == Some("echo hi");
        }
        if event_type == "item.completed" {
            saw_completed = tool_name == "shell"
                && tool_call_id == "call_1"
                && status == "completed"
                && arguments.and_then(|value| value.get("cmd")).and_then(|cmd| cmd.as_str()) == Some("echo hi");
        }
    }

    assert!(saw_started);
    assert!(saw_updated);
    assert!(!saw_completed);
}
#[test]
fn harness_streaming_bridge_tracks_streamed_tool_call_item_ids() {
    let mut bridge = HarnessStreamingBridge::new(None, "turn_tool_map", 5, 2);
    bridge.on_progress(StreamProgressEvent::ToolCallStarted {
        call_id: "call_42".to_string(),
        name: Some("shell".to_string()),
    });

    let item_ids = bridge.take_streamed_tool_call_items();
    assert_eq!(
        item_ids.get("call_42").map(|(item_id, _)| item_id.as_str()),
        Some("turn_tool_map-step-5-assistant-stream-2-tool-call-call_42")
    );
    assert_eq!(item_ids.get("call_42").map(|(_, name)| name.as_str()), Some("shell"));
}
#[tokio::test]
async fn resume_wake_during_non_streaming_request_preserves_fresh_planning() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    ctx.ctrl_c_state.request_local_cancel();
    ctx.ctrl_c_state.mark_cancel_handled();
    ctx.ctrl_c_state.reset();
    let mut provider = ScriptedProvider::new(
        "mycorp",
        false,
        Arc::new(Mutex::new(Vec::new())),
        vec![ScriptedProviderOutcome::Success {
            content: Some("continued planning"),
            request_id: None,
        }],
    );
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    provider.resume_wake = Some(ResumeWake {
        notify: ctx.ctrl_c_notify.clone(),
        entered: entered.clone(),
        release: release.clone(),
    });
    *ctx.provider_client = Box::new(provider);
    ctx.working_history
        .push(uni::Message::user("Continue drafting the proposal.".into()));
    let request = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None);
    let finish = async {
        entered.notified().await;
        task::yield_now().await;
        release.notify_one();
    };
    let (response, ()) = tokio::join!(request, finish);
    let response = response.expect("fresh planning request");
    assert_eq!(response.0.content.as_deref(), Some("continued planning"));
    assert!(ctx.is_planning_active());
    assert!(!ctx.ctrl_c_state.is_cancel_requested());
}
#[tokio::test]
async fn anthropic_fast_capacity_errors_use_existing_bounded_retries_without_clearing_preference() {
    for status in [429, 529] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let provider = ScriptedProvider::new(
            "anthropic",
            false,
            Arc::clone(&calls),
            vec![
                ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                    message: "capacity unavailable".to_owned(),
                    metadata: Some(uni::LLMErrorMetadata::new(
                        "anthropic",
                        Some(status),
                        None,
                        None,
                        None,
                        Some("7".to_owned()),
                        None,
                    )),
                }),
                ScriptedProviderOutcome::Success {
                    content: Some("fast capacity recovered"),
                    request_id: None,
                },
            ],
        );
        let mut backing = TestTurnProcessingBacking::new().await;
        let mut cfg = vtcode_config::VTCodeConfig::default();
        cfg.provider.anthropic.speed = Some(vtcode_config::AnthropicSpeed::Fast);
        backing.set_vt_cfg_for_test(cfg);
        let mut ctx = backing.turn_processing_context();
        *ctx.provider_client = Box::new(provider);
        ctx.working_history.push(uni::Message::user("continue".to_owned()));
        execute_llm_request(&mut ctx, 1, "claude-opus-5-5", Some(320), false, None)
            .await
            .unwrap();
        assert_eq!(calls.lock().unwrap().len(), 2, "status {status}");
        assert_eq!(ctx.vt_cfg.unwrap().provider.anthropic.speed, Some(vtcode_config::AnthropicSpeed::Fast));
    }
}
