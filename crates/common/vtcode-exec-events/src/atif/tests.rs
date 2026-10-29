use super::*;
use crate::{
    AgentMessageItem, AppSandboxDeniedEvent, CompactionMode, CompactionTrigger, HarnessEventItem, HarnessEventKind,
    ItemCompletedEvent, ThreadCompactBoundaryEvent, ThreadItem, ThreadStartedEvent, ToolInvocationItem, ToolOutputItem,
    TurnCompletedEvent, TurnStartedEvent, Usage,
};

fn fixed_ts() -> DateTime<Utc> {
    "2025-01-15T10:30:00Z".parse().unwrap()
}

#[test]
fn terminal_timestamps_survive_export_without_synthetic_steps() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let timestamp = "2026-10-03T01:02:03Z";
    let blocked = serde_json::from_value(
        serde_json::json!({"type":"turn.blocked", "message":"Blocked", "completed_at":timestamp}),
    )
    .unwrap();
    builder.process_event_at(&blocked, fixed_ts());
    let completed = serde_json::from_value(serde_json::json!({"type":"thread.completed", "completed_at":timestamp, "thread_id":"thread", "session_id":"session", "subtype":"success", "outcome_code":"completed", "usage":Usage::default(), "num_turns":1})).unwrap();
    builder.process_event_at(&completed, fixed_ts());
    let trajectory = builder.finish(None);
    assert_eq!(trajectory.steps.len(), 1);
    assert_eq!(trajectory.steps[0].timestamp.as_deref(), Some(timestamp));
    assert_eq!(trajectory.extra.as_ref().unwrap()["completed_at"], timestamp);
}

#[test]
fn sandbox_denied_projects_to_system_step_with_refused_uris() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    builder.process_event_at(
        &ThreadEvent::AppSandboxDenied(Box::new(AppSandboxDeniedEvent {
            tool: "mcp::demo::get_time".to_string(),
            tier: "off".to_string(),
            uris: vec!["ui://demo/app.html".to_string(), "ui://demo/other.html".to_string()],
        })),
        fixed_ts(),
    );
    let trajectory = builder.finish(None);
    assert_eq!(trajectory.steps.len(), 1);
    let step = &trajectory.steps[0];
    let message = step.message.as_deref().unwrap_or_default();
    assert!(message.contains("mcp::demo::get_time"), "step must name the tool: {message}");
    assert!(message.contains("ui://demo/app.html"), "step must name refused URIs: {message}");
    let extra = step.extra.as_ref().expect("denial step must carry structured extra");
    assert_eq!(extra["tier"], "off");
    assert_eq!(extra["uris"], serde_json::json!(["ui://demo/app.html", "ui://demo/other.html"]));
}

#[test]
fn explanation_metadata_and_public_rationale_preserve_recorded_time() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let context = crate::ItemContext {
        task_id: "task-1".into(),
        turn_id: "turn-1".into(),
        actor_id: "root".into(),
        parent_actor_id: None,
        timestamp: "2026-10-03T01:02:03Z".into(),
        activity: None,
    };
    builder.process_event_at(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "decision-1".into(),
                context: Some(Box::new(context.clone())),
                details: ThreadItemDetails::Decision(Box::new(crate::DecisionItem {
                    summary: "Reuse parser".into(),
                    rationale: "Preserve checks".into(),
                    alternatives: vec!["Replace parser".into()],
                    evidence_ids: vec!["read-1".into()],
                })),
            },
        }),
        fixed_ts(),
    );
    let trajectory = builder.finish(None);
    assert_eq!(trajectory.steps.len(), 1);
    let step = &trajectory.steps[0];
    assert_eq!(step.timestamp.as_deref(), Some(context.timestamp.as_str()));
    let extra = step.extra.as_ref().unwrap();
    assert_eq!(extra["public_rationale"], "Preserve checks");
    assert_eq!(extra["item_context"]["task_id"], "task-1");
    assert_eq!(extra["evidence_ids"][0], "read-1");
}

