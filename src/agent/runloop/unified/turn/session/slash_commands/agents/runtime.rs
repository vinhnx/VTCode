use anyhow::{Result, anyhow};
use std::path::PathBuf;
use std::time::Duration;
use vtcode_core::subagents::{BackgroundSubprocessEntry, SubagentStatusEntry};
use vtcode_core::utils::ansi::MessageStyle;
#[cfg(test)]
use vtcode_core::{CommandExecutionStatus, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use vtcode_tui::app::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, ListOverlayRequest,
    TransientEvent, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey, TransientRequest,
    TransientSelectionChange, TransientSubmission,
};

use crate::agent::runloop::unified::session_setup::refresh_local_agents;

use super::{
    ACTIVE_AGENT_INSPECTOR_REFRESH_MS, SUBAGENT_CONTROLLER_INACTIVE_MESSAGE,
    SUBPROCESS_ARCHIVE_PREFIX, SUBPROCESS_CANCEL_PREFIX, SUBPROCESS_STOP_PREFIX,
    SUBPROCESS_TRANSCRIPT_PREFIX, SlashCommandContext, SlashCommandControl, THREAD_CANCEL_PREFIX,
    THREAD_INSPECT_PREFIX, THREAD_TRANSCRIPT_PREFIX, render_missing_subagent_controller,
    status_label, wait_for_list_modal_selection,
};

pub(super) async fn close_subagent_entry(
    ctx: &mut SlashCommandContext<'_>,
    controller: &std::sync::Arc<vtcode_core::subagents::SubagentController>,
    id: &str,
    display_label: &str,
) -> Result<SlashCommandControl> {
    controller.close(id).await?;
    refresh_local_agents(ctx.handle, controller).await?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Closed delegated agent {}.", display_label),
    )?;
    Ok(SlashCommandControl::Continue)
}

pub(super) async fn apply_background_subprocess_action(
    ctx: &mut SlashCommandContext<'_>,
    controller: &std::sync::Arc<vtcode_core::subagents::SubagentController>,
    id: &str,
    force: bool,
) -> Result<BackgroundSubprocessEntry> {
    let entry = if force {
        controller.force_cancel_background(id).await?
    } else {
        controller.graceful_stop_background(id).await?
    };
    refresh_local_agents(ctx.handle, controller).await?;
    Ok(entry)
}

