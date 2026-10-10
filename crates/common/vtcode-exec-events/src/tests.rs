use super::*;
use std::error::Error;
use std::mem::size_of;

/// `ThreadEvent` is pushed into `Vec`s per streaming delta and accumulated
/// for whole sessions. Large sparse payloads must stay boxed so the enum
/// does not balloon from alignment/discriminant padding (see
/// docs/development/rust-performance-principles.md, "Enum footprint").
#[test]
fn thread_event_stays_compact() {
    assert!(
        size_of::<ThreadEvent>() <= 80,
        "ThreadEvent grew to {} bytes; box new large payloads instead of inlining them",
        size_of::<ThreadEvent>()
    );
}

/// Boxing only pays off while the inline (unboxed) payload is larger than
/// a pointer. Guard each boxed variant against accidental unboxing.
#[test]
fn boxed_thread_item_details_payloads_stay_boxed() {
    assert!(size_of::<Option<Box<CommandExecutionItem>>>() < size_of::<Option<CommandExecutionItem>>());
    assert!(size_of::<Option<Box<ToolInvocationItem>>>() < size_of::<Option<ToolInvocationItem>>());
    assert!(size_of::<Option<Box<ToolOutputItem>>>() < size_of::<Option<ToolOutputItem>>());
    assert!(size_of::<Option<Box<FileChangeItem>>>() < size_of::<Option<FileChangeItem>>());
    assert!(size_of::<Option<Box<McpToolCallItem>>>() < size_of::<Option<McpToolCallItem>>());
    assert!(size_of::<Option<Box<WebSearchItem>>>() < size_of::<Option<WebSearchItem>>());
    assert!(size_of::<Option<Box<HarnessEventItem>>>() < size_of::<Option<HarnessEventItem>>());
}

#[test]
fn file_change_item_optional_diff_fields_round_trip() -> Result<(), Box<dyn Error>> {
    // Legacy payload without the new optional fields must deserialize.
    let legacy_json = r#"{
        "changes": [{"path": "src/main.rs", "kind": "add"}],
        "status": "completed"
    }"#;
    let legacy: FileChangeItem = serde_json::from_str(legacy_json)?;
    assert!(legacy.unified_diff.is_none());
    assert!(legacy.additions.is_none());
    assert!(legacy.deletions.is_none());

    // New fields are omitted from output when unset.
    let legacy_reserialized = serde_json::to_value(&legacy)?;
    assert!(legacy_reserialized.get("unified_diff").is_none());
    assert!(legacy_reserialized.get("additions").is_none());
    assert!(legacy_reserialized.get("deletions").is_none());

    // Populated fields survive a round trip.
    let populated = FileChangeItem {
        diff_incomplete: None,
        changes: legacy.changes.clone(),
        status: PatchApplyStatus::Completed,
        unified_diff: Some("diff --git a/x b/x\n".to_string()),
        additions: Some(3),
        deletions: Some(1),
    };
    let json = serde_json::to_string(&populated)?;
    let restored: FileChangeItem = serde_json::from_str(&json)?;
    assert_eq!(restored, populated);
    Ok(())
}

#[test]
fn thread_event_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage {
            input_tokens: 1,
            cached_input_tokens: 2,
            cache_creation_tokens: 0,
            output_tokens: 3,
        },
        in_progress_exec_sessions: Vec::new(),
    });

    let json = serde_json::to_string(&event)?;
    let restored: ThreadEvent = serde_json::from_str(&json)?;

    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn turn_blocked_event_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::TurnBlocked(Box::new(TurnBlockedEvent {
        completed_at: None,
        message: "Blocked tool-call limit reached after 3 consecutive blocked calls.".to_string(),
        last_tool: Some("exec_command".to_string()),
        blocked_streak: 4,
        blocked_total: 4,
        consecutive_cap: 3,
        total_cap: 6,
        recovery_active: false,
        usage: None,
    }));

    let json = serde_json::to_string(&event)?;
    assert!(json.contains("turn.blocked"));
    let restored: ThreadEvent = serde_json::from_str(&json)?;
    assert_eq!(restored, event);

    // Legacy payloads without new counters still parse via defaults.
    let legacy = serde_json::json!({"type": "turn.blocked", "message": "blocked"});
    let parsed: ThreadEvent = serde_json::from_value(legacy)?;
    assert!(matches!(parsed, ThreadEvent::TurnBlocked(_)));
    Ok(())
}

