use super::*;
use hashbrown::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::persistent_memory::MemoryCleanupStatus;
use vtcode_core::{EditorContextSnapshot, EditorFileContext};

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
fn structured_resume_lines_preserve_tool_context() {
    let mut assistant = uni::Message::assistant("cargo fmt completed successfully.".to_string());
    assistant.reasoning = Some("Need to run formatter before checks.".to_string());
    assistant.tool_calls = Some(vec![uni::ToolCall::function(
        "call_123".to_string(),
        "unified_exec".to_string(),
        "{\"cmd\":\"cargo fmt\"}".to_string(),
    )]);

    let mut tool_response =
        uni::Message::tool_response("call_123".to_string(), "{\"exit_code\":0}".to_string());
    tool_response.origin_tool = Some("unified_exec".to_string());

    let history = vec![
        uni::Message::user("run cargo fmt".to_string()),
        assistant,
        tool_response,
    ];

    let lines = build_structured_resume_lines(&history, true);

    assert!(
        lines.iter().any(|line| {
            line.style == MessageStyle::User && line.text.contains("run cargo fmt")
        })
    );
    assert!(!lines.iter().any(|line| line.text == "You:"));
    assert!(!lines.iter().any(|line| line.text == "Assistant:"));
    assert!(lines.iter().any(|line| {
        line.style == MessageStyle::Tool
            && line
                .text
                .contains("Tool unified_exec [tool_call_id: call_123]:")
    }));
    assert!(lines.iter().any(|line| {
        line.style == MessageStyle::ToolDetail && line.text.starts_with("```json")
    }));
    assert!(lines.iter().any(|line| {
        line.style == MessageStyle::ToolOutput && line.text.contains("\"exit_code\":0")
    }));
}

#[test]
fn legacy_style_inference_maps_common_prefixes() {
    assert_eq!(infer_legacy_line_style("  [1] You:"), MessageStyle::User);
    assert_eq!(
        infer_legacy_line_style("  [5] Assistant:"),
        MessageStyle::Response
    );
    assert_eq!(
        infer_legacy_line_style("System: startup"),
        MessageStyle::Info
    );
    assert_eq!(
        infer_legacy_line_style("Tool [tool_call_id: call_1]:"),
        MessageStyle::ToolOutput
    );
}

#[test]
fn structured_resume_lines_fallback_to_reasoning_details() {
    let assistant = uni::Message::assistant("done".to_string()).with_reasoning_details(Some(vec![
        serde_json::json!(r#"{"type":"reasoning.text","text":"detail trace"}"#),
    ]));
    let lines = build_structured_resume_lines(&[assistant], true);
    assert!(lines.iter().any(|line| {
        line.style == MessageStyle::Reasoning && line.text.contains("detail trace")
    }));
}

#[test]
fn structured_resume_lines_hide_reasoning_when_unsupported() {
    let mut assistant = uni::Message::assistant("done".to_string());
    assistant.reasoning = Some("trace".to_string());
    let lines = build_structured_resume_lines(&[assistant], false);
    assert!(
        !lines
            .iter()
            .any(|line| line.style == MessageStyle::Reasoning)
    );
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

    assert_eq!(
        header_context
            .persistent_memory
            .as_ref()
            .map(|badge| badge.text.as_str()),
        Some("Memory: On")
    );
    assert!(
        header_context
            .highlights
            .iter()
            .any(|highlight| highlight.title == "Memory")
    );
}

#[test]
fn background_local_agent_visibility_hides_stopped_entries() {
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

    assert!(visible_background_local_agents(vec![entry]).is_empty());
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

    assert_eq!(
        delegated_local_agent_preview_placeholder(&entry),
        "subagent failed"
    );
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

    assert_eq!(
        background_local_agent_preview_placeholder(&entry),
        "Waiting for the subprocess to emit output..."
    );
}

#[test]
fn ide_context_status_label_respects_session_override() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let mut context_manager = context_manager::ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    context_manager.set_workspace_root(workspace.path());

    let snapshot = EditorContextSnapshot {
        workspace_root: Some(PathBuf::from(workspace.path())),
        active_file: Some(EditorFileContext {
            path: workspace.path().join("src/main.rs").display().to_string(),
            language_id: Some("rust".to_string()),
            line_range: None,
            dirty: false,
            truncated: false,
            selection: None,
        }),
        ..EditorContextSnapshot::default()
    };
    context_manager.set_editor_context_snapshot(
        Some(snapshot.clone()),
        Some(&vtcode_config::IdeContextConfig::default()),
    );

    assert_eq!(
        ide_context_status_label(
            &context_manager,
            workspace.path(),
            None,
            Some(&snapshot),
            None
        )
        .as_deref(),
        Some("IDE Context (IDE): src/main.rs")
    );

    assert!(!context_manager.toggle_session_ide_context());
    assert_eq!(
        ide_context_status_label(
            &context_manager,
            workspace.path(),
            None,
            Some(&snapshot),
            None
        ),
        None
    );
}