pub(super) async fn show_threads_modal(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        return render_missing_subagent_controller(&mut ctx);
    };

    loop {
        let threads = visible_subagent_entries(controller.status_entries().await);
        let active_count = threads
            .iter()
            .filter(|entry| !entry.status.is_terminal())
            .count();
        if threads.is_empty() {
            ctx.renderer.line(
                MessageStyle::Info,
                "No delegated agents are available in this session.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }

        let items = threads
            .iter()
            .map(|entry| InlineListItem {
                title: format!("{} {}", entry.display_label, status_label(entry.status)),
                subtitle: Some(format!(
                    "{} | {} | {}",
                    entry.agent_name,
                    entry.source,
                    entry.summary.as_deref().unwrap_or("No summary yet")
                )),
                badge: Some(if entry.background {
                    "Background".to_string()
                } else {
                    "Foreground".to_string()
                }),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(format!(
                    "{THREAD_INSPECT_PREFIX}{}",
                    entry.id
                ))),
                search_value: Some(format!(
                    "{} {} {} {}",
                    entry.id, entry.display_label, entry.agent_name, entry.description
                )),
            })
            .collect::<Vec<_>>();
        let selected = items.first().and_then(|item| item.selection.clone());
        ctx.handle
            .show_transient(TransientRequest::List(ListOverlayRequest {
                title: "Delegated agents".to_string(),
                lines: vec![
                    format!("Main session stays on {}.", ctx.active_thread_label),
                    if active_count > 0 {
                        format!(
                            "{active_count} active. Recent completed runs remain inspectable here."
                        )
                    } else {
                        "No active delegated agents right now. Recent runs remain inspectable here."
                            .to_string()
                    },
                    "Select an agent for transcript or lifecycle actions. Live preview stays in the sidebar."
                        .to_string(),
                ],
                footer_hint: Some(
                    "enter inspect · ctrl-r reload · ctrl-k close selected agent · esc close"
                        .to_string(),
                ),
                items,
                selected: selected.clone(),
                search: Some(InlineListSearchConfig {
                    label: "Search active agents".to_string(),
                    placeholder: Some("id, agent, source, status".to_string()),
                }),
                hotkeys: vec![
                    TransientHotkey {
                        key: TransientHotkeyKey::CtrlChar('r'),
                        action: TransientHotkeyAction::ReloadSubagentInspector,
                    },
                    TransientHotkey {
                        key: TransientHotkeyKey::CtrlChar('k'),
                        action: TransientHotkeyAction::GracefulStopSubagent,
                    },
                ],
            }));

        let Some(action) = wait_for_inspector_action(
            ctx.handle,
            ctx.session,
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
            selected,
            None,
        )
        .await
        else {
            return Ok(SlashCommandControl::Continue);
        };

        match action.kind {
            InspectorActionKind::Reload => continue,
            InspectorActionKind::Inspect
            | InspectorActionKind::GracefulStop
            | InspectorActionKind::ForceCancel => {
                let Some(id) =
                    selection_config_action(action.selection.as_ref(), THREAD_INSPECT_PREFIX)
                else {
                    return Ok(SlashCommandControl::Continue);
                };
                let entry = threads
                    .iter()
                    .find(|entry| entry.id == id)
                    .cloned()
                    .ok_or_else(|| anyhow!("Unknown delegated thread {}", id))?;
                if matches!(action.kind, InspectorActionKind::Inspect) {
                    return show_active_agent_inspector(&mut ctx, entry).await;
                }
                if confirm_subagent_cancellation(&mut ctx, entry.display_label.as_str()).await? {
                    controller.close(&entry.id).await?;
                    refresh_local_agents(ctx.handle, &controller).await?;
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Closed delegated agent {}.", entry.display_label),
                    )?;
                }
                return Ok(SlashCommandControl::Continue);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InspectorAction {
    kind: InspectorActionKind,
    selection: Option<InlineListSelection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorActionKind {
    Inspect,
    Reload,
    GracefulStop,
    ForceCancel,
}

async fn wait_for_inspector_action(
    handle: &vtcode_tui::app::InlineHandle,
    session: &mut vtcode_tui::app::InlineSession,
    ctrl_c_state: &std::sync::Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &std::sync::Arc<tokio::sync::Notify>,
    initial_selection: Option<InlineListSelection>,
    auto_reload_after: Option<Duration>,
) -> Option<InspectorAction> {
    let mut current_selection = initial_selection;
    loop {
        if ctrl_c_state.is_cancel_requested() {
            handle.close_transient();
            handle.force_redraw();
            return None;
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            _ = async {
                if let Some(delay) = auto_reload_after {
                    tokio::time::sleep(delay).await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                return Some(InspectorAction {
                    kind: InspectorActionKind::Reload,
                    selection: current_selection.clone(),
                });
            }
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            handle.close_transient();
            handle.force_redraw();
            return None;
        };

        match event {
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::List(selection),
            )) => {
                current_selection = Some(selection);
            }
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Selection(selection),
            )) => {
                ctrl_c_state.reset();
                return Some(InspectorAction {
                    kind: InspectorActionKind::Inspect,
                    selection: Some(selection),
                });
            }
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Hotkey(action),
            )) => {
                ctrl_c_state.reset();
                let kind = match action {
                    TransientHotkeyAction::ReloadSubagentInspector => InspectorActionKind::Reload,
                    TransientHotkeyAction::GracefulStopSubagent => {
                        InspectorActionKind::GracefulStop
                    }
                    TransientHotkeyAction::ForceCancelSubagent => InspectorActionKind::ForceCancel,
                    _ => continue,
                };
                return Some(InspectorAction {
                    kind,
                    selection: current_selection.clone(),
                });
            }
            vtcode_tui::app::InlineEvent::Transient(TransientEvent::Cancelled)
            | vtcode_tui::app::InlineEvent::Cancel
            | vtcode_tui::app::InlineEvent::Exit => {
                ctrl_c_state.reset();
                return None;
            }
            vtcode_tui::app::InlineEvent::Interrupt => {
                handle.close_transient();
                handle.force_redraw();
                return None;
            }
            _ => {}
        }
    }
}

