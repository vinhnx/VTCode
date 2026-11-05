#[test]
fn diagnostic_recorder_preserves_payload_redacts_and_deduplicates_transitions() {
    use super::*;
    use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};
    let mut recorder = ExecEventRecorder::new("thread-diagnostic", None, None);
    recorder.task_started("Inspect evidence");
    let diagnostic = Diagnostic {
        severity: DiagnosticSeverity::Warning,
        code: "provider.retrying".into(),
        summary: "Retry needs attention".into(),
        agent_details: "password=recorder-private-secret".into(),
        correlation_id: "request-7".into(),
        actions: Vec::new(),
    };
    recorder.diagnostic(&diagnostic);
    recorder.diagnostic(&diagnostic);
    let mut updated = diagnostic.clone();
    updated.agent_details = "attempt=3".into();
    recorder.diagnostic(&updated);
    let diagnostics: Vec<_> = recorder
        .take_events()
        .into_iter()
        .filter_map(|event| {
            let ThreadEvent::ItemUpdated(event) = event else {
                return None;
            };
            let ThreadItemDetails::Reasoning(reasoning) = event.item.details else {
                return None;
            };
            reasoning.diagnostic
        })
        .collect();
    assert_eq!(diagnostics.len(), 2);
    assert!(!diagnostics[0].agent_details.contains("recorder-private-secret"));
    assert_eq!(diagnostics[1].agent_details, "attempt=3");
}
#[tokio::test]
async fn planning_checkpoint_is_durable_before_admission_and_replay_preserves_allowance() {
    use crate::exec::events::{
        PlanDeltaEvent,
        planning::{PlanningOutcome, PlanningRecoverySnapshot},
    };
    let workspace = tempfile::tempdir().unwrap();
    let snapshot = PlanningRecoverySnapshot {
        outcome: PlanningOutcome::Repairing,
        research_closed: true,
        requests_used: 2,
        best_draft: Some("retained public draft".into()),
        ..Default::default()
    };
    let sink = SessionStoreSink::open(workspace.path(), "planning-recovery").await.unwrap();
    let event = ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
        thread_id: "planning-recovery".into(),
        turn_id: "turn-1".into(),
        item_id: "planning-recovery".into(),
        delta: String::new(),
        recovery: Some(Box::new(snapshot.clone())),
    }));
    sink.persist_checkpoint(&event).await.unwrap();
    assert_eq!(sink.planning_recovery().await.unwrap(), Some(snapshot.clone()));
    sink.close().await.unwrap();
    assert!(
        sink.persist_checkpoint(&event).await.is_err(),
        "closed checkpoint queue must fail before a model request"
    );
    let reopened = SessionStoreSink::open(workspace.path(), "planning-recovery").await.unwrap();
    assert_eq!(reopened.planning_recovery().await.unwrap(), Some(snapshot));
    reopened.close().await.unwrap();
}

fn pending_verifier(recorder: &mut ExecEventRecorder, session: &str) -> String {
    let args = serde_json::json!({"cmd":"cargo check --locked"});
    let launch = recorder.tool_started("exec_command", Some(&args), Some(session));
    recorder.tool_finished(
        &launch,
        crate::exec::events::ToolCallStatus::Completed,
        None,
        "still running",
        None,
        Vec::new(),
    );
    recorder.record_exec_session_output(
        &launch.id,
        "exec_command",
        &args,
        &serde_json::json!({"session_id":session,"output":"still running"}),
    );
    launch.id
}

