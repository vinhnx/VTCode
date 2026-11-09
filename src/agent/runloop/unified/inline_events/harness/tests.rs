use super::*;
use crate::agent::runloop::unified::run_loop_context::TurnRunId;
use serde_json::json;
use std::fs;
use tempfile::TempDir;
use vtcode_core::exec::events::ItemStartedEvent;
use vtcode_memory::DEFAULT_MAX_EVENTS;

#[test]
fn resolve_event_log_path_appends_jsonl_when_directory() {
    let tmp = TempDir::new().expect("temp dir");
    let run_id = TurnRunId("run-123".to_string());
    let resolved = resolve_event_log_path(tmp.path().to_str().expect("path"), &run_id);

    let file_name = resolved.file_name().and_then(|name| name.to_str()).expect("file name");
    assert!(file_name.starts_with("harness-run-123-"));
    assert!(file_name.ends_with(".jsonl"));
}

#[test]
fn tool_outcome_observation_carries_attempt_duration_and_category() {
    let event = tool_outcome_observation_event("read_file", 2, 37, Some("Network".to_string()));
    let ThreadEvent::ItemCompleted(completed) = event else {
        panic!("expected completed harness item");
    };
    let ThreadItemDetails::Harness(item) = completed.item.details else {
        panic!("expected harness details");
    };
    assert_eq!(item.event, HarnessEventKind::ToolLatencyRecorded);
    assert_eq!(item.attempt, Some(2));
    assert_eq!(item.duration_ms, Some(37));
    assert_eq!(item.error_category.as_deref(), Some("Network"));
}

#[test]
fn emit_writes_versioned_event() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).expect("emitter");

    // Use the path method to verify it works
    assert_eq!(emitter.path(), path.as_path());

    emitter.emit(turn_started_event()).expect("emit");

    let payload = fs::read_to_string(&path).expect("read log");
    let line = payload.lines().next().expect("line");
    let value: Value = serde_json::from_str(line).expect("json");

    assert_eq!(
        value.get("schema_version").and_then(|v| v.as_str()),
        Some(vtcode_core::exec::events::EVENT_SCHEMA_VERSION)
    );
    assert_eq!(value.get("event").and_then(|v| v.get("type")).and_then(|v| v.as_str()), Some("turn.started"));
}

#[test]
fn assistant_message_fallback_is_written_as_completed_item() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).expect("emitter");

    let item_id = emitter
        .emit_assistant_message("turn-1", "The turn was blocked; retry the request.")
        .expect("emit assistant message")
        .expect("nonempty message has an identity");

    let content = fs::read_to_string(path).expect("read harness log");
    assert!(content.contains(&item_id));
    assert!(content.contains("item.completed"));
    assert!(content.contains("agent_message"));
    assert!(content.contains("The turn was blocked; retry the request."));
}

#[test]
fn open_responses_integration_writes_sse_events() {
    let tmp = TempDir::new().expect("temp dir");
    let harness_path = tmp.path().join("harness.jsonl");
    let or_path = tmp.path().join("open-responses.jsonl");

    let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");

    // Enable Open Responses
    let config = OpenResponsesConfig {
        enabled: true,
        emit_events: true,
        include_extensions: true,
        map_tool_calls: true,
        include_reasoning: true,
    };
    emitter
        .enable_open_responses(config, "claude-sonnet-5", Some(or_path.clone()))
        .expect("enable");

    // Emit events
    emitter
        .emit(ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "test-thread".to_string() }))
        .expect("emit");
    emitter.emit(turn_started_event()).expect("emit turn");
    emitter
        .emit(turn_completed_event(Usage {
            input_tokens: 12,
            cached_input_tokens: 3,
            cache_creation_tokens: 0,
            output_tokens: 5,
        }))
        .expect("emit completed");
    emitter.finish_open_responses();

    // Verify harness log
    let harness_content = fs::read_to_string(&harness_path).expect("read harness");
    assert!(harness_content.contains("thread.started"));
    assert!(harness_content.contains("turn.started"));

    // Verify Open Responses log
    let or_content = fs::read_to_string(&or_path).expect("read OR");
    assert!(or_content.contains("response.created"));
    assert!(or_content.contains("response.completed"));
    assert!(or_content.contains("\n\n"), "SSE events must be separated by a blank line");
    assert!(or_content.contains("[DONE]"));
}