pub(super) async fn show_active_agent_inspector(
    ctx: &mut SlashCommandContext<'_>,
    entry: SubagentStatusEntry,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        return Ok(SlashCommandControl::Continue);
    };
    let agent_id = entry.id.clone();
    let mut selected_override = None;
    loop {
        let current_entry = controller.status_for(&agent_id).await?;
        let snapshot = controller.snapshot_for_thread(&agent_id).await?;
        let summary = active_agent_summary(&current_entry, &snapshot);
        let refresh_after = active_agent_inspector_refresh_after(&current_entry, &snapshot);

        let items = active_agent_inspector_items(&current_entry);
        let selected = selected_override
            .clone()
            .or_else(|| items.first().and_then(|item| item.selection.clone()));
        ctx.handle
            .show_transient(TransientRequest::List(ListOverlayRequest {
                title: format!("Agent {}", current_entry.display_label),
                lines: vec![
                    format!("Status: {}", current_entry.status.as_str()),
                    format!(
                        "Turn active: {}",
                        if snapshot.snapshot.turn_in_flight {
                            "yes"
                        } else {
                            "no"
                        }
                    ),
                    format!(
                        "Mode: {}",
                        if current_entry.background {
                            "background"
                        } else {
                            "foreground"
                        }
                    ),
                    format!("Source: {}", current_entry.source),
                    format!("Session: {}", current_entry.session_id),
                    format!("Started: {}", format_datetime(current_entry.created_at)),
                    format!("Updated: {}", format_datetime(current_entry.updated_at)),
                    format!("Summary: {}", summary),
                    format!("Live updates: {}", inspector_live_updates_label(refresh_after)),
                    "Live preview: sidebar panel".to_string(),
                    if let Some(error) = current_entry.error.as_deref() {
                        format!("Error: {}", error)
                    } else {
                        "Error: none".to_string()
                    },
                ],
                footer_hint: Some(
                    if refresh_after.is_some() {
                        "live preview in sidebar · auto-refresh status · enter open action · ctrl-r reload · ctrl-k close agent · esc close"
                            .to_string()
                    } else {
                        "live preview in sidebar · enter open action · ctrl-r reload · ctrl-k close agent · esc close"
                            .to_string()
                    },
                ),
                items,
                selected: selected.clone(),
                search: None,
                hotkeys: vec![
                    TransientHotkey {
                        key: TransientHotkeyKey::CtrlChar('r'),
                        action: TransientHotkeyAction::ReloadSubagentInspector,
                    },
                    TransientHotkey {
                        key: TransientHotkeyKey::CtrlChar('k'),
                        action: TransientHotkeyAction::GracefulStopSubagent,
                    },
                ],
            }));

        let Some(action) = wait_for_inspector_action(
            ctx.handle,
            ctx.session,
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
            selected,
            refresh_after,
        )
        .await
        else {
            return Ok(SlashCommandControl::Continue);
        };
        selected_override = action.selection.clone();

        match action.kind {
            InspectorActionKind::Reload => continue,
            InspectorActionKind::GracefulStop | InspectorActionKind::ForceCancel => {
                if confirm_subagent_cancellation(ctx, current_entry.display_label.as_str()).await? {
                    controller.close(&agent_id).await?;
                    refresh_local_agents(ctx.handle, &controller).await?;
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Closed delegated agent {}.", current_entry.display_label),
                    )?;
                }
                return Ok(SlashCommandControl::Continue);
            }
            InspectorActionKind::Inspect => {
                if let Some(path) = selection_path(
                    action.selection.as_ref(),
                    THREAD_TRANSCRIPT_PREFIX,
                    &current_entry.transcript_path,
                ) {
                    return launch_editor_path(ctx, path).await;
                }
                if selection_config_action(action.selection.as_ref(), THREAD_CANCEL_PREFIX)
                    .is_some()
                    && confirm_subagent_cancellation(ctx, current_entry.display_label.as_str())
                        .await?
                {
                    controller.close(&agent_id).await?;
                    refresh_local_agents(ctx.handle, &controller).await?;
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Closed delegated agent {}.", current_entry.display_label),
                    )?;
                }
                return Ok(SlashCommandControl::Continue);
            }
        }
    }
}