#[tokio::test]
async fn exec_session_verification_tracks_terminal_output_and_preserves_launch_task() {
    use vtcode_memory::explanation::ExplanationScope;
    let workspace = tempfile::tempdir().unwrap();
    let sink = SessionStoreSink::open(workspace.path(), "exec-verification").await.unwrap();
    let mut recorder = ExecEventRecorder::new("exec-verification", Some(sink.event_sink()), None);
    let launch_task = recorder.task_started("Verify changes");
    for session in ["successful", "failed", "pending"] {
        pending_verifier(&mut recorder, session);
    }
    let pending = sink.explanation(ExplanationScope::Task).await.unwrap();
    assert_eq!(pending.verification.len(), 3);
    assert!(
        pending
            .verification
            .iter()
            .all(|entry| entry.fact.status == "pending" && !entry.fresh)
    );
    for (requested, returned) in [("unrelated", "unrelated"), ("successful", "failed")] {
        recorder.record_exec_session_output(
            "poll",
            "write_stdin",
            &serde_json::json!({"session_id":requested,"chars":""}),
            &serde_json::json!({"session_id":returned,"exit_code":0}),
        );
    }
    assert!(
        sink.explanation(ExplanationScope::Task)
            .await
            .unwrap()
            .verification
            .iter()
            .all(|entry| entry.fact.status == "pending")
    );
    for (session, exit) in [("successful", Some(0)), ("failed", Some(7)), ("pending", None)] {
        recorder.record_exec_session_output(
            "poll",
            "write_stdin",
            &serde_json::json!({"session_id":session,"chars":""}),
            &serde_json::json!({"session_id":session,"exit_code":exit,"output":"terminal observation"}),
        );
    }
    let terminal = sink.explanation(ExplanationScope::Task).await.unwrap();
    assert_eq!(
        terminal
            .verification
            .iter()
            .map(|entry| (entry.fact.status.as_str(), entry.exit_code, entry.fresh))
            .collect::<Vec<_>>(),
        vec![
            ("passed", Some(0), true),
            ("failed or denied", Some(7), false),
            ("pending", None, false)
        ]
    );
    let new_task = recorder.task_started("A different task");
    assert_ne!(launch_task, new_task);
    recorder.record_exec_session_output(
        "poll-new-task",
        "write_stdin",
        &serde_json::json!({"session_id":"pending","chars":""}),
        &serde_json::json!({"session_id":"pending","exit_code":0}),
    );
    assert!(sink.explanation(ExplanationScope::Task).await.unwrap().verification.is_empty());
    let all = sink.explanation(ExplanationScope::Session).await.unwrap();
    assert_eq!(all.verification.len(), 3);
    assert!(
        all.verification
            .iter()
            .all(|entry| entry.fact.task_id.as_deref() == Some(launch_task.as_str()))
    );
    assert_eq!(all.verification[2].fact.status, "passed");
    sink.close().await.unwrap();
}

#[tokio::test]
async fn exec_session_verification_launched_before_a_mutation_remains_stale() {
    let workspace = tempfile::tempdir().unwrap();
    let sink = SessionStoreSink::open(workspace.path(), "exec-freshness").await.unwrap();
    let mut recorder = ExecEventRecorder::new("exec-freshness", Some(sink.event_sink()), None);
    recorder.task_started("Verify changes");
    pending_verifier(&mut recorder, "before-edit");
    recorder.record_thread_event(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: "edit".into(),
            context: None,
            details: ThreadItemDetails::FileChange(Box::new(crate::exec::events::FileChangeItem {
                changes: vec![crate::exec::events::FileUpdateChange {
                    path: "parser.rs".into(),
                    kind: crate::exec::events::PatchChangeKind::Update,
                    terminators_normalized: None,
                }],
                status: crate::exec::events::PatchApplyStatus::Completed,
                unified_diff: None,
                diff_incomplete: None,
                additions: Some(1),
                deletions: Some(0),
            })),
        },
    }));
    recorder.record_exec_session_output(
        "poll",
        "write_stdin",
        &serde_json::json!({"session_id":"before-edit","chars":""}),
        &serde_json::json!({"session_id":"before-edit","exit_code":0}),
    );
    let model = sink
        .explanation(vtcode_memory::explanation::ExplanationScope::Task)
        .await
        .unwrap();
    assert_eq!(model.verification[0].fact.status, "passed");
    assert!(!model.verification[0].fresh);
    pending_verifier(&mut recorder, "after-edit");
    recorder.record_exec_session_output(
        "poll",
        "write_stdin",
        &serde_json::json!({"session_id":"after-edit","chars":""}),
        &serde_json::json!({"session_id":"after-edit","exit_code":0}),
    );
    assert!(
        sink.explanation(vtcode_memory::explanation::ExplanationScope::Task)
            .await
            .unwrap()
            .verification[1]
            .fresh
    );
    sink.close().await.unwrap();
}

