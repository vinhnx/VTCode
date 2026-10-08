use super::super::{EditorOpenDispatcher, EditorOpenRequest};
use super::*;
use crate::agent::runloop::unified::session_setup::shell::{SharedExecSessions, build_session_event_callback};
use crate::agent::runloop::unified::state;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::Notify;
use vtcode_core::llm::provider as uni;
use vtcode_core::persistent_memory::{MemoryCleanupStatus, PersistentMemoryStatus};
use vtcode_ui::tui::app::InlineEvent;

fn test_exec_sessions() -> SharedExecSessions {
    Arc::new(std::sync::OnceLock::new())
}

fn sample_memory_status() -> PersistentMemoryStatus {
    PersistentMemoryStatus {
        enabled: true,
        auto_write: true,
        directory: PathBuf::from("/tmp/memory"),
        summary_file: PathBuf::from("/tmp/memory/memory_summary.md"),
        memory_file: PathBuf::from("/tmp/memory/MEMORY.md"),
        preferences_file: PathBuf::from("/tmp/memory/preferences.md"),
        repository_facts_file: PathBuf::from("/tmp/memory/repository-facts.md"),
        notes_dir: PathBuf::from("/tmp/memory/notes"),
        rollout_summaries_dir: PathBuf::from("/tmp/memory/rollout_summaries"),
        summary_exists: true,
        registry_exists: true,
        pending_rollout_summaries: 0,
        cleanup_status: MemoryCleanupStatus {
            needed: false,
            suspicious_facts: 0,
            suspicious_summary_lines: 0,
        },
    }
}

#[test]
fn session_tui_interrupt_callback_only_cancels_after_cancel_is_handled() {
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (settings_events, _settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state.clone(),
        notify,
        None,
        settings_events,
        Arc::new(EditorOpenDispatcher::new(true)),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    callback(&InlineEvent::Interrupt);
    state.mark_cancel_handled();
    thread::sleep(Duration::from_millis(250));
    callback(&InlineEvent::Interrupt);

    assert!(state.is_cancel_requested());
    assert!(!state.is_exit_requested());
}

#[tokio::test]
async fn session_tui_exit_callback_wakes_waiters_and_survives_a_fresh_submission() {
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (settings_events, _) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state.clone(),
        notify.clone(),
        None,
        settings_events,
        Arc::new(EditorOpenDispatcher::new(true)),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );
    callback(&InlineEvent::Interrupt);
    state.mark_cancel_handled();
    callback(&InlineEvent::Exit);
    callback(&InlineEvent::Submit("a fresh request".into()));
    state.reset();
    assert!(state.is_exit_requested());
    tokio::time::timeout(Duration::from_millis(100), notify.notified())
        .await
        .expect("stored stop notification");
}

#[test]
fn structured_resume_lines_preserve_tool_context() {
    let mut assistant = uni::Message::assistant("cargo fmt completed successfully.".to_string());
    assistant.reasoning = Some("Need to run formatter before checks.".to_string());
    assistant.tool_calls = Some(vec![uni::ToolCall::function(
        "call_123".to_string(),
        "exec_command".to_string(),
        "{\"cmd\":\"cargo fmt\"}".to_string(),
    )]);

    let tool_output =
        r#"{"output":" Finished dev profile [unoptimized + debuginfo]\n","exit_code":0,"backend":"pipe"}"#;
    let mut tool_response = uni::Message::tool_response("call_123".to_string(), tool_output.to_string());
    tool_response.origin_tool = Some("exec_command".to_string());

    let history = vec![
        uni::Message::user("run cargo fmt".to_string()),
        assistant,
        tool_response,
    ];

    let lines = build_structured_resume_lines(&history, true);

    assert!(
        lines
            .iter()
            .any(|line| { line.style == MessageStyle::User && line.text.contains("run cargo fmt") })
    );
    assert!(!lines.iter().any(|line| line.text == "You:"));
    assert!(!lines.iter().any(|line| line.text == "Assistant:"));
    assert!(lines.iter().any(|line| {
        line.style == MessageStyle::Tool && line.text.contains("Tool exec_command [tool_call_id: call_123]:")
    }));
    // Tool arguments should show a concise summary instead of raw JSON
    assert!(
        lines
            .iter()
            .any(|line| { line.style == MessageStyle::ToolDetail && line.text.contains("command: cargo fmt") })
    );
    // Tool output should show extracted output text, not raw JSON
    assert!(
        lines
            .iter()
            .any(|line| { line.style == MessageStyle::ToolOutput && line.text.contains("Finished dev profile") })
    );
}