pub(super) async fn show_background_subprocess_inspector(
    ctx: &mut SlashCommandContext<'_>,
    entry: BackgroundSubprocessEntry,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        return Ok(SlashCommandControl::Continue);
    };
    let record_id = entry.id.clone();
    let mut selected_override = None;
    loop {
        let snapshot = controller.background_snapshot(&record_id).await?;
        let current_entry = &snapshot.entry;
        let refresh_after = background_subprocess_refresh_after(
            current_entry,
            ctx.vt_cfg
                .as_ref()
                .map(|cfg| cfg.subagents.background.refresh_interval_ms)
                .unwrap_or(2_000),
        );
        let items = background_subprocess_inspector_items(current_entry);
        let selected = selected_override
            .clone()
            .or_else(|| items.first().and_then(|item| item.selection.clone()));
        ctx.handle
            .show_transient(TransientRequest::List(ListOverlayRequest {
                title: format!("Subprocess {}", current_entry.display_label),
                lines: vec![
                    format!("Status: {}", current_entry.status.as_str()),
                    format!(
                        "PID: {}",
                        current_entry
                            .pid
                            .map(|pid| pid.to_string())
                            .unwrap_or_else(|| "-".to_string())
                    ),
                    format!("Source: {}", current_entry.source),
                    format!("Session: {}", current_entry.session_id),
                    format!("Exec session: {}", current_entry.exec_session_id),
                    format!(
                        "Started: {}",
                        format_optional_datetime(current_entry.started_at)
                    ),
                    format!(
                        "Uptime: {}",
                        format_uptime(current_entry.started_at.unwrap_or(current_entry.created_at))
                    ),
                    format!("Summary: {}", background_subprocess_summary(current_entry)),
                    format!("Live updates: {}", inspector_live_updates_label(refresh_after)),
                    if let Some(error) = current_entry.error.as_deref() {
                        format!("Error: {}", error)
                    } else {
                        "Error: none".to_string()
                    },
                    format!(
                        "Preview:\n{}",
                        if snapshot.preview.trim().is_empty() {
                            background_subprocess_preview_placeholder(current_entry)
                        } else {
                            snapshot.preview.clone()
                        }
                    ),
                ],
                footer_hint: Some(
                    if refresh_after.is_some() {
                        "auto-refresh while active · enter open action · ctrl-r reload · ctrl-k graceful stop · ctrl-x force cancel"
                            .to_string()
                    } else {
                        "enter open action · ctrl-r reload · ctrl-k graceful stop · ctrl-x force cancel"
                            .to_string()
                    },
                ),
                items,
                selected: selected.clone(),
                search: None,
                hotkeys: vec![
                    TransientHotkey {
                        key: TransientHotkeyKey::CtrlChar('r'),
                        action: TransientHotkeyAction::ReloadSubagentInspector,
                    },
                    TransientHotkey {
                        key: TransientHotkeyKey::CtrlChar('k'),
                        action: TransientHotkeyAction::GracefulStopSubagent,
                    },
                    TransientHotkey {
                        key: TransientHotkeyKey::CtrlChar('x'),
                        action: TransientHotkeyAction::ForceCancelSubagent,
                    },
                ],
            }));

        let Some(action) = wait_for_inspector_action(
            ctx.handle,
            ctx.session,
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
            selected,
            refresh_after,
        )
        .await
        else {
            return Ok(SlashCommandControl::Continue);
        };
        selected_override = action.selection.clone();

        match action.kind {
            InspectorActionKind::Reload => continue,
            InspectorActionKind::GracefulStop => {
                if confirm_subprocess_action(ctx, current_entry.display_label.as_str(), false)
                    .await?
                {
                    let updated = controller.graceful_stop_background(&record_id).await?;
                    refresh_local_agents(ctx.handle, &controller).await?;
                    render_subprocess_status(ctx, &updated)?;
                }
                return Ok(SlashCommandControl::Continue);
            }
            InspectorActionKind::ForceCancel => {
                if confirm_subprocess_action(ctx, current_entry.display_label.as_str(), true)
                    .await?
                {
                    let updated = controller.force_cancel_background(&record_id).await?;
                    refresh_local_agents(ctx.handle, &controller).await?;
                    render_subprocess_status(ctx, &updated)?;
                }
                return Ok(SlashCommandControl::Continue);
            }
            InspectorActionKind::Inspect => {
                if let Some(path) = selection_path(
                    action.selection.as_ref(),
                    SUBPROCESS_TRANSCRIPT_PREFIX,
                    &current_entry.transcript_path,
                ) {
                    return launch_editor_path(ctx, path).await;
                }
                if let Some(path) = selection_path(
                    action.selection.as_ref(),
                    SUBPROCESS_ARCHIVE_PREFIX,
                    &current_entry.archive_path,
                ) {
                    return launch_editor_path(ctx, path).await;
                }
                if selection_config_action(action.selection.as_ref(), SUBPROCESS_STOP_PREFIX)
                    .is_some()
                    && confirm_subprocess_action(ctx, current_entry.display_label.as_str(), false)
                        .await?
                {
                    let updated = controller.graceful_stop_background(&record_id).await?;
                    refresh_local_agents(ctx.handle, &controller).await?;
                    render_subprocess_status(ctx, &updated)?;
                }
                if selection_config_action(action.selection.as_ref(), SUBPROCESS_CANCEL_PREFIX)
                    .is_some()
                    && confirm_subprocess_action(ctx, current_entry.display_label.as_str(), true)
                        .await?
                {
                    let updated = controller.force_cancel_background(&record_id).await?;
                    refresh_local_agents(ctx.handle, &controller).await?;
                    render_subprocess_status(ctx, &updated)?;
                }
                return Ok(SlashCommandControl::Continue);
            }
        }
    }
}