#[tokio::test]
async fn explanation_barrier_and_decision_validation_include_accepted_events() {
    let workspace = tempfile::tempdir().unwrap();
    let sink = SessionStoreSink::open(workspace.path(), "explanation-barrier").await.unwrap();
    let mut recorder = ExecEventRecorder::new("explanation-barrier", Some(sink.event_sink()), None);
    let task_id = recorder.task_started("Fix parser");
    recorder.record_thread_event(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: "parser-change".into(),
            context: None,
            details: ThreadItemDetails::Decision(Box::new(crate::exec::events::DecisionItem {
                summary: "Use existing parser".into(),
                rationale: "Preserve validated inputs".into(),
                alternatives: vec![],
                evidence_ids: vec![],
            })),
        },
    }));
    let validate = sink.decision_validator();
    validate(task_id.clone(), vec!["parser-change".into()]).await.unwrap();
    assert!(validate("other-task".into(), vec!["parser-change".into()]).await.is_err());
    assert!(validate(task_id.clone(), vec!["missing".into()]).await.is_err());
    let model = sink
        .explanation(vtcode_memory::explanation::ExplanationScope::Task)
        .await
        .unwrap();
    assert_eq!(model.task_id.as_deref(), Some(task_id.as_str()));
    assert_eq!(model.decisions.len(), 1);
    let projected_page = sink
        .explanation_page(vtcode_memory::explanation::ExplanationScope::Task, 0)
        .await
        .unwrap();
    assert_eq!(projected_page.model.revision, model.revision);
    assert_eq!(projected_page.model.decisions, model.decisions);
    let reference = model.decisions[0].fact.evidence.clone();
    let page = sink.evidence(reference, 0).await.unwrap();
    assert!(page.text.contains("Preserve validated inputs"));
    recorder.turn_completed(Usage::default());
    recorder.turn_started();
    let updated_page = sink
        .explanation_page(vtcode_memory::explanation::ExplanationScope::Task, 0)
        .await
        .unwrap();
    assert_ne!(updated_page.model.revision, projected_page.model.revision);
    assert_eq!(updated_page.model.decisions, model.decisions);
    let starts: Vec<_> = recorder
        .events
        .iter()
        .filter_map(|e| {
            if let ThreadEvent::TurnStarted(t) = e {
                t.context.as_deref()
            } else {
                None
            }
        })
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0].task_id, starts[1].task_id);
    assert_ne!(starts[0].turn_id, starts[1].turn_id);
    sink.close().await.unwrap();
    assert!(validate(task_id, vec!["parser-change".into()]).await.is_err());
}
#[test]
fn ten_thousand_events_share_cached_projections_for_bounded_pages() {
    use vtcode_memory::explanation::{ExplanationScope, page_explanation};

    let workspace = tempfile::tempdir().unwrap();
    let log = vtcode_memory::open(workspace.path(), "large-explanation", DEFAULT_MAX_EVENTS).unwrap();
    let mut context = ExecutionContextTracker::default();
    context.begin("root", "turn", "Verify retained work", crate::exec::events::InputOrigin::User);
    let mut started = ThreadEvent::TurnStarted(TurnStartedEvent::default());
    context.annotate(&mut started);
    log.append(&started).unwrap();
    for index in 0..DEFAULT_MAX_EVENTS - 1 {
        let mut event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: format!("check-{index}"),
                context: None,
                details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: format!("cargo check --locked # {}", "x".repeat(256)),
                    arguments: None,
                    aggregated_output: "discarded output".repeat(64),
                    exit_code: Some(0),
                    status: CommandExecutionStatus::Completed,
                })),
            },
        });
        context.annotate(&mut event);
        log.append(&event).unwrap();
    }
    let mut cache = Vec::new();
    let original = cached_explanation(&log, ExplanationScope::Task, &mut cache).unwrap();
    assert_eq!(original.actions.len(), DEFAULT_MAX_EVENTS - 1);
    let mut counter = LimitedByteCounter { bytes: 0, limit: EXPLANATION_CACHE_MAX_BYTES };
    serde_json::to_writer(&mut counter, original.as_ref()).unwrap();
    assert!(counter.bytes > 8 * 1024 * 1024, "fixture exceeds the old cache ceiling");
    let mut offset = 0;
    let mut seen = 0;
    loop {
        let shared = cached_explanation(&log, ExplanationScope::Task, &mut cache).unwrap();
        assert!(Arc::ptr_eq(&original, &shared), "pages reuse the same reduced snapshot");
        let page = page_explanation(&shared, offset);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        assert_eq!(page.model.revision, original.revision);
        seen += page.model.actions.len();
        match page.next_offset {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(seen, original.actions.len());
    assert_eq!(cache.len(), 1);
}

use super::*;
use crate::core::threads::{ThreadBootstrap, ThreadManager};
use std::fs;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::time::timeout;

fn make_recorder() -> ExecEventRecorder {
    ExecEventRecorder::new("thread", None, None)
}

#[test]
fn compact_boundary_reports_the_installed_segment_identity() {
    let previous = crate::core::agent::request_envelope::SessionRequestEnvelope::with_prefix_hash(
        "segment-1",
        "prompt",
        vec![crate::llm::provider::ToolDefinition::function(
            "read_file".to_string(),
            "Read a file".to_string(),
            serde_json::json!({"type": "object"}),
        )],
        11,
        22,
    );
    let next = previous.begin_segment("segment-2");
    let mut recorder = make_recorder();

    recorder.compact_boundary(
        CompactionTrigger::Auto,
        CompactionMode::Local,
        20,
        8,
        None,
        Some(&previous),
        Some(&next),
    );

    let events = recorder.into_events();
    let Some(ThreadEvent::ThreadCompactBoundary(boundary)) = events.last() else {
        panic!("expected compact boundary");
    };
    assert_eq!(boundary.previous_segment_id.as_deref(), Some(previous.segment_id()));
    assert_eq!(boundary.new_segment_id.as_deref(), Some(next.segment_id()));
    assert_eq!(boundary.previous_prefix_hash, boundary.new_prefix_hash);
    assert_eq!(boundary.previous_catalog_hash, boundary.new_catalog_hash);
    assert!(boundary.previous_catalog_hash.is_some());
}

#[tokio::test]
async fn session_store_sink_preserves_order_when_queue_is_small() {
    let workspace = TempDir::new().expect("workspace");
    let (sink, handle) = session_store_sink_with_capacity_handle(workspace.path(), "session", 1)
        .await
        .expect("session sink");
    let health = Arc::clone(&handle.state.health);
    let events = vec![
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }),
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
    ];

    for (index, event) in events.iter().enumerate() {
        if index > 0 {
            timeout(Duration::from_secs(5), async {
                while health.snapshot().persisted_events < index as u64 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("bounded queue should keep draining");
        }
        let mut callback = sink.lock();
        callback(event);
        assert_eq!(health.snapshot().accepted_events, index as u64 + 1);
    }
    drop(sink);

    timeout(Duration::from_secs(5), handle.close())
        .await
        .expect("session sink should drain before timeout")
        .expect("session sink should close successfully");

    assert_eq!(health.snapshot().accepted_events, events.len() as u64);
    let log = vtcode_memory::open(workspace.path(), "session", DEFAULT_MAX_EVENTS).expect("reopen session");
    assert_eq!(log.event_count(), events.len() as u64);
    let event_path = workspace.path().join(".vtcode/sessions/session/events.jsonl");
    let persisted = fs::read_to_string(event_path)
        .expect("read persisted events")
        .lines()
        .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).expect("decode event"))
        .map(VersionedThreadEvent::into_event)
        .collect::<Vec<_>>();
    assert_eq!(persisted, events);
    assert_eq!(health.snapshot().append_failures, 0);
    assert_eq!(health.snapshot().channel_failures, 0);
    assert!(!health.snapshot().failed);
}