#[test]
fn legacy_style_inference_maps_common_prefixes() {
    assert_eq!(infer_legacy_line_style("  [1] You:"), MessageStyle::User);
    assert_eq!(infer_legacy_line_style("  [5] Assistant:"), MessageStyle::Response);
    assert_eq!(infer_legacy_line_style("System: startup"), MessageStyle::Info);
    assert_eq!(infer_legacy_line_style("Tool [tool_call_id: call_1]:"), MessageStyle::ToolOutput);
}

#[test]
fn structured_resume_lines_fallback_to_reasoning_details() {
    let assistant = uni::Message::assistant("done".to_string())
        .with_reasoning_details(Some(vec![serde_json::json!(r#"{"type":"reasoning.text","text":"detail trace"}"#)]));
    let lines = build_structured_resume_lines(&[assistant], true);
    assert!(
        lines
            .iter()
            .any(|line| { line.style == MessageStyle::Reasoning && line.text.contains("detail trace") })
    );
}

#[test]
fn structured_resume_lines_omit_persisted_request_context() {
    let few_shot = format!("{}\n### patch-edit\nexample body", vtcode_core::prompts::FEW_SHOT_SECTION_HEADER);
    let editor = "## Active Editor Context\n- Active file: src/parser.rs".to_string();
    let history = vec![
        uni::Message::system(editor),
        uni::Message::user("edit the parser".to_string()),
        uni::Message::turn_scoped_system(few_shot),
        uni::Message::assistant("done".to_string()),
    ];
    let lines = build_structured_resume_lines(&history, true);
    assert!(!lines.iter().any(|line| line.text.contains("example body")
        || line.text.contains("src/parser.rs")
        || line.text == "System:"));
    assert!(lines.iter().any(|line| line.text.contains("edit the parser")));
    assert!(lines.iter().any(|line| line.text.contains("done")));
}

#[test]
fn structured_resume_lines_hide_reasoning_when_unsupported() {
    let mut assistant = uni::Message::assistant("done".to_string());
    assistant.reasoning = Some("trace".to_string());
    let lines = build_structured_resume_lines(&[assistant], false);
    assert!(!lines.iter().any(|line| line.style == MessageStyle::Reasoning));
}

#[test]
fn persistent_memory_guide_lines_show_standard_actions() {
    let lines = persistent_memory_guide_lines(&sample_memory_status());
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("Memory is enabled"));
    assert!(lines[1].contains("remember"));
    assert!(lines[2].contains("Auto-write is on"));
}

#[test]
fn persistent_memory_guide_lines_call_out_cleanup_when_needed() {
    let mut status = sample_memory_status();
    status.auto_write = false;
    status.cleanup_status.needed = true;

    let lines = persistent_memory_guide_lines(&status);
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("one-time cleanup"));
    assert!(lines[2].contains("Auto-write is off"));
}

#[test]
fn persistent_memory_header_badge_reflects_memory_mode() {
    let badge = persistent_memory_header_badge(&sample_memory_status());
    assert_eq!(badge.text, "Memory: On");
    assert_eq!(badge.tone, InlineHeaderStatusTone::Ready);
}

#[test]
fn persistent_memory_header_badge_warns_on_cleanup() {
    let mut status = sample_memory_status();
    status.cleanup_status.needed = true;

    let badge = persistent_memory_header_badge(&status);
    assert_eq!(badge.text, "Memory: Needs cleanup");
    assert_eq!(badge.tone, InlineHeaderStatusTone::Warning);
}

#[test]
fn apply_persistent_memory_header_guide_sets_badge_and_highlight() {
    let mut header_context = InlineHeaderContext::default();

    apply_persistent_memory_header_guide(&mut header_context, &sample_memory_status());

    assert_eq!(header_context.persistent_memory.as_ref().map(|badge| badge.text.as_str()), Some("Memory: On"));
    assert!(header_context.highlights.iter().any(|highlight| highlight.title == "Memory"));
}

#[test]
fn background_local_agent_visibility_keeps_stopped_entries() {
    let entry = vtcode_core::subagents::BackgroundSubprocessEntry {
        id: "background-default".to_string(),
        session_id: "session-456".to_string(),
        exec_session_id: String::new(),
        agent_name: "default".to_string(),
        display_label: "default".to_string(),
        description: "Default agent".to_string(),
        source: "builtin".to_string(),
        color: None,
        status: vtcode_core::subagents::BackgroundSubprocessStatus::Stopped,
        desired_enabled: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        started_at: None,
        ended_at: None,
        pid: None,
        summary: None,
        error: None,
        archive_path: None,
        transcript_path: None,
    };

    let visible = visible_background_local_agents(vec![entry]);
    assert_eq!(visible.len(), 1, "finished background rows stay visible for history");
}

#[test]
fn delegated_local_agent_visibility_keeps_completed_and_hides_closed() {
    let base = SubagentStatusEntry {
        id: "thread-1".to_string(),
        session_id: "session-123".to_string(),
        parent_thread_id: "main".to_string(),
        agent_name: "rust-engineer".to_string(),
        display_label: "rust-engineer".to_string(),
        description: "Review Rust changes".to_string(),
        source: "project".to_string(),
        color: None,
        status: vtcode_core::subagents::SubagentStatus::Completed,
        background: false,
        depth: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
        summary: Some("done".to_string()),
        error: None,
        transcript_path: None,
        nickname: None,
    };
    let mut closed = base.clone();
    closed.id = "thread-2".to_string();
    closed.status = vtcode_core::subagents::SubagentStatus::Closed;

    let visible = visible_delegated_local_agents(vec![base, closed]);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].status, vtcode_core::subagents::SubagentStatus::Completed);
}