fn active_agent_inspector_refresh_after(
    entry: &SubagentStatusEntry,
    snapshot: &vtcode_core::subagents::SubagentThreadSnapshot,
) -> Option<Duration> {
    (!entry.status.is_terminal() || snapshot.snapshot.turn_in_flight)
        .then_some(Duration::from_millis(ACTIVE_AGENT_INSPECTOR_REFRESH_MS))
}

fn background_subprocess_refresh_after(
    entry: &BackgroundSubprocessEntry,
    refresh_interval_ms: u64,
) -> Option<Duration> {
    matches!(
        entry.status,
        vtcode_core::subagents::BackgroundSubprocessStatus::Starting
            | vtcode_core::subagents::BackgroundSubprocessStatus::Running
    )
    .then_some(Duration::from_millis(refresh_interval_ms.max(250)))
}

fn inspector_live_updates_label(refresh_after: Option<Duration>) -> String {
    refresh_after.map_or_else(
        || "manual (Ctrl+R)".to_string(),
        |delay| format!("auto every {} ms", delay.as_millis()),
    )
}

fn active_agent_summary(
    entry: &SubagentStatusEntry,
    snapshot: &vtcode_core::subagents::SubagentThreadSnapshot,
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
            } else if matches!(entry.status, vtcode_core::subagents::SubagentStatus::Queued) {
                "Queued and waiting to start.".to_string()
            } else if entry.status.is_terminal() {
                "No summary recorded".to_string()
            } else {
                "Running without a final summary yet.".to_string()
            }
        })
}