#[tokio::test]
async fn session_store_sink_open_failure_is_propagated() {
    let temp_dir = TempDir::new().expect("workspace");
    let workspace_file = temp_dir.path().join("workspace-file");
    fs::write(&workspace_file, "not a directory").expect("workspace file");

    let result = SessionStoreSink::open(&workspace_file, "session").await;
    assert!(result.is_err(), "canonical store setup must fail closed");
}

#[tokio::test]
async fn session_store_sink_drain_failure_is_propagated() {
    let health = Arc::new(SessionStoreSinkHealth::default());
    health.failed.store(true, Ordering::Release);
    let state = Arc::new(SessionStoreSinkState {
        sender: Mutex::new(None),
        reserved_bytes: AtomicU64::new(0),
        max_bytes: SESSION_STORE_DRAIN_MAX_BYTES,
        health,
    });
    let drain = tokio::spawn(async {});
    let handle = SessionStoreSinkHandle { state, drain: Some(drain) };

    let result = handle.close().await;
    assert!(result.is_err(), "canonical drain failures must fail closed");
}

#[tokio::test]
async fn session_store_sink_concurrent_close_waits_for_shared_completion() {
    let health = Arc::new(SessionStoreSinkHealth::default());
    let state = Arc::new(SessionStoreSinkState {
        sender: Mutex::new(None),
        reserved_bytes: AtomicU64::new(0),
        max_bytes: SESSION_STORE_DRAIN_MAX_BYTES,
        health,
    });
    let (started_sender, started_receiver) = oneshot::channel();
    let (release_sender, release_receiver) = oneshot::channel();
    let drain = tokio::spawn(async move {
        started_sender.send(()).expect("notify drain start");
        release_receiver.await.expect("release drain");
    });
    let handle = SessionStoreSinkHandle { state: Arc::clone(&state), drain: Some(drain) };
    let sink = Arc::new(SessionStoreSink {
        state,
        handle: Arc::new(Mutex::new(Some(handle))),
        close_result: Arc::new(OnceCell::new()),
    });

    let first = {
        let sink = Arc::clone(&sink);
        tokio::spawn(async move { sink.close().await })
    };
    started_receiver.await.expect("drain should start");
    let second = {
        let sink = Arc::clone(&sink);
        tokio::spawn(async move { sink.close().await })
    };
    tokio::task::yield_now().await;
    assert!(!second.is_finished(), "concurrent close must wait for the first drain");

    release_sender.send(()).expect("release drain");
    assert!(first.await.expect("first close task").is_ok());
    assert!(second.await.expect("second close task").is_ok());
}

