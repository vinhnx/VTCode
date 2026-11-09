use anyhow::Result;
use std::sync::Arc;
use vtcode_commons::program_status::ProgramState;
use vtcode_core::subagents::{
    BackgroundSubprocessEntry, BackgroundSubprocessSnapshot, BackgroundSubprocessStatus, SubagentController,
    SubagentStatus, SubagentStatusEntry, SubagentThreadSnapshot,
};
use vtcode_core::tools::exec_session::ExecSessionManager;
use vtcode_core::tools::types::{VTCodeExecSession, VTCodeSessionLifecycleState};
use vtcode_core::{CommandExecutionStatus, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use vtcode_ui::tui::app::{InlineHandle, LocalAgentEntry, LocalAgentKind, LocalAgentStatus};

pub(crate) async fn refresh_local_agents(
    handle: &InlineHandle,
    controller: Option<&Arc<SubagentController>>,
    exec_sessions: ExecSessionManager,
) -> Result<()> {
    let (delegated_entries, background_entries, refresh_error) = if let Some(controller) = controller {
        let delegated_entries = controller.status_entries().await;
        match controller.refresh_background_processes().await {
            Ok(background_entries) => (delegated_entries, background_entries, None),
            Err(error) => (delegated_entries, controller.background_status_entries().await, Some(error)),
        }
    } else {
        (Vec::new(), Vec::new(), None)
    };
    let local_agents =
        build_local_agent_entries(controller, delegated_entries, background_entries, &exec_sessions).await;
    handle.set_local_agents(local_agents);
    refresh_error.map_or(Ok(()), Err)
}

async fn build_local_agent_entries(
    controller: Option<&Arc<SubagentController>>,
    delegated_entries: Vec<SubagentStatusEntry>,
    background_entries: Vec<BackgroundSubprocessEntry>,
    exec_sessions: &ExecSessionManager,
) -> Vec<LocalAgentEntry> {
    let mut entries = Vec::new();

    for entry in visible_delegated_local_agents(delegated_entries) {
        let snapshot = if let Some(controller) = controller {
            match controller.snapshot_for_thread(&entry.id).await {
                Ok(snapshot) => Some(snapshot),
                Err(err) => {
                    tracing::debug!(
                        subagent_id = entry.id.as_str(),
                        "Failed to snapshot delegated agent for local-agents UI: {}",
                        err
                    );
                    None
                }
            }
        } else {
            None
        };
        let preview = snapshot
            .as_ref()
            .map(|snapshot| delegated_local_agent_preview(&entry, snapshot))
            .unwrap_or_else(|| delegated_local_agent_preview_placeholder(&entry));
        let summary = snapshot
            .as_ref()
            .map(|snapshot| delegated_local_agent_summary(&entry, snapshot))
            .or_else(|| entry.summary.clone());
        entries.push((
            Some(entry.updated_at),
            LocalAgentEntry {
                id: entry.id.clone(),
                display_label: entry.display_label.clone(),
                agent_name: entry.agent_name.clone(),
                color: entry.color.clone(),
                kind: LocalAgentKind::Delegated,
                program_status: delegated_program_state(entry.status),
                updated_at: entry.updated_at.timestamp_millis(),
                status: local_agent_status_from_subagent(entry.status),
                summary,
                preview,
                transcript_path: entry.transcript_path.clone(),
            },
        ));
    }

    for entry in visible_background_local_agents(background_entries) {
        let snapshot = if let Some(controller) = controller {
            match controller.background_snapshot(&entry.id).await {
                Ok(snapshot) => Some(snapshot),
                Err(err) => {
                    tracing::debug!(
                        subprocess_id = entry.id.as_str(),
                        "Failed to snapshot background subprocess for local-agents UI: {}",
                        err
                    );
                    None
                }
            }
        } else {
            None
        };
        let preview = snapshot
            .as_ref()
            .map(background_local_agent_preview)
            .unwrap_or_else(|| background_local_agent_preview_placeholder(&entry));
        entries.push((
            Some(entry.updated_at),
            LocalAgentEntry {
                id: entry.id.clone(),
                display_label: entry.display_label.clone(),
                agent_name: entry.agent_name.clone(),
                color: entry.color.clone(),
                kind: LocalAgentKind::Background,
                program_status: background_program_state(&entry),
                updated_at: entry.ended_at.unwrap_or(entry.updated_at).timestamp_millis(),
                status: local_agent_status_from_background(entry.status),
                summary: Some(background_local_agent_summary(&entry)),
                preview,
                transcript_path: entry.transcript_path.clone().or(entry.archive_path.clone()),
            },
        ));
    }

    for snapshot in exec_sessions.background_session_snapshots().await {
        let metadata = snapshot.metadata;
        entries.push((
            metadata.started_at,
            LocalAgentEntry {
                id: metadata.id.as_str().to_string(),
                display_label: metadata.command_label(),
                agent_name: "exec-session".to_string(),
                color: None,
                kind: LocalAgentKind::ExecSession,
                program_status: command_program_state(&metadata, snapshot.termination_requested),
                updated_at: snapshot.updated_at.timestamp_millis(),
                status: local_agent_status_from_exec_session(&metadata),
                summary: Some(exec_session_summary(&metadata)),
                preview: snapshot.preview,
                transcript_path: None,
            },
        ));
    }

    entries.sort_by_key(|left| std::cmp::Reverse(left.0));
    entries.into_iter().map(|(_, entry)| entry).collect()
}

/// Decode a delegated-agent status once, at the UI boundary.
fn local_agent_status_from_subagent(status: SubagentStatus) -> LocalAgentStatus {
    match status {
        SubagentStatus::Queued => LocalAgentStatus::Queued,
        SubagentStatus::Running => LocalAgentStatus::Running,
        SubagentStatus::Waiting => LocalAgentStatus::Waiting,
        SubagentStatus::Completed => LocalAgentStatus::Completed,
        SubagentStatus::Failed => LocalAgentStatus::Failed,
        SubagentStatus::Closed => LocalAgentStatus::Closed,
    }
}

/// Decode a background-subprocess status once, at the UI boundary.
fn local_agent_status_from_background(status: BackgroundSubprocessStatus) -> LocalAgentStatus {
    match status {
        BackgroundSubprocessStatus::Starting => LocalAgentStatus::Starting,
        BackgroundSubprocessStatus::Running => LocalAgentStatus::Running,
        BackgroundSubprocessStatus::Stopped => LocalAgentStatus::Stopped,
        BackgroundSubprocessStatus::Error => LocalAgentStatus::Error,
    }
}

/// Decode exec-session liveness from the typed lifecycle state (not the
/// display label), preserving the exit code for the `exited (code)` row.
fn local_agent_status_from_exec_session(metadata: &VTCodeExecSession) -> LocalAgentStatus {
    match metadata.lifecycle_state {
        Some(VTCodeSessionLifecycleState::Running) => LocalAgentStatus::Running,
        Some(VTCodeSessionLifecycleState::Exited) => LocalAgentStatus::Exited { code: metadata.exit_code },
        None => LocalAgentStatus::Unknown,
    }
}

fn delegated_program_state(status: SubagentStatus) -> ProgramState {
    match status {
        SubagentStatus::Queued | SubagentStatus::Running | SubagentStatus::Waiting => ProgramState::Working,
        SubagentStatus::Completed => ProgramState::Done,
        SubagentStatus::Failed => ProgramState::Error,
        SubagentStatus::Closed => ProgramState::Idle,
    }
}

fn background_program_state(entry: &BackgroundSubprocessEntry) -> ProgramState {
    if entry.termination_requested {
        return ProgramState::Idle;
    }
    match entry.status {
        BackgroundSubprocessStatus::Starting | BackgroundSubprocessStatus::Running => ProgramState::Working,
        BackgroundSubprocessStatus::Error => ProgramState::Error,
        BackgroundSubprocessStatus::Stopped => match entry.exit_code {
            Some(0) => ProgramState::Done,
            Some(_) => ProgramState::Error,
            None => ProgramState::Idle,
        },
    }
}

fn command_program_state(metadata: &VTCodeExecSession, termination_requested: bool) -> ProgramState {
    if termination_requested {
        return ProgramState::Idle;
    }
    match metadata.exit_code {
        Some(0) => ProgramState::Done,
        Some(_) => ProgramState::Error,
        None if metadata.lifecycle_state == Some(VTCodeSessionLifecycleState::Running) => ProgramState::Working,
        None => ProgramState::Idle,
    }
}

fn exec_session_summary(metadata: &VTCodeExecSession) -> String {
    format!(
        "cwd {} · pid {}",
        metadata.working_dir.as_deref().unwrap_or("unknown"),
        metadata.child_pid.map_or_else(|| "-".to_string(), |pid| pid.to_string())
    )
}

/// Retain live and finished delegated agents so the expanded window can show
/// history. `Closed` stays hidden — that status means the user dismissed it.
pub(super) fn visible_delegated_local_agents(entries: Vec<SubagentStatusEntry>) -> Vec<SubagentStatusEntry> {
    let mut entries = entries
        .into_iter()
        .filter(|entry| !matches!(entry.status, SubagentStatus::Closed))
        .collect::<Vec<_>>();
    entries.sort_by_key(|left| std::cmp::Reverse(left.updated_at));
    entries
}

/// Retain live and finished background subprocesses (`Starting`, `Running`,
/// `Stopped`, `Error`) so the window can show what finished. There is no
/// dismissed/closed status on this type today.
pub(super) fn visible_background_local_agents(
    mut entries: Vec<BackgroundSubprocessEntry>,
) -> Vec<BackgroundSubprocessEntry> {
    entries.sort_by_key(|left| std::cmp::Reverse(left.updated_at));
    entries
}

fn delegated_local_agent_summary(entry: &SubagentStatusEntry, snapshot: &SubagentThreadSnapshot) -> String {
    entry
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if snapshot.snapshot.turn_in_flight {
                "Turn in flight; streaming live updates.".to_string()
            } else if matches!(entry.status, SubagentStatus::Failed) {
                entry
                    .error
                    .as_deref()
                    .map(str::trim)
                    .filter(|error| !error.is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| "Delegated agent failed before producing a summary.".to_string())
            } else if matches!(entry.status, SubagentStatus::Queued) {
                "Queued and waiting to start.".to_string()
            } else {
                "Running without a final summary yet.".to_string()
            }
        })
}