pub(super) fn background_subprocess_summary(entry: &BackgroundSubprocessEntry) -> String {
    entry
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| match entry.status {
            vtcode_core::subagents::BackgroundSubprocessStatus::Starting => {
                "Starting; waiting for subprocess output.".to_string()
            }
            vtcode_core::subagents::BackgroundSubprocessStatus::Running => {
                "Running; waiting for transcript output.".to_string()
            }
            vtcode_core::subagents::BackgroundSubprocessStatus::Stopped
            | vtcode_core::subagents::BackgroundSubprocessStatus::Error => {
                "No summary recorded".to_string()
            }
        })
}

fn background_subprocess_preview_placeholder(entry: &BackgroundSubprocessEntry) -> String {
    match entry.status {
        vtcode_core::subagents::BackgroundSubprocessStatus::Starting => {
            "Waiting for the subprocess to emit output...".to_string()
        }
        vtcode_core::subagents::BackgroundSubprocessStatus::Running => {
            "Subprocess is running; waiting for the next transcript update.".to_string()
        }
        vtcode_core::subagents::BackgroundSubprocessStatus::Stopped
        | vtcode_core::subagents::BackgroundSubprocessStatus::Error => {
            "No recent output yet.".to_string()
        }
    }
}

#[cfg(test)]
pub(super) fn summarize_thread_event_preview(events: &[ThreadEvent]) -> String {
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

#[cfg(test)]
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

#[cfg(test)]
fn tool_status_label(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::InProgress => "running",
    }
}

#[cfg(test)]
fn command_status_label(status: CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::InProgress => "running",
    }
}

#[cfg(test)]
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

#[cfg(test)]
fn collapse_preview_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
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

fn active_agent_inspector_items(entry: &SubagentStatusEntry) -> Vec<InlineListItem> {
    let mut items = Vec::new();
    if entry.transcript_path.is_some() {
        items.push(InlineListItem {
            title: "Open transcript".to_string(),
            subtitle: Some("Open the archived child transcript in your editor".to_string()),
            badge: Some("Open".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{THREAD_TRANSCRIPT_PREFIX}{}",
                entry.id
            ))),
            search_value: Some("open transcript".to_string()),
        });
    }
    items.push(InlineListItem {
        title: "Cancel agent".to_string(),
        subtitle: Some("Stop this delegated agent and keep the main session active".to_string()),
        badge: Some("Ctrl+K".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{THREAD_CANCEL_PREFIX}{}",
            entry.id
        ))),
        search_value: Some("cancel active agent".to_string()),
    });
    items
}

fn background_subprocess_inspector_items(entry: &BackgroundSubprocessEntry) -> Vec<InlineListItem> {
    let mut items = Vec::new();
    if entry.transcript_path.is_some() {
        items.push(InlineListItem {
            title: "Open transcript".to_string(),
            subtitle: Some("Open the archived subprocess transcript".to_string()),
            badge: Some("Open".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{SUBPROCESS_TRANSCRIPT_PREFIX}{}",
                entry.id
            ))),
            search_value: Some("open subprocess transcript".to_string()),
        });
    }
    if entry.archive_path.is_some() {
        items.push(InlineListItem {
            title: "Open archive".to_string(),
            subtitle: Some("Open the persisted session archive".to_string()),
            badge: Some("Open".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{SUBPROCESS_ARCHIVE_PREFIX}{}",
                entry.id
            ))),
            search_value: Some("open subprocess archive".to_string()),
        });
    }
    items.push(InlineListItem {
        title: "Graceful stop".to_string(),
        subtitle: Some("Request a clean shutdown for this subprocess".to_string()),
        badge: Some("Ctrl+K".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{SUBPROCESS_STOP_PREFIX}{}",
            entry.id
        ))),
        search_value: Some("graceful stop subprocess".to_string()),
    });
    items.push(InlineListItem {
        title: "Force cancel".to_string(),
        subtitle: Some("Close the subprocess immediately and clean up".to_string()),
        badge: Some("Ctrl+X".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{SUBPROCESS_CANCEL_PREFIX}{}",
            entry.id
        ))),
        search_value: Some("force cancel subprocess".to_string()),
    });
    items
}