#[tokio::test]
async fn session_store_sink_rejects_events_over_byte_budget() {
    let workspace = TempDir::new().expect("workspace");
    let (state, handle) = open_session_store_sink_with_limits(workspace.path(), "session", 4, 128)
        .await
        .expect("session sink");
    let health = Arc::clone(&state.health);
    let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "x".repeat(256) });

    let result = enqueue_session_event(&state, &event);
    assert!(result.is_err(), "oversized events must fail closed");
    handle.close().await.expect_err("failed sink must not close successfully");

    let snapshot = health.snapshot();
    assert_eq!(snapshot.accepted_events, 0);
    assert_eq!(snapshot.serialization_failures, 1);
    assert!(snapshot.failed);
}

#[test]
fn session_store_sink_saturation_fails_closed() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let state = SessionStoreSinkState {
        sender: Mutex::new(Some(sender)),
        reserved_bytes: AtomicU64::new(0),
        max_bytes: SESSION_STORE_DRAIN_MAX_BYTES,
        health: Arc::new(SessionStoreSinkHealth::default()),
    };
    let first = ThreadEvent::TurnStarted(TurnStartedEvent::default());
    let second = ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    });

    enqueue_session_event(&state, &first).expect("first event should fit");
    assert!(enqueue_session_event(&state, &second).is_err(), "queue saturation must fail closed");

    let snapshot = state.health.snapshot();
    assert_eq!(snapshot.accepted_events, 1);
    assert_eq!(snapshot.channel_failures, 1);
    assert!(snapshot.failed);
    drop(receiver);
}

#[test]
fn closed_session_store_channel_is_observable() {
    let (sender, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    let health = SessionStoreSinkHealth::default();
    let event = ThreadEvent::TurnStarted(TurnStartedEvent::default());

    if sender.send(event).is_err() {
        health.channel_failures.fetch_add(1, Ordering::Relaxed);
    }

    assert_eq!(health.snapshot().channel_failures, 1);
}

#[test]
fn streaming_events_flush_on_completion() {
    let mut recorder = make_recorder();
    recorder.turn_started();
    assert!(recorder.agent_message_stream_update("partial"));
    recorder.agent_message_stream_complete();
    let events = recorder.into_events();
    assert!(events.iter().any(|event| matches!(event, ThreadEvent::ItemCompleted(_))));
}

#[test]
fn thread_failure_emits_terminal_lifecycle_events() {
    let mut recorder = make_recorder();
    recorder.turn_started();
    recorder.thread_failed("session", "setup failed", 1);
    let events = recorder.into_events();

    assert!(events.iter().any(|event| matches!(event, ThreadEvent::TurnFailed(_))));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            ThreadEvent::ThreadCompleted(item)
                if item.subtype == ThreadCompletionSubtype::ErrorDuringExecution
                    && item.outcome_code == "error"
        )
    }));
}