#[test]
fn turn_completed_in_progress_sessions_default_empty_and_omitted() -> Result<(), Box<dyn Error>> {
    // Legacy payload without the new field must deserialize to empty.
    let legacy = serde_json::json!({
        "type": "turn.completed",
        "usage": {"input_tokens": 1, "cached_input_tokens": 0, "cache_creation_tokens": 0, "output_tokens": 2}
    });
    let parsed: ThreadEvent = serde_json::from_value(legacy)?;
    let ThreadEvent::TurnCompleted(completed) = parsed else {
        panic!("expected turn.completed");
    };
    assert!(completed.in_progress_exec_sessions.is_empty());

    // Empty ids are omitted from output so steady-state streams stay small.
    let json = serde_json::to_value(ThreadEvent::TurnCompleted(completed))?;
    assert!(json.get("in_progress_exec_sessions").is_none());

    // Explicit null degrades to empty instead of failing.
    let null_field = serde_json::json!({
        "type": "turn.completed",
        "usage": {"input_tokens": 0, "cached_input_tokens": 0, "cache_creation_tokens": 0, "output_tokens": 0},
        "in_progress_exec_sessions": null
    });
    let parsed_null: ThreadEvent = serde_json::from_value(null_field)?;
    let ThreadEvent::TurnCompleted(null_completed) = parsed_null else {
        panic!("expected turn.completed");
    };
    assert!(null_completed.in_progress_exec_sessions.is_empty());
    Ok(())
}

#[test]
fn turn_completed_in_progress_sessions_round_trip_and_bound() -> Result<(), Box<dyn Error>> {
    assert_eq!(MAX_IN_PROGRESS_EXEC_SESSIONS, 4);
    let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: vec!["run-1".to_string(), "run-2".to_string()],
    });
    let json = serde_json::to_string(&event)?;
    assert!(json.contains("in_progress_exec_sessions"));
    let restored: ThreadEvent = serde_json::from_str(&json)?;
    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn usage_uncached_input_tokens_saturates() {
    let usage = Usage {
        input_tokens: 1_000,
        cached_input_tokens: 800,
        cache_creation_tokens: 100,
        output_tokens: 50,
    };
    assert_eq!(usage.uncached_input_tokens(), 100);

    let inconsistent = Usage {
        input_tokens: 100,
        cached_input_tokens: 150,
        cache_creation_tokens: 0,
        output_tokens: 0,
    };
    assert_eq!(inconsistent.uncached_input_tokens(), 0);

    let inconsistent_with_creation = Usage {
        input_tokens: 100,
        cached_input_tokens: 80,
        cache_creation_tokens: 50,
        output_tokens: 0,
    };
    assert_eq!(inconsistent_with_creation.uncached_input_tokens(), 0);
}

#[test]
fn usage_cache_hit_rate() {
    assert_eq!(Usage::default().cache_hit_rate(), None);

    let usage = Usage {
        input_tokens: 1_000,
        cached_input_tokens: 750,
        cache_creation_tokens: 0,
        output_tokens: 0,
    };
    let rate = usage.cache_hit_rate().expect("rate");
    assert!((rate - 0.75).abs() < f64::EPSILON);
}

#[test]
fn usage_cache_summary_formats() {
    assert_eq!(Usage::default().cache_summary(), "No input tokens recorded.");

    let usage = Usage {
        input_tokens: 1_000,
        cached_input_tokens: 800,
        cache_creation_tokens: 100,
        output_tokens: 50,
    };
    assert_eq!(
        usage.cache_summary(),
        "Cache: 800 cached / 1000 total input (80.0% hit rate), 100 cache-creation, 100 uncached"
    );
}