fn selection_config_action<'a>(
    selection: Option<&'a InlineListSelection>,
    prefix: &str,
) -> Option<&'a str> {
    match selection {
        Some(InlineListSelection::ConfigAction(action)) => action.strip_prefix(prefix),
        _ => None,
    }
}

fn selection_path(
    selection: Option<&InlineListSelection>,
    prefix: &str,
    path: &Option<PathBuf>,
) -> Option<String> {
    selection_config_action(selection, prefix)
        .and(path.as_ref())
        .map(|path| path.display().to_string())
}

async fn confirm_subagent_cancellation(
    ctx: &mut SlashCommandContext<'_>,
    name: &str,
) -> Result<bool> {
    confirm_list_action(
        ctx,
        "Close delegated agent",
        &format!("Close `{name}` and stay on the main VT Code session?"),
        "Close agent",
    )
    .await
}

async fn confirm_subprocess_action(
    ctx: &mut SlashCommandContext<'_>,
    name: &str,
    force: bool,
) -> Result<bool> {
    let (title, message, confirm_label) = subprocess_action_prompt(name, force);
    confirm_list_action(ctx, title, &message, confirm_label).await
}

#[cfg(test)]
pub(super) fn active_subagent_entries(
    entries: Vec<SubagentStatusEntry>,
) -> Vec<SubagentStatusEntry> {
    entries
        .into_iter()
        .filter(|entry| !entry.status.is_terminal())
        .collect()
}

pub(super) fn visible_subagent_entries(
    mut entries: Vec<SubagentStatusEntry>,
) -> Vec<SubagentStatusEntry> {
    entries.retain(|entry| entry.status != vtcode_core::subagents::SubagentStatus::Closed);
    entries.sort_by(|left, right| {
        left.status
            .is_terminal()
            .cmp(&right.status.is_terminal())
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| right.created_at.cmp(&left.created_at))
    });
    entries
}

pub(super) fn subprocess_action_prompt(
    name: &str,
    force: bool,
) -> (&'static str, String, &'static str) {
    if force {
        (
            "Force cancel subprocess",
            format!("Force cancel `{name}` immediately?"),
            "Force cancel",
        )
    } else {
        (
            "Graceful stop subprocess",
            format!("Request a graceful shutdown for `{name}`?"),
            "Graceful stop",
        )
    }
}

async fn confirm_list_action(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    message: &str,
    confirm_label: &str,
) -> Result<bool> {
    ctx.handle.show_list_modal(
        title.to_string(),
        vec![message.to_string()],
        vec![
            InlineListItem {
                title: confirm_label.to_string(),
                subtitle: Some("Proceed with the selected action".to_string()),
                badge: Some("Confirm".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "agents:confirm-action".to_string(),
                )),
                search_value: Some("confirm action".to_string()),
            },
            InlineListItem {
                title: "Cancel".to_string(),
                subtitle: Some("Keep the subprocess/session running".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "agents:cancel-action".to_string(),
                )),
                search_value: Some("cancel".to_string()),
            },
        ],
        Some(InlineListSelection::ConfigAction(
            "agents:cancel-action".to_string(),
        )),
        None,
    );
    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(false);
    };
    Ok(matches!(
        selection,
        InlineListSelection::ConfigAction(action) if action == "agents:confirm-action"
    ))
}

pub(super) fn render_background_setup_guidance(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    ctx.renderer.line(
        MessageStyle::Info,
        "Background subagents are opt-in. VT Code will not launch one until it is explicitly configured.",
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        r#"Add `[subagents.background] enabled = true` and `default_agent = "<agent-name>"`, then use `Ctrl+B` or `/subprocesses toggle`."#,
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        "Use `/agents` to browse available agent names. `/subprocesses` opens the Local Agents drawer.",
    )?;
    Ok(())
}