#[test]
fn command_events_capture_status() {
    let mut recorder = make_recorder();
    let handle = recorder.command_started("git status");
    recorder.command_finished(&handle, CommandExecutionStatus::Completed, Some(0), "");
    let events = recorder.into_events();
    let command = events
        .into_iter()
        .filter_map(|event| match event {
            ThreadEvent::ItemCompleted(event) => Some(event.item),
            _ => None,
        })
        .find(|item| matches!(item.details, ThreadItemDetails::CommandExecution(_)))
        .expect("command event should be emitted");

    match command.details {
        ThreadItemDetails::CommandExecution(details) => {
            assert_eq!(details.command, "git status");
            assert_eq!(details.status, CommandExecutionStatus::Completed);
        }
        _ => panic!("unexpected event variant"),
    }
}

#[test]
fn rejected_tool_call_emits_failed_tool_output_item() {
    let mut recorder = make_recorder();
    recorder.tool_rejected("read_file", None, Some("call_1"), "Tool permission denied");

    let events = recorder.into_events();
    let tool_outputs = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                ThreadItemDetails::ToolOutput(details) => Some(details),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(tool_outputs.len(), 1);
    assert_eq!(tool_outputs[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(tool_outputs[0].status, crate::exec::events::ToolCallStatus::Failed);
    assert_eq!(tool_outputs[0].output, "Tool permission denied");
}

#[test]
fn thread_backed_recorder_reuses_submission_id_within_turn() {
    let handle = ThreadManager::new().start_thread_with_identifier("thread", ThreadBootstrap::new(None));
    let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

    recorder.turn_started();
    recorder.agent_message("hello");
    recorder.turn_completed(Usage::default());

    let records = handle.replay_recent();
    let submission_ids: std::collections::BTreeSet<String> = records
        .iter()
        .filter_map(|record| record.submission_id.as_ref().map(|id| id.as_str().to_string()))
        .collect();

    assert_eq!(submission_ids.len(), 1);
    assert!(
        records
            .iter()
            .any(|record| matches!(record.event, ThreadEvent::TurnStarted(_)) && record.submission_id.is_some())
    );
    assert!(
        records
            .iter()
            .any(|record| matches!(record.event, ThreadEvent::TurnCompleted(_)) && record.submission_id.is_some())
    );
}

#[test]
fn thread_backed_recorder_keeps_full_event_history_beyond_thread_buffer() {
    let handle =
        ThreadManager::with_event_buffer_capacity(2).start_thread_with_identifier("thread", ThreadBootstrap::new(None));
    let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

    recorder.turn_started();
    recorder.agent_message("first");
    recorder.agent_message("second");
    recorder.turn_completed(Usage::default());

    let full_events = recorder.into_events();
    let buffered_events = handle.recent_events();

    assert_eq!(buffered_events.len(), 2);
    assert!(full_events.len() > buffered_events.len());
    assert_eq!(
        full_events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::ItemCompleted(_)))
            .count(),
        2
    );
}

#[test]
fn tool_output_finished_forwards_app_previews_to_recorded_item() {
    use super::*;
    use crate::exec::events::{AppUiPosture, ToolCallStatus};

    let mut recorder = ExecEventRecorder::new("thread-app-preview", None, None);
    recorder.tool_output_finished(
        "call_1",
        Some("tool_call_0"),
        ToolCallStatus::Completed,
        Some(0),
        "done",
        None,
        vec![AppPreviewItem {
            uri: "ui://demo/app.html".to_string(),
            mime_type: Some("text/html;profile=mcp-app".to_string()),
            bytes: 4312,
            blocks: 1,
            ui: Some(AppUiPosture {
                connect_domains: vec!["https://api.example.com".to_string()],
                ..AppUiPosture::default()
            }),
        }],
    );
    // Failure-path completions carry no previews; the field must stay empty, not absent.
    recorder.tool_output_finished("call_2", None, ToolCallStatus::Failed, None, "boom", None, Vec::new());

    let outputs: Vec<_> = recorder
        .take_events()
        .into_iter()
        .filter_map(|event| {
            let ThreadEvent::ItemCompleted(completed) = event else {
                return None;
            };
            let ThreadItemDetails::ToolOutput(output) = completed.item.details else {
                return None;
            };
            Some(output)
        })
        .collect();

    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].app_previews.len(), 1);
    assert_eq!(outputs[0].app_previews[0].uri, "ui://demo/app.html");
    assert_eq!(outputs[0].app_previews[0].bytes, 4312);
    assert!(outputs[1].app_previews.is_empty());
}