fn delegated_local_agent_preview(entry: &SubagentStatusEntry, snapshot: &SubagentThreadSnapshot) -> String {
    let preview = summarize_subagent_sidebar_preview(snapshot);
    if preview.trim().is_empty() {
        delegated_local_agent_preview_placeholder(entry)
    } else {
        preview
    }
}

pub(super) fn delegated_local_agent_preview_placeholder(entry: &SubagentStatusEntry) -> String {
    if matches!(entry.status, SubagentStatus::Queued) {
        "Agent is queued and has not emitted transcript output yet.".to_string()
    } else if matches!(entry.status, SubagentStatus::Failed) {
        entry
            .error
            .as_deref()
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "Agent failed before emitting more transcript output.".to_string())
    } else {
        "Waiting for the next delegated transcript update.".to_string()
    }
}

fn background_local_agent_summary(entry: &BackgroundSubprocessEntry) -> String {
    entry
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| match entry.status {
            BackgroundSubprocessStatus::Starting => "Starting; waiting for subprocess output.".to_string(),
            BackgroundSubprocessStatus::Running => "Running; waiting for transcript output.".to_string(),
            BackgroundSubprocessStatus::Stopped => "Stopped.".to_string(),
            BackgroundSubprocessStatus::Error => "Exited with an error.".to_string(),
        })
}

