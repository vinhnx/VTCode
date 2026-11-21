pub(super) use super::*;
pub(super) use std::collections::BTreeMap;
pub(super) use std::error::Error;
pub(super) use std::mem::size_of;

mod events;

/// `ThreadEvent` is pushed into `Vec`s per streaming delta and accumulated
/// for whole sessions. Large sparse payloads must stay boxed so the enum
/// does not balloon from alignment/discriminant padding (see
/// docs/development/rust-performance-principles.md, "Enum footprint").
#[test]
fn wire_name_matches_serialized_type_tag() {
    let item = ThreadItem {
        context: None,
        id: "item-1".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "hello".to_string() }),
    };
    // One representative of every variant: a serde rename added to a new
    // variant without a `wire_type` const (or vice versa) must fail here,
    // not drift for downstream consumers that compare event types by string.
    let events = [
        ThreadEvent::MatrixUpdated(Box::new(matrix::MatrixSnapshot {
            spec: matrix::MatrixSpec {
                id: "spec-1".to_string(),
                tasks: vec![matrix::MatrixTaskSpec {
                    id: "task-1".to_string(),
                    instructions: "do the thing".to_string(),
                    dependencies: Vec::new(),
                    workspace: ".".to_string(),
                    access: matrix::WorkspaceAccess::Read,
                    checks: Vec::new(),
                    resources: BTreeMap::new(),
                    timeout_secs: 60,
                    replay_safe: false,
                    inputs: Vec::new(),
                }],
                resources: BTreeMap::new(),
            },
            lifecycle: matrix::MatrixLifecycle::Running,
            tasks: vec![matrix::MatrixTaskState {
                id: "task-1".to_string(),
                status: matrix::MatrixTaskStatus::Queued,
                attempts: Vec::new(),
                automatic_retries: 0,
            }],
            generation: None,
            revision: 0,
        })),
        ThreadEvent::TrackerUpdated(Box::new(tracker::TrackerSnapshot {
            title: "checklist".to_string(),
            completed: 0,
            total: 1,
            items: Vec::new(),
        })),
        ThreadEvent::GoalUpdated(Box::new(goal::GoalUpdatedEvent { goal: None })),
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread-1".to_string() }),
        ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
            completed_at: None,
            thread_id: "thread-1".to_string(),
            session_id: "session-1".to_string(),
            subtype: ThreadCompletionSubtype::Success,
            outcome_code: "success".to_string(),
            result: None,
            stop_reason: None,
            usage: Usage::default(),
            total_cost_usd: None,
            num_turns: 1,
        })),
        ThreadEvent::ThreadCompactBoundary(Box::new(ThreadCompactBoundaryEvent {
            thread_id: "thread-1".to_string(),
            trigger: CompactionTrigger::Manual,
            mode: CompactionMode::Local,
            original_message_count: 2,
            compacted_message_count: 1,
            history_artifact_path: None,
            previous_segment_id: None,
            new_segment_id: None,
            previous_prefix_hash: None,
            new_prefix_hash: None,
            previous_catalog_hash: None,
            new_catalog_hash: None,
        })),
        ThreadEvent::ContextReset(ContextResetEvent {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            trigger: ContextResetTrigger::PlanApproval,
            plan_preserved: true,
            previous_context_usage_percent: 7,
            tool_budget_reset: true,
        }),
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            final_response_item_id: None,
            completed_at: None,
            usage: Usage {
                input_tokens: 1,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
                output_tokens: 1,
            },
            in_progress_exec_sessions: Vec::new(),
        }),
        ThreadEvent::TurnFailed(TurnFailedEvent {
            completed_at: None,
            message: "boom".to_string(),
            usage: None,
        }),
        ThreadEvent::TurnBlocked(Box::new(TurnBlockedEvent {
            completed_at: None,
            message: "fuse tripped".to_string(),
            last_tool: Some("exec_command".to_string()),
            blocked_streak: 3,
            blocked_total: 4,
            consecutive_cap: 3,
            total_cap: 29,
            recovery_active: false,
            usage: None,
        })),
        ThreadEvent::ItemStarted(ItemStartedEvent { item: item.clone() }),
        ThreadEvent::ItemUpdated(ItemUpdatedEvent { item: item.clone() }),
        ThreadEvent::ItemCompleted(ItemCompletedEvent { item }),
        ThreadEvent::AppSandboxDenied(Box::new(AppSandboxDeniedEvent {
            tool: "mcp__server__tool".to_string(),
            tier: "off".to_string(),
            uris: vec!["ui://server/tool".to_string()],
        })),
        ThreadEvent::PermissionRequested(PermissionRequestedEvent {
            tool_name: "exec_command".to_string(),
            tool_call_id: None,
        }),
        ThreadEvent::PermissionResolved(PermissionResolvedEvent {
            tool_name: "exec_command".to_string(),
            tool_call_id: None,
            decision: PermissionDecision::Allow,
            wait_ms: 5,
        }),
        ThreadEvent::Interjected(InterjectedEvent {
            text: None,
            source: InterjectionSource::Direct,
            image_count: 0,
            redirect_kind: RedirectKind::Interjection,
        }),
        ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "plan-1".to_string(),
            delta: "step one".to_string(),
            recovery: None,
        })),
        ThreadEvent::PlanApprovalRequested(PlanApprovalRequestedEvent {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            plan_file: None,
        }),
        ThreadEvent::PlanApprovalResolved(PlanApprovalResolvedEvent {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            decision: PlanApprovalDecision::Execute,
            automatic: false,
        }),
        ThreadEvent::Error(ThreadErrorEvent { message: "fatal".to_string() }),
        ThreadEvent::Unknown,
    ];
    for event in events {
        let value = serde_json::to_value(&event).expect("serialize event");
        assert_eq!(value["type"].as_str(), Some(event.wire_name()), "{event:?}");
    }
}