pub(super) fn render_active_agent_status_text(
    ctx: &mut SlashCommandContext<'_>,
    entry: &SubagentStatusEntry,
    snapshot: &vtcode_core::subagents::SubagentThreadSnapshot,
) -> Result<()> {
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "{} {} {}",
            entry.display_label,
            entry.status.as_str(),
            if entry.background {
                "(background)"
            } else {
                "(delegated)"
            }
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("Summary: {}", active_agent_summary(entry, snapshot)),
    )?;
    if let Some(path) = entry.transcript_path.as_ref() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("Transcript: {}", path.display()),
        )?;
    }
    Ok(())
}

pub(super) fn render_background_subprocess_status_text(
    ctx: &mut SlashCommandContext<'_>,
    snapshot: &vtcode_core::subagents::BackgroundSubprocessSnapshot,
) -> Result<()> {
    render_subprocess_status(ctx, &snapshot.entry)?;
    if let Some(path) = snapshot
        .entry
        .transcript_path
        .as_ref()
        .or(snapshot.entry.archive_path.as_ref())
    {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("Transcript: {}", path.display()),
        )?;
    }
    let preview = if snapshot.preview.trim().is_empty() {
        background_subprocess_preview_placeholder(&snapshot.entry)
    } else {
        snapshot.preview.clone()
    };
    ctx.renderer
        .line(MessageStyle::Output, &format!("Preview: {}", preview))?;
    Ok(())
}

pub(super) fn render_subprocess_status(
    ctx: &mut SlashCommandContext<'_>,
    entry: &BackgroundSubprocessEntry,
) -> Result<()> {
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "{} {} pid {}",
            entry.display_label,
            entry.status.as_str(),
            entry
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("Summary: {}", background_subprocess_summary(entry)),
    )?;
    if let Some(error) = entry.error.as_deref() {
        ctx.renderer
            .line(MessageStyle::Error, &format!("Error: {}", error))?;
    }
    Ok(())
}

pub(super) async fn handle_list_subprocesses_text(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer
            .line(MessageStyle::Info, SUBAGENT_CONTROLLER_INACTIVE_MESSAGE)?;
        return Ok(SlashCommandControl::Continue);
    };

    let entries = controller.refresh_background_processes().await?;
    if entries.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No managed background subprocesses.")?;
        return Ok(SlashCommandControl::Continue);
    }

    for entry in entries {
        render_subprocess_status(ctx, &entry)?;
    }

    Ok(SlashCommandControl::Continue)
}

async fn launch_editor_path(
    ctx: &mut SlashCommandContext<'_>,
    path: String,
) -> Result<SlashCommandControl> {
    super::super::apps::launch_editor_from_context(ctx, Some(path)).await
}

fn format_datetime(timestamp: chrono::DateTime<chrono::Utc>) -> String {
    timestamp
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn format_optional_datetime(timestamp: Option<chrono::DateTime<chrono::Utc>>) -> String {
    timestamp
        .map(format_datetime)
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_uptime(started_at: chrono::DateTime<chrono::Utc>) -> String {
    let elapsed = chrono::Utc::now().signed_duration_since(started_at);
    let total_seconds = elapsed.num_seconds().max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub(super) async fn handle_list_threads_text(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer
            .line(MessageStyle::Info, SUBAGENT_CONTROLLER_INACTIVE_MESSAGE)?;
        return Ok(SlashCommandControl::Continue);
    };

    let threads = visible_subagent_entries(controller.status_entries().await);
    if threads.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("No delegated agents in main thread {}.", ctx.thread_id),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let active_count = threads
        .iter()
        .filter(|entry| !entry.status.is_terminal())
        .count();
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Delegated agents for main thread {} ({} active):",
            ctx.thread_id, active_count
        ),
    )?;
    for entry in threads {
        let summary = entry.summary.unwrap_or_default();
        let summary = summary.trim();
        let suffix = if summary.is_empty() {
            String::new()
        } else {
            format!(" - {}", summary)
        };
        ctx.renderer.line(
            MessageStyle::Output,
            &format!(
                "{} {} {}{}",
                entry.id,
                entry.agent_name,
                status_label(entry.status),
                suffix
            ),
        )?;
    }
    Ok(SlashCommandControl::Continue)
}