fn background_local_agent_preview(snapshot: &BackgroundSubprocessSnapshot) -> String {
    if snapshot.preview.trim().is_empty() {
        background_local_agent_preview_placeholder(&snapshot.entry)
    } else {
        snapshot.preview.clone()
    }
}

pub(super) fn background_local_agent_preview_placeholder(entry: &BackgroundSubprocessEntry) -> String {
    match entry.status {
        BackgroundSubprocessStatus::Starting => "Waiting for the subprocess to emit output...".to_string(),
        BackgroundSubprocessStatus::Running => {
            "Subprocess is running; waiting for the next transcript update.".to_string()
        }
        BackgroundSubprocessStatus::Stopped => "Subprocess stopped.".to_string(),
        BackgroundSubprocessStatus::Error => "Subprocess ended with an error.".to_string(),
    }
}

fn summarize_subagent_sidebar_preview(snapshot: &SubagentThreadSnapshot) -> String {
    let live_preview = summarize_thread_event_preview(&snapshot.recent_events);
    if !live_preview.is_empty() {
        return live_preview;
    }

    // Take the 16 newest previewable messages (rev → take) then restore
    // chronological order with an in-place `reverse()` — one allocation
    // instead of two (`collect` → `into_iter().rev().collect()` allocated a
    // second Vec just to reverse).
    let mut lines: Vec<String> = snapshot
        .snapshot
        .messages
        .iter()
        .rev()
        .filter_map(|message| {
            let text = message.content.as_text();
            let preview = summarize_preview_text(text.as_ref())?;
            Some(format!("{:?}: {}", message.role, preview))
        })
        .take(16)
        .collect();
    lines.reverse();
    lines.join("\n")
}