#[tokio::test]
async fn async_emitter_drains_large_canonical_batch_and_uses_unique_ids() {
    let tmp = TempDir::new().expect("temp dir");
    let emitter = HarnessEventEmitter::new_async(tmp.path(), "large-session", None)
        .await
        .expect("canonical emitter");

    for index in 0..3_253 {
        emitter
            .emit(harness_event(
                HarnessEventKind::ToolLatencyRecorded,
                Some(format!("event-{index}")),
                None,
                None,
                None,
            ))
            .expect("emit canonical event");
    }
    emitter.finish().await.expect("canonical drain");

    let event_path = tmp.path().join(".vtcode/sessions/large-session/events.jsonl");
    let lines = fs::read_to_string(event_path)
        .expect("read canonical events")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 3_253);

    let mut ids = std::collections::HashSet::new();
    for line in lines {
        let event: VersionedThreadEvent = serde_json::from_str(&line).expect("decode canonical event");
        let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event.into_event() else {
            panic!("expected harness item event");
        };
        assert!(ids.insert(item.id), "harness item id was duplicated");
    }
}

#[tokio::test]
async fn optional_legacy_export_setup_failure_does_not_disable_canonical_persistence() {
    let tmp = TempDir::new().expect("temp dir");
    let legacy_path = tmp.path().join("legacy-directory");
    fs::create_dir(&legacy_path).expect("create legacy directory");

    let emitter = HarnessEventEmitter::new_async(tmp.path(), "canonical-session", Some(legacy_path))
        .await
        .expect("canonical emitter should remain available");
    emitter.emit(turn_started_event()).expect("emit canonical event");
    emitter.finish().await.expect("canonical drain");

    let event_path = tmp.path().join(".vtcode/sessions/canonical-session/events.jsonl");
    let event_lines = fs::read_to_string(event_path).expect("read canonical events").lines().count();
    assert_eq!(event_lines, 1);
}

#[tokio::test]
async fn webmcp_export_failure_does_not_disable_canonical_persistence() {
    let tmp = TempDir::new().expect("temp dir");
    let emitter = HarnessEventEmitter::new_async(tmp.path(), "webmcp-failure-session", None)
        .await
        .expect("canonical emitter");
    let hub = WebmcpEventHub::new_with_max_event_bytes(EventHubConfig::default(), 1).expect("bounded hub");
    emitter.attach_webmcp_event_hub(hub).expect("attach WebMCP hub");

    emitter
        .emit(turn_started_event())
        .expect("optional WebMCP failure must be isolated");
    emitter.finish().await.expect("canonical emitter should still finish");

    let event_path = tmp.path().join(".vtcode/sessions/webmcp-failure-session/events.jsonl");
    let event_lines = fs::read_to_string(event_path).expect("read canonical events").lines().count();
    assert_eq!(event_lines, 1);
}