#[test]
fn trajectory_round_trip() {
    let trajectory = Trajectory {
        schema_version: ATIF_SCHEMA_VERSION.to_string(),
        session_id: "test-session".to_string(),
        agent: AtifAgent::vtcode(),
        steps: vec![Step::user(1, "hello")],
        notes: None,
        final_metrics: None,
        extra: None,
    };

    let json = serde_json::to_string_pretty(&trajectory).unwrap();
    let restored: Trajectory = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.schema_version, ATIF_SCHEMA_VERSION);
    assert_eq!(restored.session_id, "test-session");
    assert_eq!(restored.steps.len(), 1);
}

#[test]
fn builder_thread_started_sets_session_id() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread-abc".to_string() });
    builder.process_event_at(&event, fixed_ts());
    let trajectory = builder.finish(None);
    assert_eq!(trajectory.session_id, "thread-abc");
}

#[test]
fn builder_agent_message_step() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "msg-1".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Hello, world!".to_string() }),
        },
    });
    builder.process_event_at(&event, fixed_ts());
    let trajectory = builder.finish(None);

    assert_eq!(trajectory.steps.len(), 1);
    let step = &trajectory.steps[0];
    assert_eq!(step.step_id, 1);
    assert_eq!(step.source, StepSource::Agent);
    assert_eq!(step.message.as_deref(), Some("Hello, world!"));
}

#[test]
fn background_completion_preserves_identity_in_atif_extra() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "background-completion:task-1:exec-1".to_string(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event: HarnessEventKind::BackgroundSubprocessCompleted,
                message: Some("completed successfully".to_string()),
                command: None,
                path: None,
                exit_code: Some(0),
                attempt: None,
                error_category: Some("background_subprocess".to_string()),
                duration_ms: None,
                task_id: Some("task-1".to_string()),
                session_id: Some("session-1".to_string()),
                exec_session_id: Some("exec-1".to_string()),
                status: Some("stopped".to_string()),
                transcript_path: Some("/tmp/transcript.jsonl".to_string()),
                archive_path: Some("/tmp/archive.json".to_string()),
            })),
        },
    });

    builder.process_event_at(&event, fixed_ts());
    let trajectory = builder.finish(None);
    let step = trajectory.steps.first().expect("background completion step");
    let extra = step.extra.as_ref().expect("background completion metadata");
    assert_eq!(step.message.as_deref(), Some("harness: BackgroundSubprocessCompleted"));
    assert_eq!(extra["harness_message"], "completed successfully");
    assert_eq!(extra["task_id"], "task-1");
    assert_eq!(extra["session_id"], "session-1");
    assert_eq!(extra["exec_session_id"], "exec-1");
    assert_eq!(extra["status"], "stopped");
    assert_eq!(extra["exit_code"], 0);
    assert_eq!(extra["transcript_path"], "/tmp/transcript.jsonl");
    assert_eq!(extra["archive_path"], "/tmp/archive.json");
    assert_eq!(extra["error_category"], "background_subprocess");
}

#[test]
fn builder_tool_invocation_with_output() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let ts = fixed_ts();

    // Tool invocation
    let inv_event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "tool_1".to_string(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "read_file".to_string(),
                arguments: Some(serde_json::json!({"path": "README.md"})),
                tool_call_id: Some("tc_0".to_string()),
                status: ToolCallStatus::Completed,
                outcome: None,
            })),
        },
    });
    builder.process_event_at(&inv_event, ts);

    // Tool output
    let out_event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "tool_1:output".to_string(),
            details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "tool_1".to_string(),
                tool_call_id: Some("tc_0".to_string()),
                spool_path: None,
                output: "file contents here".to_string(),
                exit_code: Some(0),
                status: ToolCallStatus::Completed,
                app_previews: Vec::new(),
            })),
        },
    });
    builder.process_event_at(&out_event, ts);

    let trajectory = builder.finish(None);
    // Only one step: the invocation is buffered until output arrives
    assert_eq!(trajectory.steps.len(), 1);
    let step = &trajectory.steps[0];
    assert_eq!(step.source, StepSource::Agent);

    let calls = step.tool_calls.as_ref().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].function_name, "read_file");
    assert_eq!(calls[0].tool_call_id, "tc_0");

    let obs = step.observation.as_ref().unwrap();
    assert_eq!(obs.results.len(), 1);
    assert_eq!(obs.results[0].content, "file contents here");
}