pub(crate) fn summarize_thread_event_preview(events: &[ThreadEvent]) -> String {
    let mut items = Vec::<(String, String)>::new();
    for event in events {
        let Some((item_id, line)) = thread_event_preview_line(event) else {
            continue;
        };
        if let Some((_, current)) = items.iter_mut().find(|(id, _)| id == &item_id) {
            *current = line;
        } else {
            items.push((item_id, line));
        }
    }

    // Take the 16 newest lines (rev → take) then restore chronological order
    // with an in-place `reverse()` — one allocation instead of two
    // (`collect` → `into_iter().rev().collect()` allocated a second Vec).
    let mut lines: Vec<String> = items.into_iter().map(|(_, line)| line).rev().take(16).collect();
    lines.reverse();
    lines.join("\n")
}

fn thread_event_preview_line(event: &ThreadEvent) -> Option<(String, String)> {
    let item = match event {
        ThreadEvent::ItemStarted(event) => &event.item,
        ThreadEvent::ItemUpdated(event) => &event.item,
        ThreadEvent::ItemCompleted(event) => &event.item,
        _ => return None,
    };

    let line = match &item.details {
        ThreadItemDetails::AgentMessage(message) => {
            format!("assistant: {}", summarize_preview_text(&message.text)?)
        }
        ThreadItemDetails::Reasoning(reasoning) => {
            if let Some(diagnostic) = &reasoning.diagnostic {
                let diagnostic = vtcode_commons::diagnostics::redact_diagnostic(diagnostic);
                format!("diagnostic [{}]: {}", diagnostic.code, diagnostic.text())
            } else {
                format!("thinking: {}", summarize_preview_text(&reasoning.text)?)
            }
        }
        ThreadItemDetails::ToolInvocation(tool) => {
            format!("tool {}: {}", tool.tool_name, tool_status_label(tool.status.clone()))
        }
        ThreadItemDetails::ToolOutput(output) => summarize_preview_text(&output.output)
            .map(|text| format!("tool output: {text}"))
            .unwrap_or_else(|| format!("tool output: {}", tool_status_label(output.status.clone()))),
        ThreadItemDetails::CommandExecution(command) => summarize_preview_text(&command.aggregated_output)
            .map(|text| format!("command {}: {}", command.command, text))
            .unwrap_or_else(|| {
                format!("command {}: {}", command.command, command_status_label(command.status.clone()))
            }),
        _ => return None,
    };

    Some((item.id.clone(), line))
}

fn tool_status_label(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::InProgress => "running",
    }
}

fn command_status_label(status: CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::InProgress => "running",
    }
}