#[test]
fn status_as_str_matches_serde_snake_case() {
    use crate::{CommandExecutionStatus, McpToolCallStatus, PatchApplyStatus, ToolCallStatus, ToolOutcome};
    fn check<T: Serialize>(value: &T, spelled: &str, label: &str) {
        let wire = serde_json::to_value(value).expect("serialize status");
        assert_eq!(wire.as_str(), Some(spelled), "{label}");
    }
    for status in [
        CommandExecutionStatus::Completed,
        CommandExecutionStatus::Failed,
        CommandExecutionStatus::InProgress,
    ] {
        check(&status, status.as_str(), "CommandExecutionStatus");
    }
    for status in [
        ToolCallStatus::Completed,
        ToolCallStatus::Failed,
        ToolCallStatus::InProgress,
    ] {
        check(&status, status.as_str(), "ToolCallStatus");
    }
    for status in [PatchApplyStatus::Completed, PatchApplyStatus::Failed] {
        check(&status, status.as_str(), "PatchApplyStatus");
    }
    for status in [
        McpToolCallStatus::Started,
        McpToolCallStatus::Completed,
        McpToolCallStatus::Failed,
    ] {
        check(&status, status.as_str(), "McpToolCallStatus");
    }
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
        check(&outcome, outcome.as_str(), "ToolOutcome");
    }
}

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
        final_response_item_id: Some(Box::new("turn-42-final".to_string())),
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
fn permission_events_preserve_optional_tool_call_ids() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::PermissionResolved(PermissionResolvedEvent {
        tool_name: "web_fetch".to_string(),
        tool_call_id: Some("call-123".to_string()),
        decision: PermissionDecision::Allow,
        wait_ms: 2,
    });

    let json = serde_json::to_value(&event)?;
    assert_eq!(json["tool_call_id"], "call-123");
    let restored: ThreadEvent = serde_json::from_value(json)?;
    assert_eq!(restored, event);

    let legacy = serde_json::json!({
        "type": "permission.requested",
        "tool_name": "web_fetch"
    });
    let parsed: ThreadEvent = serde_json::from_value(legacy)?;
    let ThreadEvent::PermissionRequested(requested) = parsed else {
        panic!("expected permission.requested");
    };
    assert_eq!(requested.tool_call_id, None);
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
fn app_sandbox_denied_event_round_trip() -> Result<(), Box<dyn Error>> {
    let event = ThreadEvent::AppSandboxDenied(Box::new(AppSandboxDeniedEvent {
        tool: "mcp::demo::get_time".to_string(),
        tier: "off".to_string(),
        uris: vec!["ui://demo/app.html".to_string()],
    }));

    let json = serde_json::to_string(&event)?;
    assert!(json.contains("app.sandbox_denied"));
    let restored: ThreadEvent = serde_json::from_str(&json)?;
    assert_eq!(restored, event);
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
        final_response_item_id: None,
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