#[test]
fn usage_add_accumulates_all_fields_with_saturation() {
    let mut total = Usage {
        input_tokens: 100,
        cached_input_tokens: 20,
        cache_creation_tokens: 5,
        output_tokens: 10,
    };
    total.add(&Usage {
        input_tokens: 50,
        cached_input_tokens: 10,
        cache_creation_tokens: 2,
        output_tokens: 8,
    });

    assert_eq!(total.input_tokens, 150);
    assert_eq!(total.cached_input_tokens, 30);
    assert_eq!(total.cache_creation_tokens, 7);
    assert_eq!(total.output_tokens, 18);

    let mut saturating = Usage {
        input_tokens: u64::MAX,
        cached_input_tokens: u64::MAX,
        cache_creation_tokens: u64::MAX,
        output_tokens: u64::MAX,
    };
    saturating.add(&Usage {
        input_tokens: 1,
        cached_input_tokens: 1,
        cache_creation_tokens: 1,
        output_tokens: 1,
    });
    assert_eq!(saturating.input_tokens, u64::MAX);
    assert_eq!(saturating.cached_input_tokens, u64::MAX);
    assert_eq!(saturating.cache_creation_tokens, u64::MAX);
    assert_eq!(saturating.output_tokens, u64::MAX);
}

#[test]
fn versioned_event_wraps_schema_version() {
    let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "abc".to_string() });

    let versioned = VersionedThreadEvent::new(event.clone());

    assert_eq!(versioned.schema_version, EVENT_SCHEMA_VERSION);
    assert_eq!(versioned.event, event);
    assert_eq!(versioned.into_event(), event);
}

#[test]
fn plan_approval_events_round_trip_with_decision() {
    let requested = ThreadEvent::PlanApprovalRequested(PlanApprovalRequestedEvent {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-2".to_string(),
        plan_file: Some(".vtcode/plans/change.md".to_string()),
    });
    let resolved = ThreadEvent::PlanApprovalResolved(PlanApprovalResolvedEvent {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-3".to_string(),
        decision: PlanApprovalDecision::AutoAccept,
        automatic: false,
    });

    for event in [requested, resolved] {
        let serialized = serde_json::to_string(&event).expect("serialize plan approval event");
        let restored: ThreadEvent = serde_json::from_str(&serialized).expect("deserialize plan approval event");
        assert_eq!(restored, event);
    }
}

#[test]
fn context_reset_event_round_trips_with_handoff_metadata() {
    let event = ThreadEvent::ContextReset(ContextResetEvent {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-3".to_string(),
        trigger: ContextResetTrigger::PlanApproval,
        plan_preserved: true,
        previous_context_usage_percent: 7,
        tool_budget_reset: true,
    });

    let serialized = serde_json::to_string(&event).expect("serialize context reset event");
    let restored: ThreadEvent = serde_json::from_str(&serialized).expect("deserialize context reset event");
    assert_eq!(restored, event);
    assert_eq!(serde_json::to_value(event).expect("wire value")["type"], "context.reset");
}

#[test]
fn plan_approval_decision_uses_stable_wire_names() {
    let event = ThreadEvent::PlanApprovalResolved(PlanApprovalResolvedEvent {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        decision: PlanApprovalDecision::SwitchBuild,
        automatic: false,
    });

    let serialized = serde_json::to_value(event).expect("serialize plan approval decision");
    assert_eq!(serialized["type"], "plan.approval.resolved");
    assert_eq!(serialized["decision"], "switch_build");
}

#[test]
fn plan_approval_decision_is_forward_compatible() {
    let payload = serde_json::json!({
        "type": "plan.approval.resolved",
        "thread_id": "thread-1",
        "turn_id": "turn-1",
        "decision": "future_decision",
        "automatic": true,
    });
    let event: ThreadEvent = serde_json::from_value(payload).expect("future decision should deserialize");
    assert!(matches!(
        event,
        ThreadEvent::PlanApprovalResolved(PlanApprovalResolvedEvent {
            decision: PlanApprovalDecision::Unknown,
            automatic: true,
            ..
        })
    ));
}