/// Marker line introducing the structured metadata block core appends to
/// aggregated tool output. Contract with `structured_stream_metadata` in
/// `vtcode-core/src/core/agent/events/lifecycle/summary.rs`: previews stop
/// above it so a session row never previews a raw JSON tail (`}`).
const STRUCTURED_OUTPUT_MARKER: &str = "Structured output:";

fn summarize_preview_text(text: &str) -> Option<String> {
    // Prefer the last meaningful line above the metadata envelope; fall back
    // to the whole text when the head holds nothing, so degenerate
    // envelope-only captures still preview something.
    let preview = last_meaningful_line(truncate_before_envelope(text)).or_else(|| last_meaningful_line(text))?;

    Some(truncate_preview_text(preview, 180))
}

/// Text above the first `Structured output:` marker line, if any.
///
/// The marker must open its line — a mid-line mention is prose, not an
/// envelope. Cutting at the first (not last) marker also skips nested
/// echoes a tool may have catted above core's trailing block.
fn truncate_before_envelope(text: &str) -> &str {
    let cut = text
        .match_indices(STRUCTURED_OUTPUT_MARKER)
        .find_map(|(pos, _)| (pos == 0 || text.as_bytes().get(pos - 1) == Some(&b'\n')).then_some(pos));
    cut.and_then(|cut| text.get(..cut)).unwrap_or(text)
}

fn last_meaningful_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .find_map(|line| {
            let collapsed = collapse_preview_whitespace(line);
            (!collapsed.is_empty()).then_some(collapsed)
        })
        .or_else(|| {
            let collapsed = collapse_preview_whitespace(text);
            (!collapsed.is_empty()).then_some(collapsed)
        })
}

fn collapse_preview_whitespace(text: &str) -> String {
    vtcode_commons::formatting::collapse_whitespace(text)
}