#[tokio::test]
async fn async_exporters_share_order_and_finalize_once() {
    let tmp = TempDir::new().expect("temp dir");
    let legacy_path = tmp.path().join("legacy.jsonl");
    let emitter = HarnessEventEmitter::new_async(tmp.path(), "ordered-session", Some(legacy_path.clone()))
        .await
        .expect("canonical emitter");
    let config = OpenResponsesConfig {
        enabled: true,
        emit_events: true,
        include_extensions: true,
        map_tool_calls: true,
        include_reasoning: true,
    };
    let open_responses_path = tmp.path().join("open-responses.jsonl");
    emitter
        .enable_open_responses_async(config, "test-model", Some(open_responses_path.clone()))
        .await
        .expect("Open Responses exporter");

    let events = vec![
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "ordered-thread".to_string() }),
        turn_started_event(),
        turn_completed_event(Usage::default()),
    ];
    for event in events {
        emitter.emit(event).expect("emit ordered event");
    }
    emitter.finish().await.expect("finish exporters");
    emitter.finish().await.expect("second finish is idempotent");

    let canonical = fs::read_to_string(tmp.path().join(".vtcode/sessions/ordered-session/events.jsonl"))
        .expect("read canonical events");
    let legacy = fs::read_to_string(legacy_path).expect("read legacy events");
    assert_eq!(canonical.lines().collect::<Vec<_>>(), legacy.lines().collect::<Vec<_>>());

    let open_responses = fs::read_to_string(open_responses_path).expect("read Open Responses events");
    assert!(open_responses.contains("response.created"));
    assert_eq!(open_responses.matches("data: [DONE]").count(), 1);
}

#[tokio::test]
async fn canonical_lifecycle_status_changes_only_at_thread_completion() {
    let tmp = TempDir::new().expect("temp dir");
    let emitter = HarnessEventEmitter::new_async(tmp.path(), "lifecycle-session", None)
        .await
        .expect("canonical emitter");
    emitter
        .emit(ThreadEvent::TurnStarted(TurnStartedEvent::default()))
        .expect("turn start");
    emitter.emit(turn_completed_event(Usage::default())).expect("turn completion");
    emitter.finish().await.expect("finish active session");

    let active = vtcode_memory::open(tmp.path(), "lifecycle-session", DEFAULT_MAX_EVENTS).expect("open active session");
    assert_eq!(active.manifest().status, "active");
}

#[test]
fn turn_completed_event_preserves_usage_payload() {
    let before = chrono::Utc::now();
    let event = turn_completed_event(Usage {
        input_tokens: 42,
        cached_input_tokens: 7,
        cache_creation_tokens: 0,
        output_tokens: 9,
    });

    let ThreadEvent::TurnCompleted(TurnCompletedEvent { completed_at: Some(completed_at), usage, .. }) = event else {
        panic!("expected turn.completed");
    };

    let completed_at = chrono::DateTime::parse_from_rfc3339(&completed_at).expect("valid completion timestamp");
    assert!(completed_at >= before && completed_at <= chrono::Utc::now());

    assert_eq!(usage.input_tokens, 42);
    assert_eq!(usage.cached_input_tokens, 7);
    assert_eq!(usage.cache_creation_tokens, 0);
    assert_eq!(usage.output_tokens, 9);
}

#[test]
fn turn_completed_event_with_sessions_caps_ids() {
    let sessions = vec![
        "run-1".to_string(),
        "run-2".to_string(),
        "run-3".to_string(),
        "run-4".to_string(),
        "run-5".to_string(),
        "run-6".to_string(),
    ];
    let event = turn_completed_event_with_sessions(Usage::default(), sessions);
    let ThreadEvent::TurnCompleted(completed) = event else {
        panic!("expected turn.completed");
    };
    assert_eq!(
        completed.in_progress_exec_sessions,
        vec![
            "run-1".to_string(),
            "run-2".to_string(),
            "run-3".to_string(),
            "run-4".to_string(),
        ]
    );

    let empty = turn_completed_event(Usage::default());
    let ThreadEvent::TurnCompleted(empty_completed) = empty else {
        panic!("expected turn.completed");
    };
    assert!(empty_completed.in_progress_exec_sessions.is_empty());
}

#[test]
fn tool_started_event_captures_arguments() {
    let args = json!({ "path": "README.md" });
    let event = tool_started_event("tool-1".to_string(), "read_file", Some(&args), Some("tool_call_0"));

    let ThreadEvent::ItemStarted(ItemStartedEvent { item }) = event else {
        panic!("expected item.started");
    };
    let ThreadItemDetails::ToolInvocation(details) = item.details else {
        panic!("expected tool invocation item");
    };

    assert_eq!(details.tool_name, "read_file");
    assert_eq!(details.arguments, Some(json!({ "path": "README.md" })));
    assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
    assert_eq!(details.status, ToolCallStatus::InProgress);
}