#[test]
fn delegated_local_agent_preview_uses_queue_placeholder() {
    let entry = SubagentStatusEntry {
        id: "thread-1".to_string(),
        session_id: "session-123".to_string(),
        parent_thread_id: "main".to_string(),
        agent_name: "rust-engineer".to_string(),
        display_label: "rust-engineer".to_string(),
        description: "Review Rust changes".to_string(),
        source: "project".to_string(),
        color: None,
        status: vtcode_core::subagents::SubagentStatus::Queued,
        background: false,
        depth: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: None,
        summary: None,
        error: None,
        transcript_path: None,
        nickname: None,
    };

    assert_eq!(
        delegated_local_agent_preview_placeholder(&entry),
        "Agent is queued and has not emitted transcript output yet."
    );
}

#[test]
fn delegated_local_agent_visibility_keeps_failed_entries() {
    let entry = SubagentStatusEntry {
        id: "thread-1".to_string(),
        session_id: "session-123".to_string(),
        parent_thread_id: "main".to_string(),
        agent_name: "rust-engineer".to_string(),
        display_label: "rust-engineer".to_string(),
        description: "Review Rust changes".to_string(),
        source: "project".to_string(),
        color: None,
        status: vtcode_core::subagents::SubagentStatus::Failed,
        background: false,
        depth: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
        summary: None,
        error: Some("subagent failed".to_string()),
        transcript_path: None,
        nickname: None,
    };

    let visible = visible_delegated_local_agents(vec![entry]);
    assert_eq!(visible.len(), 1);
}

#[test]
fn delegated_local_agent_preview_uses_failure_message() {
    let entry = SubagentStatusEntry {
        id: "thread-1".to_string(),
        session_id: "session-123".to_string(),
        parent_thread_id: "main".to_string(),
        agent_name: "rust-engineer".to_string(),
        display_label: "rust-engineer".to_string(),
        description: "Review Rust changes".to_string(),
        source: "project".to_string(),
        color: None,
        status: vtcode_core::subagents::SubagentStatus::Failed,
        background: false,
        depth: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
        summary: None,
        error: Some("subagent failed".to_string()),
        transcript_path: None,
        nickname: None,
    };

    assert_eq!(delegated_local_agent_preview_placeholder(&entry), "subagent failed");
}

#[test]
fn background_local_agent_preview_uses_status_placeholder() {
    let entry = vtcode_core::subagents::BackgroundSubprocessEntry {
        id: "background-default".to_string(),
        session_id: "session-456".to_string(),
        exec_session_id: String::new(),
        agent_name: "default".to_string(),
        display_label: "default".to_string(),
        description: "Default agent".to_string(),
        source: "builtin".to_string(),
        color: None,
        status: vtcode_core::subagents::BackgroundSubprocessStatus::Starting,
        desired_enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        started_at: None,
        ended_at: None,
        pid: None,
        summary: None,
        error: None,
        archive_path: None,
        transcript_path: None,
    };

    assert_eq!(background_local_agent_preview_placeholder(&entry), "Waiting for the subprocess to emit output...");
}