#[cfg(feature = "serde-json")]
#[test]
fn versioned_json_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "item-1".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "hello".to_string() }),
        },
    });

    let payload = json::versioned_to_string(&event)?;
    let restored = json::versioned_from_str(&payload)?;

    assert_eq!(restored.schema_version, EVENT_SCHEMA_VERSION);
    assert_eq!(restored.event, event);
    Ok(())
}

#[test]
fn compaction_trigger_serializes_snake_case_and_round_trips() {
    for trigger in [
        CompactionTrigger::Manual,
        CompactionTrigger::Auto,
        CompactionTrigger::Recovery,
        CompactionTrigger::ModelSwitch,
        CompactionTrigger::Unknown,
    ] {
        let json = serde_json::to_string(&trigger).unwrap();
        assert_eq!(json, format!("\"{}\"", trigger.as_str()));
        let restored: CompactionTrigger = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, trigger);
    }
}

#[test]
fn tool_invocation_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "tool_1".to_string(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "read_file".to_string(),
                arguments: Some(serde_json::json!({ "path": "README.md" })),
                tool_call_id: Some("tool_call_0".to_string()),
                status: ToolCallStatus::Completed,
                outcome: None,
            })),
        },
    });

    let json = serde_json::to_string(&event)?;
    let restored: ThreadEvent = serde_json::from_str(&json)?;

    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn tool_outcome_serializes_snake_case() {
    for outcome in [
        ToolOutcome::Success,
        ToolOutcome::Error,
        ToolOutcome::PermissionRejected,
        ToolOutcome::PermissionCancelled,
        ToolOutcome::Followup,
        ToolOutcome::HookDenied,
        ToolOutcome::InvalidTool,
        ToolOutcome::Cancelled,
    ] {
        let json = serde_json::to_string(&outcome).unwrap();
        let restored: ToolOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, outcome);
    }
}