#[test]
fn tool_invocation_completed_event_captures_raw_tool_call_id() {
    let args = json!({ "path": "README.md" });
    let event = tool_invocation_completed_event(
        "tool-1".to_string(),
        "read_file",
        Some(&args),
        Some("tool_call_0"),
        ToolCallStatus::Completed,
    );

    let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
        panic!("expected item.completed");
    };
    let ThreadItemDetails::ToolInvocation(details) = item.details else {
        panic!("expected tool invocation item");
    };

    assert_eq!(details.tool_name, "read_file");
    assert_eq!(details.arguments, Some(json!({ "path": "README.md" })));
    assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
    assert_eq!(details.status, ToolCallStatus::Completed);
}

#[test]
fn tool_output_completed_event_captures_output() {
    let event = tool_output_completed_event(
        "tool-1".to_string(),
        Some("tool_call_0"),
        ToolCallStatus::Completed,
        Some(0),
        None,
        "On branch main",
        Vec::new(),
    );

    let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
        panic!("expected item.completed");
    };
    assert_eq!(item.id, "tool-1:output");
    let ThreadItemDetails::ToolOutput(details) = item.details else {
        panic!("expected tool output item");
    };

    assert_eq!(details.call_id, "tool-1");
    assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
    assert_eq!(details.spool_path, None);
    assert_eq!(details.output, "On branch main");
    assert_eq!(details.exit_code, Some(0));
    assert_eq!(details.status, ToolCallStatus::Completed);
}

#[test]
fn tool_output_started_event_starts_empty_output_item() {
    let event = tool_output_started_event("tool-1".to_string(), Some("tool_call_0"));

    let ThreadEvent::ItemStarted(ItemStartedEvent { item }) = event else {
        panic!("expected item.started");
    };
    assert_eq!(item.id, "tool-1:output");
    let ThreadItemDetails::ToolOutput(details) = item.details else {
        panic!("expected tool output item");
    };

    assert_eq!(details.call_id, "tool-1");
    assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
    assert_eq!(details.spool_path, None);
    assert!(details.output.is_empty());
    assert_eq!(details.status, ToolCallStatus::InProgress);
}

#[test]
fn tool_updated_event_captures_streamed_output() {
    let event = tool_updated_event("tool-1".to_string(), Some("tool_call_0"), "On branch main");

    let ThreadEvent::ItemUpdated(vtcode_core::exec::events::ItemUpdatedEvent { item }) = event else {
        panic!("expected item.updated");
    };
    assert_eq!(item.id, "tool-1:output");
    let ThreadItemDetails::ToolOutput(details) = item.details else {
        panic!("expected tool output item");
    };

    assert_eq!(details.call_id, "tool-1");
    assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
    assert_eq!(details.spool_path, None);
    assert_eq!(details.output, "On branch main");
    assert_eq!(details.status, ToolCallStatus::InProgress);
}

#[test]
fn session_message_event_carries_peer_and_correlation() {
    let event = harness_session_message_event(
        HarnessEventKind::SessionMessageReceived,
        Some("answer from session peer-a".to_string()),
        Some("peer-a".to_string()),
        Some("msg-1".to_string()),
    );
    let ThreadEvent::ItemCompleted(completed) = event else {
        panic!("expected item.completed");
    };
    let ThreadItemDetails::Harness(item) = completed.item.details else {
        panic!("expected harness item");
    };
    assert_eq!(item.event, HarnessEventKind::SessionMessageReceived);
    assert_eq!(item.session_id.as_deref(), Some("peer-a"));
    assert_eq!(item.correlation_id.as_deref(), Some("msg-1"));
    assert!(item.task_id.is_none(), "no task identity is fabricated");
}