#[test]
fn file_open_callback_forwards_out_of_band_without_idle_drain() {
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (sender, mut receiver) = super::super::bounded_editor_open_requests();
    let dispatcher = Arc::new(EditorOpenDispatcher::new(true));
    dispatcher.set_sender(sender);
    let (settings_events, _settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state,
        notify,
        None,
        settings_events,
        dispatcher,
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    callback(&InlineEvent::OpenFileInEditor("/tmp/demo.rs".to_string()));

    let request = receiver.try_recv().expect("callback should forward file-open immediately");
    assert_eq!(
        request,
        EditorOpenRequest::from_raw_target("/tmp/demo.rs", &PathBuf::from("/tmp")).expect("valid editor target")
    );
}

#[test]
fn file_open_callback_without_sender_is_noop() {
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (settings_events, _settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state,
        notify,
        None,
        settings_events,
        Arc::new(EditorOpenDispatcher::new(true)),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    callback(&InlineEvent::OpenFileInEditor("/tmp/demo.rs".to_string()));
}

#[test]
fn file_open_callback_defers_terminal_editors_to_idle_drain() {
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (sender, mut receiver) = super::super::bounded_editor_open_requests();
    let dispatcher = Arc::new(EditorOpenDispatcher::new(false));
    dispatcher.set_sender(sender.clone());
    let (settings_events, _settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state,
        notify,
        None,
        settings_events,
        dispatcher.clone(),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    callback(&InlineEvent::OpenFileInEditor("/tmp/demo.rs".to_string()));

    // Terminal editors must not open mid-turn (TUI suspension would contend
    // with the running turn); the deferred drain delivers after the turn.
    assert!(receiver.try_recv().is_err());
    dispatcher.try_forward_deferred(&sender, "/tmp/demo.rs", &PathBuf::from("/tmp"));
    assert!(receiver.try_recv().is_ok());
}

#[test]
fn busy_model_steer_routes_to_settings_events() {
    use vtcode_core::core::agent::steering::SteeringMessage;
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (steering_tx, mut steering_rx) = tokio::sync::mpsc::unbounded_channel::<SteeringMessage>();
    let (settings_events, mut settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state,
        notify,
        Some(steering_tx),
        settings_events,
        Arc::new(EditorOpenDispatcher::new(true)),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    for text in ["/model", "/model foo"] {
        callback(&InlineEvent::Steer(text.into()));
        let forwarded = settings_rx.try_recv().expect("model steer must reach settings task");
        assert!(matches!(forwarded, InlineEvent::Steer(_)), "expected Steer for {text}");
        assert!(steering_rx.try_recv().is_err(), "{text} must not become follow-up steering");
    }
}

#[test]
fn ordinary_steer_still_routes_to_steering_channel() {
    use vtcode_core::core::agent::steering::SteeringMessage;
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (steering_tx, mut steering_rx) = tokio::sync::mpsc::unbounded_channel::<SteeringMessage>();
    let (settings_events, mut settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state.clone(),
        notify,
        Some(steering_tx),
        settings_events,
        Arc::new(EditorOpenDispatcher::new(true)),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    callback(&InlineEvent::Steer("keep going".into()));
    assert!(matches!(steering_rx.try_recv(), Ok(SteeringMessage::FollowUpInput(_))));
    assert!(settings_rx.try_recv().is_err());
    assert!(
        state.take_steer_delivered(),
        "successful steering delivery must latch so the runloop does not queue twice"
    );
}

#[test]
fn steer_without_steering_sender_leaves_delivery_latch_clear() {
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (settings_events, _settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state.clone(),
        notify,
        None,
        settings_events,
        Arc::new(EditorOpenDispatcher::new(true)),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    callback(&InlineEvent::Steer("queue me".into()));
    assert!(
        !state.take_steer_delivered(),
        "undelivered steer must leave the latch clear so the runloop queues it"
    );
}

#[test]
fn transient_overlay_events_reach_settings_task_for_picker() {
    let state = Arc::new(state::CtrlCState::new());
    let notify = Arc::new(Notify::new());
    let (settings_events, mut settings_rx) = tokio::sync::mpsc::unbounded_channel();
    let callback = build_session_event_callback(
        state,
        notify,
        None,
        settings_events,
        Arc::new(EditorOpenDispatcher::new(true)),
        PathBuf::from("/tmp"),
        test_exec_sessions(),
    );

    callback(&InlineEvent::Transient(vtcode_ui::tui::app::TransientEvent::Cancelled));
    assert!(matches!(
        settings_rx.try_recv(),
        Ok(InlineEvent::Transient(vtcode_ui::tui::app::TransientEvent::Cancelled))
    ));
}