#[test]
fn tool_invocation_outcome_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "tool_1".to_string(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "exec_command".to_string(),
                arguments: Some(serde_json::json!({ "command": ["pwd"] })),
                tool_call_id: Some("tool_call_0".to_string()),
                status: ToolCallStatus::Failed,
                outcome: Some(ToolOutcome::PermissionRejected),
            })),
        },
    });

    let json = serde_json::to_string(&event)?;
    let restored: ThreadEvent = serde_json::from_str(&json)?;

    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn tool_output_round_trip_preserves_raw_tool_call_id() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "tool_1:output".to_string(),
            details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "tool_1".to_string(),
                tool_call_id: Some("tool_call_0".to_string()),
                spool_path: None,
                output: "done".to_string(),
                exit_code: Some(0),
                status: ToolCallStatus::Completed,
            })),
        },
    });

    let json = serde_json::to_string(&event)?;
    let restored: ThreadEvent = serde_json::from_str(&json)?;

    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn harness_item_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "harness_1".to_string(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event: HarnessEventKind::VerificationFailed,
                message: Some("cargo check failed".to_string()),
                command: Some("cargo check".to_string()),
                path: None,
                exit_code: Some(101),
                attempt: None,
                error_category: None,
                duration_ms: None,
                task_id: None,
                session_id: None,
                exec_session_id: None,
                status: None,
                transcript_path: None,
                archive_path: None,
            })),
        },
    });

    let json = serde_json::to_string(&event)?;
    let restored: ThreadEvent = serde_json::from_str(&json)?;

    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn background_completion_harness_item_preserves_terminal_identity() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "background-completion:task:exec:0".to_string(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event: HarnessEventKind::BackgroundSubprocessCompleted,
                message: Some("Background subprocess completed successfully".to_string()),
                command: None,
                path: None,
                exit_code: Some(0),
                attempt: None,
                error_category: None,
                duration_ms: None,
                task_id: Some("task".to_string()),
                session_id: Some("child-session".to_string()),
                exec_session_id: Some("exec-session".to_string()),
                status: Some("stopped".to_string()),
                transcript_path: Some("/tmp/transcript.jsonl".to_string()),
                archive_path: Some("/tmp/archive.json".to_string()),
            })),
        },
    });

    let value = serde_json::to_value(&event)?;
    assert_eq!(value["item"]["event"], "background_subprocess_completed");
    assert_eq!(value["item"]["task_id"], "task");
    assert_eq!(value["item"]["exec_session_id"], "exec-session");
    assert_eq!(value["item"]["status"], "stopped");

    let restored: ThreadEvent = serde_json::from_value(value)?;
    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn blocked_handoff_resolved_uses_stable_wire_name() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "harness_resolved".to_string(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event: HarnessEventKind::BlockedHandoffResolved,
                message: Some("resolved".to_string()),
                command: None,
                path: None,
                exit_code: None,
                attempt: None,
                error_category: None,
                duration_ms: None,
                task_id: None,
                session_id: None,
                exec_session_id: None,
                status: None,
                transcript_path: None,
                archive_path: None,
            })),
        },
    });

    let value = serde_json::to_value(&event)?;
    assert_eq!(value["item"]["event"], "blocked_handoff_resolved");

    let restored: ThreadEvent = serde_json::from_value(value)?;
    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn thread_completed_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
        completed_at: None,
        thread_id: "thread-1".to_string(),
        session_id: "session-1".to_string(),
        subtype: ThreadCompletionSubtype::ErrorMaxBudgetUsd,
        outcome_code: "budget_limit_reached".to_string(),
        result: None,
        stop_reason: Some("max_tokens".to_string()),
        usage: Usage {
            input_tokens: 10,
            cached_input_tokens: 4,
            cache_creation_tokens: 2,
            output_tokens: 5,
        },
        total_cost_usd: serde_json::Number::from_f64(1.25),
        num_turns: 3,
    }));

    let json = serde_json::to_string(&event)?;
    let restored: ThreadEvent = serde_json::from_str(&json)?;

    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn compact_boundary_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::ThreadCompactBoundary(Box::new(ThreadCompactBoundaryEvent {
        thread_id: "thread-1".to_string(),
        trigger: CompactionTrigger::Recovery,
        mode: CompactionMode::Provider,
        original_message_count: 12,
        compacted_message_count: 5,
        history_artifact_path: Some("/tmp/history.jsonl".to_string()),
        previous_segment_id: Some("segment-0001".to_string()),
        new_segment_id: Some("segment-0002".to_string()),
        previous_prefix_hash: Some("prefix-before".to_string()),
        new_prefix_hash: Some("prefix-after".to_string()),
        previous_catalog_hash: Some("catalog-before".to_string()),
        new_catalog_hash: Some("catalog-after".to_string()),
    }));

    let json = serde_json::to_string(&event)?;
    let restored: ThreadEvent = serde_json::from_str(&json)?;

    assert_eq!(restored, event);
    Ok(())
}

#[test]
fn compact_boundary_deserializes_legacy_payload_without_segment_metadata() -> Result<(), Box<dyn Error>> {
    let payload = r#"{
        "type":"thread.compact_boundary",
        "thread_id":"thread-1",
        "trigger":"recovery",
        "mode":"provider",
        "original_message_count":12,
        "compacted_message_count":5
    }"#;

    let restored: ThreadEvent = serde_json::from_str(payload)?;
    let ThreadEvent::ThreadCompactBoundary(event) = restored else {
        panic!("expected thread.compact_boundary event");
    };

    assert_eq!(event.thread_id, "thread-1");
    assert_eq!(event.history_artifact_path, None);
    assert_eq!(event.previous_segment_id, None);
    assert_eq!(event.new_segment_id, None);
    assert_eq!(event.previous_prefix_hash, None);
    assert_eq!(event.new_prefix_hash, None);
    assert_eq!(event.previous_catalog_hash, None);
    assert_eq!(event.new_catalog_hash, None);
    Ok(())
}