#[test]
fn builder_turn_completed_accumulates_metrics() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage {
            input_tokens: 500,
            cached_input_tokens: 100,
            cache_creation_tokens: 0,
            output_tokens: 200,
        },
        in_progress_exec_sessions: Vec::new(),
    });
    builder.process_event_at(&event, fixed_ts());

    let trajectory = builder.finish(None);
    let fm = trajectory.final_metrics.as_ref().unwrap();
    assert_eq!(fm.total_prompt_tokens, Some(500));
    assert_eq!(fm.total_completion_tokens, Some(200));
    assert_eq!(fm.total_cached_tokens, Some(100));
}

#[test]
fn builder_turn_completed_preserves_in_progress_sessions_for_resume() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: vec!["run-7".to_string()],
    });
    builder.process_event_at(&event, fixed_ts());

    let trajectory = builder.finish(None);
    let step = trajectory.steps.last().expect("turn_completed step");
    let extra = step.extra.clone().expect("extra carries resume ids");
    assert_eq!(extra["in_progress_exec_sessions"], serde_json::json!(["run-7"]));

    // Empty ids stay omitted so steady-state export is unchanged.
    let mut empty_builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let empty = ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    });
    empty_builder.process_event_at(&empty, fixed_ts());
    let empty_trajectory = empty_builder.finish(None);
    assert!(empty_trajectory.steps.last().expect("step").extra.is_none());
}

#[test]
fn step_metrics_from_usage() {
    let usage = Usage {
        input_tokens: 1000,
        cached_input_tokens: 200,
        cache_creation_tokens: 50,
        output_tokens: 300,
    };
    let metrics = StepMetrics::from_usage(&usage);
    assert_eq!(metrics.prompt_tokens, Some(1000));
    assert_eq!(metrics.completion_tokens, Some(300));
    assert_eq!(metrics.cached_tokens, Some(200));
    assert!(metrics.extra.is_some());
}

#[test]
fn builder_implements_event_emitter() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "t-1".to_string() });
    // Use EventEmitter trait
    crate::EventEmitter::emit(&mut builder, &event);
    assert_eq!(builder.step_count(), 0); // ThreadStarted doesn't create a step
}

#[test]
fn skips_lifecycle_events() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    builder.process_event(&ThreadEvent::TurnStarted(TurnStartedEvent::default()));
    assert_eq!(builder.step_count(), 0);
}

#[test]
fn compact_boundary_with_segment_metadata_preserves_atif_export() {
    let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
    let event = ThreadEvent::ThreadCompactBoundary(Box::new(ThreadCompactBoundaryEvent {
        thread_id: "thread-1".to_string(),
        trigger: CompactionTrigger::Auto,
        mode: CompactionMode::Local,
        original_message_count: 12,
        compacted_message_count: 5,
        history_artifact_path: None,
        previous_segment_id: Some("segment-0001".to_string()),
        new_segment_id: Some("segment-0002".to_string()),
        previous_prefix_hash: Some("prefix-before".to_string()),
        new_prefix_hash: Some("prefix-after".to_string()),
        previous_catalog_hash: Some("catalog-before".to_string()),
        new_catalog_hash: Some("catalog-after".to_string()),
    }));

    builder.process_event_at(&event, fixed_ts());
    let trajectory = builder.finish(None);

    assert_eq!(trajectory.steps.len(), 1);
    assert_eq!(trajectory.steps[0].source, StepSource::System);
    assert_eq!(trajectory.steps[0].message.as_deref(), Some("context_compaction: 12 messages -> 5 messages (auto)"));
}
