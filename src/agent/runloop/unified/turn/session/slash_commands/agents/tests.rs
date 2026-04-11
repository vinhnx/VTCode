use super::{
    active_subagent_entries, background_subprocess_summary, subprocess_action_prompt,
    summarize_thread_event_preview, visible_subagent_entries,
};
use chrono::Utc;
use std::path::PathBuf;
use vtcode_core::subagents::{
    BackgroundSubprocessEntry, BackgroundSubprocessStatus, SubagentStatus, SubagentStatusEntry,
};
use vtcode_core::{
    ItemStartedEvent, ItemUpdatedEvent, ReasoningItem, ThreadEvent, ThreadItem, ThreadItemDetails,
    ToolCallStatus, ToolOutputItem,
};

fn test_subagent_entry(id: &str, status: SubagentStatus) -> SubagentStatusEntry {
    let now = Utc::now();
    SubagentStatusEntry {
        id: id.to_string(),
        session_id: format!("session-{id}"),
        parent_thread_id: "parent".to_string(),
        agent_name: "rust-engineer".to_string(),
        display_label: "Rust Engineer".to_string(),
        description: "Test agent".to_string(),
        source: "project".to_string(),
        color: Some("blue".to_string()),
        status,
        background: false,
        depth: 1,
        created_at: now,
        updated_at: now,
        completed_at: None,
        summary: Some("summary".to_string()),
        error: None,
        transcript_path: Some(PathBuf::from("/tmp/transcript.md")),
        nickname: None,
    }
}

#[test]
fn active_subagent_entries_filter_terminal_statuses() {
    let entries = vec![
        test_subagent_entry("queued", SubagentStatus::Queued),
        test_subagent_entry("running", SubagentStatus::Running),
        test_subagent_entry("waiting", SubagentStatus::Waiting),
        test_subagent_entry("completed", SubagentStatus::Completed),
        test_subagent_entry("failed", SubagentStatus::Failed),
        test_subagent_entry("closed", SubagentStatus::Closed),
    ];

    let active = active_subagent_entries(entries);
    let active_ids = active.into_iter().map(|entry| entry.id).collect::<Vec<_>>();

    assert_eq!(active_ids, vec!["queued", "running", "waiting"]);
}

#[test]
fn visible_subagent_entries_keep_recent_terminal_runs_inspectable() {
    let mut completed = test_subagent_entry("completed", SubagentStatus::Completed);
    completed.updated_at = Utc::now();

    let mut running = test_subagent_entry("running", SubagentStatus::Running);
    running.updated_at = completed.updated_at - chrono::Duration::seconds(1);

    let mut failed = test_subagent_entry("failed", SubagentStatus::Failed);
    failed.updated_at = running.updated_at - chrono::Duration::seconds(1);

    let mut closed = test_subagent_entry("closed", SubagentStatus::Closed);
    closed.updated_at = failed.updated_at - chrono::Duration::seconds(1);

    let visible = visible_subagent_entries(vec![completed, closed, failed, running]);
    let visible_ids = visible
        .into_iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();

    assert_eq!(visible_ids, vec!["running", "completed", "failed"]);
}

#[test]
fn subprocess_action_prompt_matches_requested_action() {
    let (graceful_title, graceful_message, graceful_confirm) =
        subprocess_action_prompt("Rust Engineer", false);
    assert_eq!(graceful_title, "Graceful stop subprocess");
    assert_eq!(
        graceful_message,
        "Request a graceful shutdown for `Rust Engineer`?"
    );
    assert_eq!(graceful_confirm, "Graceful stop");

    let (force_title, force_message, force_confirm) =
        subprocess_action_prompt("Rust Engineer", true);
    assert_eq!(force_title, "Force cancel subprocess");
    assert_eq!(force_message, "Force cancel `Rust Engineer` immediately?");
    assert_eq!(force_confirm, "Force cancel");
}

#[test]
fn summarize_thread_event_preview_uses_latest_live_updates() {
    let preview = summarize_thread_event_preview(&[
        ThreadEvent::ItemStarted(ItemStartedEvent {
            item: ThreadItem {
                id: "reasoning-1".to_string(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: "Inspecting the diff".to_string(),
                    stage: None,
                }),
            },
        }),
        ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                id: "reasoning-1".to_string(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: "Inspecting the diff carefully".to_string(),
                    stage: None,
                }),
            },
        }),
        ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                id: "tool-output-1".to_string(),
                details: ThreadItemDetails::ToolOutput(ToolOutputItem {
                    call_id: "call-1".to_string(),
                    tool_call_id: None,
                    spool_path: None,
                    output: "line 1\nFinished `cargo check`".to_string(),
                    exit_code: Some(0),
                    status: ToolCallStatus::Completed,
                }),
            },
        }),
    ]);

    assert!(preview.contains("thinking: Inspecting the diff carefully"));
    assert!(preview.contains("tool output: Finished `cargo check`"));
    assert!(!preview.contains("thinking: Inspecting the diff\n"));
}

#[test]
fn background_subprocess_summary_reports_waiting_state_without_summary() {
    let entry = BackgroundSubprocessEntry {
        id: "background-rust-engineer".to_string(),
        session_id: "session-123".to_string(),
        exec_session_id: "exec-session-123".to_string(),
        agent_name: "rust-engineer".to_string(),
        display_label: "rust-engineer".to_string(),
        description: "Review Rust changes".to_string(),
        source: "project".to_string(),
        color: None,
        status: BackgroundSubprocessStatus::Starting,
        desired_enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        started_at: None,
        ended_at: None,
        pid: None,
        summary: None,
        error: None,
        archive_path: None,
        transcript_path: None,
    };

    assert_eq!(
        background_subprocess_summary(&entry),
        "Starting; waiting for subprocess output."
    );
}
