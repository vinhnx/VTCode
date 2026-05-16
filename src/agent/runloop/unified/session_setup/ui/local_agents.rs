use anyhow::Result;
use std::sync::Arc;
use vtcode_core::subagents::{
    BackgroundSubprocessEntry, BackgroundSubprocessSnapshot, BackgroundSubprocessStatus,
    SubagentController, SubagentStatus, SubagentStatusEntry, SubagentThreadSnapshot,
};
use vtcode_core::{CommandExecutionStatus, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use vtcode_tui::app::{InlineHandle, LocalAgentEntry, LocalAgentKind};

pub(crate) async fn refresh_local_agents(
    handle: &InlineHandle,
    controller: &Arc<SubagentController>,
) -> Result<()> {
    let background_entries = controller.refresh_background_processes().await?;
    let delegated_entries = controller.status_entries().await;
    let local_agents =
        build_local_agent_entries(controller, delegated_entries, background_entries).await;
    handle.set_local_agents(local_agents);
    Ok(())
}

async fn build_local_agent_entries(
    controller: &Arc<SubagentController>,
    delegated_entries: Vec<SubagentStatusEntry>,
    background_entries: Vec<BackgroundSubprocessEntry>,
) -> Vec<LocalAgentEntry> {
    let mut entries = Vec::new();

    for entry in visible_delegated_local_agents(delegated_entries) {
        let snapshot = match controller.snapshot_for_thread(&entry.id).await {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                tracing::debug!(
                    subagent_id = entry.id.as_str(),
                    "Failed to snapshot delegated agent for local-agents UI: {}",
                    err
                );
                None
            }
        };
        let preview = snapshot
            .as_ref()
            .map(|snapshot| delegated_local_agent_preview(&entry, snapshot))
            .unwrap_or_else(|| delegated_local_agent_preview_placeholder(&entry));
        let summary = snapshot
            .as_ref()
            .map(|snapshot| delegated_local_agent_summary(&entry, snapshot));
        entries.push((
            entry.updated_at,
            LocalAgentEntry {
                id: entry.id.clone(),
                display_label: entry.display_label.clone(),
                agent_name: entry.agent_name.clone(),
                color: entry.color.clone(),
                kind: LocalAgentKind::Delegated,
                status: entry.status.as_str().to_string(),
                summary,
                preview,
                transcript_path: entry.transcript_path.clone(),
            },
        ));
    }

    for entry in visible_background_local_agents(background_entries) {
        let snapshot = match controller.background_snapshot(&entry.id).await {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                tracing::debug!(
                    subprocess_id = entry.id.as_str(),
                    "Failed to snapshot background subprocess for local-agents UI: {}",
                    err
                );
                None
            }
        };
        let preview = snapshot
            .as_ref()
            .map(background_local_agent_preview)
            .unwrap_or_else(|| background_local_agent_preview_placeholder(&entry));
        entries.push((
            entry.updated_at,
            LocalAgentEntry {
                id: entry.id.clone(),
                display_label: entry.display_label.clone(),
                agent_name: entry.agent_name.clone(),
                color: entry.color.clone(),
                kind: LocalAgentKind::Background,
                status: entry.status.as_str().to_string(),
                summary: Some(background_local_agent_summary(&entry)),
                preview,
                transcript_path: entry.transcript_path.clone().or(entry.archive_path.clone()),
            },
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    entries.into_iter().map(|(_, entry)| entry).collect()
}

pub(super) fn visible_delegated_local_agents(
    entries: Vec<SubagentStatusEntry>,
) -> Vec<SubagentStatusEntry> {
    let mut entries = entries
        .into_iter()
        .filter(|entry| {
            !matches!(
                entry.status,
                SubagentStatus::Completed | SubagentStatus::Closed
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    entries
}

pub(super) fn visible_background_local_agents(
    entries: Vec<BackgroundSubprocessEntry>,
) -> Vec<BackgroundSubprocessEntry> {
    let mut entries = entries
        .into_iter()
        .filter(|entry| {
            matches!(
                entry.status,
                BackgroundSubprocessStatus::Starting | BackgroundSubprocessStatus::Running
            ) || (entry.desired_enabled
                && matches!(entry.status, BackgroundSubprocessStatus::Error))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    entries
}

fn delegated_local_agent_summary(
    entry: &SubagentStatusEntry,
    snapshot: &SubagentThreadSnapshot,
) -> String {
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
                    .unwrap_or_else(|| {
                        "Delegated agent failed before producing a summary.".to_string()
                    })
            } else if matches!(entry.status, SubagentStatus::Queued) {
                "Queued and waiting to start.".to_string()
            } else {
                "Running without a final summary yet.".to_string()
            }
        })
}

fn delegated_local_agent_preview(
    entry: &SubagentStatusEntry,
    snapshot: &SubagentThreadSnapshot,
) -> String {
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
            BackgroundSubprocessStatus::Starting => {
                "Starting; waiting for subprocess output.".to_string()
            }
            BackgroundSubprocessStatus::Running => {
                "Running; waiting for transcript output.".to_string()
            }
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

pub(super) fn background_local_agent_preview_placeholder(
    entry: &BackgroundSubprocessEntry,
) -> String {
    match entry.status {
        BackgroundSubprocessStatus::Starting => {
            "Waiting for the subprocess to emit output...".to_string()
        }
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

    snapshot
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
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn summarize_thread_event_preview(events: &[ThreadEvent]) -> String {
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

    items
        .into_iter()
        .map(|(_, line)| line)
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
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
            format!("thinking: {}", summarize_preview_text(&reasoning.text)?)
        }
        ThreadItemDetails::ToolInvocation(tool) => {
            format!(
                "tool {}: {}",
                tool.tool_name,
                tool_status_label(tool.status.clone())
            )
        }
        ThreadItemDetails::ToolOutput(output) => summarize_preview_text(&output.output)
            .map(|text| format!("tool output: {}", text))
            .unwrap_or_else(|| {
                format!("tool output: {}", tool_status_label(output.status.clone()))
            }),
        ThreadItemDetails::CommandExecution(command) => {
            summarize_preview_text(&command.aggregated_output)
                .map(|text| format!("command {}: {}", command.command, text))
                .unwrap_or_else(|| {
                    format!(
                        "command {}: {}",
                        command.command,
                        command_status_label(command.status.clone())
                    )
                })
        }
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

fn summarize_preview_text(text: &str) -> Option<String> {
    let preview = text
        .lines()
        .rev()
        .find_map(|line| {
            let collapsed = collapse_preview_whitespace(line);
            (!collapsed.is_empty()).then_some(collapsed)
        })
        .or_else(|| {
            let collapsed = collapse_preview_whitespace(text);
            (!collapsed.is_empty()).then_some(collapsed)
        })?;

    Some(truncate_preview_text(preview, 180))
}

fn collapse_preview_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_preview_text(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}