fn truncate_preview_text(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }

    let mut truncated = text.chars().take(max_chars.saturating_sub(1)).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_diagnostic_preview_retains_details_without_private_reasoning_label() {
        use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};
        use vtcode_core::exec::events::{ItemCompletedEvent, ReasoningItem, ThreadItem};
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "incident-2".into(),
                details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                    text: "legacy summary".into(),
                    stage: Some("diagnosis".into()),
                    diagnostic: Some(Box::new(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        code: "planning.awaiting_input".into(),
                        summary: "Planning needs input.".into(),
                        agent_details: "attempts=3; password=preview-private-secret".into(),
                        correlation_id: "incident-2".into(),
                        actions: Vec::new(),
                    })),
                })),
            },
        });
        let (_, preview) = thread_event_preview_line(&event).unwrap();
        assert!(preview.starts_with("diagnostic [planning.awaiting_input]"));
        assert!(preview.contains("attempts=3"));
        assert!(!preview.contains("preview-private-secret"));
        assert!(!preview.contains("thinking:"));
    }

    #[test]
    fn program_status_child_outcomes_use_typed_evidence() {
        for status in [SubagentStatus::Queued, SubagentStatus::Running, SubagentStatus::Waiting] {
            assert_eq!(delegated_program_state(status), ProgramState::Working);
        }
        assert_eq!(delegated_program_state(SubagentStatus::Completed), ProgramState::Done);
        assert_eq!(delegated_program_state(SubagentStatus::Failed), ProgramState::Error);
        let mut metadata: VTCodeExecSession = serde_json::from_value(serde_json::json!({"id":"opaque-session", "command":"ignored", "backend":"pipe", "args":[], "working_dir":null})).unwrap();
        assert_eq!(command_program_state(&metadata, false), ProgramState::Idle);
        metadata.lifecycle_state = Some(VTCodeSessionLifecycleState::Running);
        assert_eq!(command_program_state(&metadata, false), ProgramState::Working);
        metadata.exit_code = Some(0);
        assert_eq!(command_program_state(&metadata, false), ProgramState::Done);
        metadata.exit_code = Some(17);
        assert_eq!(command_program_state(&metadata, false), ProgramState::Error);
        assert_eq!(command_program_state(&metadata, true), ProgramState::Idle);
        let now = chrono::Utc::now();
        let mut entry: BackgroundSubprocessEntry = serde_json::from_value(serde_json::json!({
            "id":"stable", "session_id":"old", "exec_session_id":"old", "agent_name":"demo", "display_label":"demo", "description":"", "source":"test", "status":"stopped", "desired_enabled":false, "created_at":now, "updated_at":now
        })).unwrap();
        assert_eq!(background_program_state(&entry), ProgramState::Idle);
        entry.exit_code = Some(0);
        assert_eq!(background_program_state(&entry), ProgramState::Done);
        entry.exit_code = Some(23);
        assert_eq!(background_program_state(&entry), ProgramState::Error);
        entry.termination_requested = true;
        assert_eq!(background_program_state(&entry), ProgramState::Idle);
    }

    #[test]
    fn local_agent_status_decodes_each_producer_exhaustively() {
        for (source, expected) in [
            (SubagentStatus::Queued, LocalAgentStatus::Queued),
            (SubagentStatus::Running, LocalAgentStatus::Running),
            (SubagentStatus::Waiting, LocalAgentStatus::Waiting),
            (SubagentStatus::Completed, LocalAgentStatus::Completed),
            (SubagentStatus::Failed, LocalAgentStatus::Failed),
            (SubagentStatus::Closed, LocalAgentStatus::Closed),
        ] {
            let decoded = local_agent_status_from_subagent(source);
            assert_eq!(decoded, expected, "{source:?} mapping");
            assert_eq!(decoded.to_string(), source.as_str(), "{source:?} label");
        }

        for (source, expected) in [
            (BackgroundSubprocessStatus::Starting, LocalAgentStatus::Starting),
            (BackgroundSubprocessStatus::Running, LocalAgentStatus::Running),
            (BackgroundSubprocessStatus::Stopped, LocalAgentStatus::Stopped),
            (BackgroundSubprocessStatus::Error, LocalAgentStatus::Error),
        ] {
            let decoded = local_agent_status_from_background(source);
            assert_eq!(decoded, expected, "{source:?} mapping");
            assert_eq!(decoded.to_string(), source.as_str(), "{source:?} label");
        }

        let mut metadata: VTCodeExecSession = serde_json::from_value(serde_json::json!({"id":"opaque-session", "command":"ignored", "backend":"pipe", "args":[], "working_dir":null})).unwrap();
        assert_eq!(local_agent_status_from_exec_session(&metadata), LocalAgentStatus::Unknown);
        metadata.lifecycle_state = Some(VTCodeSessionLifecycleState::Running);
        assert_eq!(local_agent_status_from_exec_session(&metadata), LocalAgentStatus::Running);
        metadata.lifecycle_state = Some(VTCodeSessionLifecycleState::Exited);
        metadata.exit_code = None;
        assert_eq!(local_agent_status_from_exec_session(&metadata), LocalAgentStatus::Exited { code: None });
        metadata.exit_code = Some(3);
        let exited = local_agent_status_from_exec_session(&metadata);
        assert_eq!(exited, LocalAgentStatus::Exited { code: Some(3) });
        assert_eq!(exited.to_string(), "exited (3)");

        // Liveness matches the pre-refactor per-kind string sets.
        assert!(LocalAgentStatus::Queued.is_live());
        assert!(LocalAgentStatus::Starting.is_live());
        assert!(!LocalAgentStatus::Unknown.is_live());
        assert!(!LocalAgentStatus::Exited { code: Some(0) }.is_live());
    }

    #[test]
    fn preview_stops_above_structured_envelope() {
        // Trailing metadata never previews as a raw JSON tail.
        assert_eq!(summarize_preview_text("output line\nStructured output:\n{\n}").as_deref(), Some("output line"));
        // Marker-less output keeps the last meaningful line.
        assert_eq!(summarize_preview_text("first\nsecond").as_deref(), Some("second"));
        // Asymmetric: a mid-line mention is prose, not an envelope.
        assert_eq!(
            summarize_preview_text("see Structured output: docs").as_deref(),
            Some("see Structured output: docs")
        );
        // Degenerate envelope-only captures still preview something.
        assert_eq!(summarize_preview_text("Structured output:\n}").as_deref(), Some("}"));
    }
}
