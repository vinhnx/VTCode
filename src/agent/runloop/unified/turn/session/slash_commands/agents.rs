use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;
use std::time::Duration;
use vtcode_core::constants::tools;
use vtcode_core::subagents::{BackgroundSubprocessEntry, SubagentStatusEntry};
use vtcode_core::utils::ansi::MessageStyle;
#[cfg(test)]
use vtcode_core::{CommandExecutionStatus, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use vtcode_tui::app::{
    AgentPaletteItem, InlineListItem, InlineListSearchConfig, InlineListSelection,
    ListOverlayRequest, TransientEvent, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey,
    TransientRequest, TransientSelectionChange, TransientSubmission, WizardModalMode, WizardStep,
};

use super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::{
    AgentDefinitionScope, AgentManagerAction, SubprocessManagerAction,
};
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

const AGENT_ACTION_PREFIX: &str = "agents:";
const AGENT_INSPECT_PREFIX: &str = "agents:inspect:";
const THREAD_INSPECT_PREFIX: &str = "agents:thread:";
const THREAD_TRANSCRIPT_PREFIX: &str = "agents:transcript:";
const THREAD_CANCEL_PREFIX: &str = "agents:cancel:";
const SUBPROCESS_TRANSCRIPT_PREFIX: &str = "subprocesses:transcript:";
const SUBPROCESS_ARCHIVE_PREFIX: &str = "subprocesses:archive:";
const SUBPROCESS_STOP_PREFIX: &str = "subprocesses:stop:";
const SUBPROCESS_CANCEL_PREFIX: &str = "subprocesses:cancel:";
const PROMPT_QUESTION_ID: &str = "agent-name";
const ACTIVE_AGENT_INSPECTOR_REFRESH_MS: u64 = 750;

pub(crate) async fn handle_manage_agents(
    mut ctx: SlashCommandContext<'_>,
    action: AgentManagerAction,
) -> Result<SlashCommandControl> {
    match action {
        AgentManagerAction::List => {
            if ctx.renderer.supports_inline_ui() {
                let mut ctx = ctx;
                if !ensure_selection_ui_available(&mut ctx, "opening subagent manager")? {
                    return Ok(SlashCommandControl::Continue);
                }
                show_agents_manager(ctx).await
            } else {
                let mut ctx = ctx;
                handle_list_agents_text(&mut ctx).await
            }
        }
        AgentManagerAction::Threads => {
            if ctx.renderer.supports_inline_ui() {
                let mut ctx = ctx;
                if !ensure_selection_ui_available(&mut ctx, "browsing delegated child threads")? {
                    return Ok(SlashCommandControl::Continue);
                }
                show_threads_modal(ctx).await
            } else {
                let mut ctx = ctx;
                handle_list_threads_text(&mut ctx).await
            }
        }
        AgentManagerAction::Create { scope, name } => {
            let mut ctx = ctx;
            handle_create_agent(&mut ctx, scope, &name).await
        }
        AgentManagerAction::Inspect { id } => {
            let Some(controller) = ctx.tool_registry.subagent_controller() else {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Subagent controller is not active in this session.",
                )?;
                return Ok(SlashCommandControl::Continue);
            };
            let entry = controller.status_for(&id).await?;
            if ctx.renderer.supports_inline_ui() {
                let mut ctx = ctx;
                show_active_agent_inspector(&mut ctx, entry).await
            } else {
                let snapshot = controller.snapshot_for_thread(&id).await?;
                render_active_agent_status_text(&mut ctx, &entry, &snapshot)?;
                Ok(SlashCommandControl::Continue)
            }
        }
        AgentManagerAction::Close { id } => {
            let Some(controller) = ctx.tool_registry.subagent_controller() else {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Subagent controller is not active in this session.",
                )?;
                return Ok(SlashCommandControl::Continue);
            };
            let entry = controller.close(&id).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Closed delegated agent {}.", entry.display_label),
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentManagerAction::Edit { name } => handle_edit_agent(ctx, &name).await,
        AgentManagerAction::Delete { name } => {
            let mut ctx = ctx;
            handle_delete_agent(&mut ctx, &name).await
        }
    }
}

pub(crate) async fn handle_manage_subprocesses(
    mut ctx: SlashCommandContext<'_>,
    action: SubprocessManagerAction,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    match action {
        SubprocessManagerAction::ToggleDefault => {
            if !controller.background_subagents_enabled()
                || controller.configured_default_background_agent().is_none()
            {
                if ctx.renderer.supports_inline_ui() {
                    ctx.handle.show_local_agents();
                }
                render_background_setup_guidance(&mut ctx)?;
                return Ok(SlashCommandControl::Continue);
            }
            let entry = controller.toggle_default_background_subagent().await?;
            render_subprocess_status(&mut ctx, &entry)?;
            Ok(SlashCommandControl::Continue)
        }
        SubprocessManagerAction::Refresh => {
            let entries = controller.refresh_background_processes().await?;
            if entries.is_empty() {
                ctx.renderer
                    .line(MessageStyle::Info, "No managed background subprocesses.")?;
            } else {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Refreshed {} background subprocesses.", entries.len()),
                )?;
            }
            Ok(SlashCommandControl::Continue)
        }
        SubprocessManagerAction::List => {
            if ctx.renderer.supports_inline_ui() {
                ctx.handle.show_local_agents();
                return Ok(SlashCommandControl::Continue);
            }
            handle_list_subprocesses_text(&mut ctx).await
        }
        SubprocessManagerAction::Inspect { id } => {
            let entry = controller.background_snapshot(&id).await?;
            if ctx.renderer.supports_inline_ui() {
                show_background_subprocess_inspector(&mut ctx, entry.entry).await
            } else {
                render_background_subprocess_status_text(&mut ctx, &entry)?;
                Ok(SlashCommandControl::Continue)
            }
        }
        SubprocessManagerAction::Stop { id } => {
            let entry = controller.graceful_stop_background(&id).await?;
            render_subprocess_status(&mut ctx, &entry)?;
            Ok(SlashCommandControl::Continue)
        }
        SubprocessManagerAction::Cancel { id } => {
            let entry = controller.force_cancel_background(&id).await?;
            render_subprocess_status(&mut ctx, &entry)?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

async fn show_agents_manager(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.handle.show_list_modal(
        "Subagents".to_string(),
        vec![
            "Manage effective subagents, active agents, and custom definitions.".to_string(),
            "Use Enter to inspect, create, edit, or delete definitions.".to_string(),
        ],
        vec![
            action_item(
                "Browse agents",
                "List effective and shadowed definitions with source badges",
                Some("Recommended"),
                "browse effective shadowed agents",
                "browse",
            ),
            action_item(
                "Browse active agents",
                "Inspect delegated runs without switching the main session",
                None,
                "active agents delegated inspector",
                "threads",
            ),
            action_item(
                "Create project agent",
                "Scaffold `.vtcode/agents/<name>.md` with VT Code-native frontmatter",
                Some("Project"),
                "create project agent scaffold",
                "create-project",
            ),
            action_item(
                "Create user agent",
                "Scaffold `~/.vtcode/agents/<name>.md` with VT Code-native frontmatter",
                Some("User"),
                "create user agent scaffold",
                "create-user",
            ),
            action_item(
                "Edit custom agent",
                "Pick a project or user agent file and open it in your editor",
                None,
                "edit custom agent file",
                "edit",
            ),
            action_item(
                "Delete custom agent",
                "Pick a project or user agent file and remove it",
                None,
                "delete custom agent file",
                "delete",
            ),
        ],
        Some(InlineListSelection::ConfigAction(format!(
            "{AGENT_ACTION_PREFIX}browse"
        ))),
        Some(InlineListSearchConfig {
            label: "Search subagent actions".to_string(),
            placeholder: Some("browse, create, edit, thread".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };

    match action.as_str() {
        value if value == format!("{AGENT_ACTION_PREFIX}browse") => show_agent_catalog(ctx).await,
        value if value == format!("{AGENT_ACTION_PREFIX}threads") => show_threads_modal(ctx).await,
        value if value == format!("{AGENT_ACTION_PREFIX}create-project") => {
            let name = prompt_agent_name(&mut ctx, "Create project agent", "Agent name").await?;
            if let Some(name) = name {
                handle_create_agent(&mut ctx, AgentDefinitionScope::Project, &name).await
            } else {
                Ok(SlashCommandControl::Continue)
            }
        }
        value if value == format!("{AGENT_ACTION_PREFIX}create-user") => {
            let name = prompt_agent_name(&mut ctx, "Create user agent", "Agent name").await?;
            if let Some(name) = name {
                handle_create_agent(&mut ctx, AgentDefinitionScope::User, &name).await
            } else {
                Ok(SlashCommandControl::Continue)
            }
        }
        value if value == format!("{AGENT_ACTION_PREFIX}edit") => {
            let Some(name) = select_custom_agent_name(&mut ctx, "Edit custom agent").await? else {
                return Ok(SlashCommandControl::Continue);
            };
            handle_edit_agent(ctx, &name).await
        }
        value if value == format!("{AGENT_ACTION_PREFIX}delete") => {
            let Some(name) = select_custom_agent_name(&mut ctx, "Delete custom agent").await?
            else {
                return Ok(SlashCommandControl::Continue);
            };
            if confirm_delete_agent(&mut ctx, &name).await? {
                handle_delete_agent(&mut ctx, &name).await
            } else {
                Ok(SlashCommandControl::Continue)
            }
        }
        _ => Ok(SlashCommandControl::Continue),
    }
}

async fn show_agent_catalog(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let specs = controller.effective_specs().await;
    let shadowed = controller.shadowed_specs().await;
    if specs.is_empty() && shadowed.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "No subagent definitions are currently loaded.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let mut items = Vec::new();
    for spec in &specs {
        items.push(InlineListItem {
            title: spec.name.clone(),
            subtitle: Some(agent_subtitle(spec, false)),
            badge: Some(agent_badge(spec)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{AGENT_INSPECT_PREFIX}{}",
                spec.name
            ))),
            search_value: Some(format!(
                "{} {} {}",
                spec.name,
                spec.description,
                spec.source.label()
            )),
        });
    }
    for spec in &shadowed {
        items.push(InlineListItem {
            title: format!("{} (shadowed)", spec.name),
            subtitle: Some(agent_subtitle(spec, true)),
            badge: Some("Shadowed".to_string()),
            indent: 0,
            selection: None,
            search_value: Some(format!(
                "{} shadowed {} {}",
                spec.name,
                spec.description,
                spec.source.label()
            )),
        });
    }

    let selected = items.iter().find_map(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        "Loaded subagents".to_string(),
        vec![
            format!(
                "{} effective definition(s), {} shadowed definition(s).",
                specs.len(),
                shadowed.len()
            ),
            "Select an effective definition to inspect details.".to_string(),
        ],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search subagents".to_string(),
            placeholder: Some("name, source, description".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };
    let Some(name) = action.strip_prefix(AGENT_INSPECT_PREFIX) else {
        return Ok(SlashCommandControl::Continue);
    };
    let spec = specs
        .into_iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| anyhow!("Unknown agent {}", name))?;
    render_agent_details(
        &mut ctx,
        &spec,
        shadowed.iter().filter(|entry| entry.name == name).count(),
    )?;
    Ok(SlashCommandControl::Continue)
}

async fn show_threads_modal(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
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

async fn show_active_agent_inspector(
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

async fn show_background_subprocess_inspector(
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
                    render_subprocess_status(ctx, &updated)?;
                }
                return Ok(SlashCommandControl::Continue);
            }
            InspectorActionKind::ForceCancel => {
                if confirm_subprocess_action(ctx, current_entry.display_label.as_str(), true)
                    .await?
                {
                    let updated = controller.force_cancel_background(&record_id).await?;
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
                    render_subprocess_status(ctx, &updated)?;
                }
                if selection_config_action(action.selection.as_ref(), SUBPROCESS_CANCEL_PREFIX)
                    .is_some()
                    && confirm_subprocess_action(ctx, current_entry.display_label.as_str(), true)
                        .await?
                {
                    let updated = controller.force_cancel_background(&record_id).await?;
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

fn background_subprocess_summary(entry: &BackgroundSubprocessEntry) -> String {
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
fn active_subagent_entries(entries: Vec<SubagentStatusEntry>) -> Vec<SubagentStatusEntry> {
    entries
        .into_iter()
        .filter(|entry| !entry.status.is_terminal())
        .collect()
}

fn visible_subagent_entries(mut entries: Vec<SubagentStatusEntry>) -> Vec<SubagentStatusEntry> {
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

fn subprocess_action_prompt(name: &str, force: bool) -> (&'static str, String, &'static str) {
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

fn render_background_setup_guidance(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
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

fn render_active_agent_status_text(
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

fn render_background_subprocess_status_text(
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

fn render_subprocess_status(
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

async fn handle_list_subprocesses_text(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
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
    use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer.line(
            MessageStyle::Warning,
            "External editor is disabled (`tools.editor.enabled = false`).",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let launch_config = EditorLaunchConfig {
        preferred_editor: if editor_config.preferred_editor.trim().is_empty() {
            None
        } else {
            Some(editor_config.preferred_editor.clone())
        },
    };

    match launcher.launch_editor_with_config(Some(PathBuf::from(path)), launch_config) {
        Ok(_) => {
            ctx.renderer.line(MessageStyle::Info, "Editor closed.")?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to launch editor: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
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

async fn handle_list_agents_text(ctx: &mut SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let specs = controller.effective_specs().await;
    let shadowed = controller.shadowed_specs().await;
    let threads = controller.status_entries().await;

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Loaded {} effective subagents ({} shadowed definitions).",
            specs.len(),
            shadowed.len()
        ),
    )?;
    for spec in specs {
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("{} {}", spec.name, agent_subtitle(&spec, false)),
        )?;
    }

    if threads.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No delegated child threads yet.")?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{} delegated child thread(s):", threads.len()),
        )?;
        for entry in threads {
            ctx.renderer.line(
                MessageStyle::Output,
                &format!(
                    "{} {} {}",
                    entry.id,
                    entry.agent_name,
                    status_label(entry.status)
                ),
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

async fn handle_list_threads_text(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
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

async fn handle_create_agent(
    ctx: &mut SlashCommandContext<'_>,
    scope: AgentDefinitionScope,
    name: &str,
) -> Result<SlashCommandControl> {
    validate_agent_name(name)?;
    let path = match scope {
        AgentDefinitionScope::Project => ctx
            .config
            .workspace
            .join(".vtcode/agents")
            .join(format!("{name}.md")),
        AgentDefinitionScope::User => dirs::home_dir()
            .ok_or_else(|| anyhow!("Cannot resolve home directory for user-scope agent"))?
            .join(".vtcode/agents")
            .join(format!("{name}.md")),
    };

    if path.exists() {
        bail!("Agent file already exists at {}", path.display());
    }

    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow!("Invalid agent destination {}", path.display()))?,
    )?;
    std::fs::write(&path, scaffold_agent_markdown(name))?;

    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
        refresh_agent_palette(ctx.handle, controller.as_ref()).await;
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Created agent scaffold at {} with VT Code-native subagent frontmatter.",
            path.display()
        ),
    )?;
    Ok(SlashCommandControl::Continue)
}

async fn handle_edit_agent(
    ctx: SlashCommandContext<'_>,
    name: &str,
) -> Result<SlashCommandControl> {
    let path = resolve_custom_agent_path(&ctx, name).await?;
    super::apps::handle_launch_editor(ctx, Some(path.display().to_string())).await
}

async fn handle_delete_agent(
    ctx: &mut SlashCommandContext<'_>,
    name: &str,
) -> Result<SlashCommandControl> {
    let path = resolve_custom_agent_path(ctx, name).await?;
    std::fs::remove_file(&path)?;
    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
        refresh_agent_palette(ctx.handle, controller.as_ref()).await;
    }
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Deleted agent definition {}.", path.display()),
    )?;
    Ok(SlashCommandControl::Continue)
}

async fn select_custom_agent_name(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
) -> Result<Option<String>> {
    let controller = ctx
        .tool_registry
        .subagent_controller()
        .ok_or_else(|| anyhow!("Subagent controller is not active in this session"))?;
    let specs = controller
        .effective_specs()
        .await
        .into_iter()
        .filter(|spec| spec.file_path.is_some())
        .collect::<Vec<_>>();
    if specs.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "No editable custom agents are currently loaded.",
        )?;
        return Ok(None);
    }

    let items = specs
        .iter()
        .map(|spec| InlineListItem {
            title: spec.name.clone(),
            subtitle: Some(agent_subtitle(spec, false)),
            badge: Some(agent_badge(spec)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{AGENT_INSPECT_PREFIX}{}",
                spec.name
            ))),
            search_value: Some(format!(
                "{} {} {}",
                spec.name,
                spec.description,
                spec.source.label()
            )),
        })
        .collect::<Vec<_>>();
    let selected = items.first().and_then(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        title.to_string(),
        vec!["Select a project or user-scope agent definition.".to_string()],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search custom agents".to_string(),
            placeholder: Some("name, description, source".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };
    Ok(action
        .strip_prefix(AGENT_INSPECT_PREFIX)
        .map(ToString::to_string))
}

async fn prompt_agent_name(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    freeform_label: &str,
) -> Result<Option<String>> {
    let step = WizardStep {
        title: "Name".to_string(),
        question: "Enter a lowercase hyphenated agent name.".to_string(),
        items: vec![InlineListItem {
            title: "Save".to_string(),
            subtitle: Some(
                "Press Tab to type the agent name, then Enter to scaffold it.".to_string(),
            ),
            badge: Some("Required".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: PROMPT_QUESTION_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("save agent name".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some("example-agent".to_string()),
    };

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        title.to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;
    let Some(value) = (match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == PROMPT_QUESTION_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    }) else {
        return Ok(None);
    };

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    validate_agent_name(&trimmed)?;
    Ok(Some(trimmed))
}

async fn confirm_delete_agent(ctx: &mut SlashCommandContext<'_>, name: &str) -> Result<bool> {
    ctx.handle.show_list_modal(
        "Delete custom agent".to_string(),
        vec![format!(
            "Delete `{name}` from disk? This cannot be undone automatically."
        )],
        vec![
            InlineListItem {
                title: "Delete agent".to_string(),
                subtitle: Some("Remove the selected definition file".to_string()),
                badge: Some("Confirm".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "agents:confirm-delete".to_string(),
                )),
                search_value: Some("confirm delete".to_string()),
            },
            InlineListItem {
                title: "Cancel".to_string(),
                subtitle: Some("Keep the agent definition".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "agents:cancel-delete".to_string(),
                )),
                search_value: Some("cancel".to_string()),
            },
        ],
        Some(InlineListSelection::ConfigAction(
            "agents:cancel-delete".to_string(),
        )),
        None,
    );
    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(false);
    };
    Ok(matches!(
        selection,
        InlineListSelection::ConfigAction(action) if action == "agents:confirm-delete"
    ))
}

async fn resolve_custom_agent_path(ctx: &SlashCommandContext<'_>, name: &str) -> Result<PathBuf> {
    let controller = ctx
        .tool_registry
        .subagent_controller()
        .ok_or_else(|| anyhow!("Subagent controller is not active in this session"))?;
    let spec = controller
        .effective_specs()
        .await
        .into_iter()
        .find(|spec| spec.matches_name(name))
        .ok_or_else(|| anyhow!("Unknown agent {}", name))?;
    let path = spec.file_path.ok_or_else(|| {
        anyhow!(
            "Agent {} is built-in or plugin-provided and cannot be edited here",
            name
        )
    })?;
    Ok(path)
}

fn render_agent_details(
    ctx: &mut SlashCommandContext<'_>,
    spec: &vtcode_config::SubagentSpec,
    shadowed_count: usize,
) -> Result<()> {
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("{} [{}]", spec.name, spec.source.label()),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("Description: {}", spec.description),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "Mode: {}",
            if spec.is_read_only() {
                "read-only"
            } else {
                "write-capable"
            }
        ),
    )?;
    if let Some(path) = spec.file_path.as_ref() {
        ctx.renderer
            .line(MessageStyle::Output, &format!("File: {}", path.display()))?;
    }
    if shadowed_count > 0 {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{shadowed_count} lower-priority definition(s) are shadowed."),
        )?;
    }
    for warning in &spec.warnings {
        ctx.renderer
            .line(MessageStyle::Warning, &format!("Warning: {}", warning))?;
    }
    Ok(())
}

fn validate_agent_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("Agent name cannot be empty");
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        bail!("Agent name must use lowercase letters, digits, or hyphens");
    }
    Ok(())
}

fn scaffold_agent_markdown(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Describe when VT Code should delegate to this agent.\ntools:\n  - {read_file}\n  - {list_files}\n  - {unified_search}\nmodel: inherit\ncolor: blue\nreasoning_effort: medium\n---\n\nYou are a focused VT Code subagent.\n\nScope:\n- Describe the tasks this agent should handle.\n- Keep behavior narrow and task-specific.\n\nConstraints:\n- Use VT Code tool ids in frontmatter such as `read_file`, `list_files`, `unified_search`, and `unified_exec`.\n- Prefer the narrowest tool set that fits the job.\n- Return concise, actionable results.\n\nOutput:\n- State what you checked.\n- Summarize findings or changes.\n- Call out verification or remaining risks when relevant.\n",
        read_file = tools::READ_FILE,
        list_files = tools::LIST_FILES,
        unified_search = tools::UNIFIED_SEARCH,
    )
}

fn action_item(
    title: &str,
    subtitle: &str,
    badge: Option<&str>,
    search_value: &str,
    action: &str,
) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: badge.map(ToString::to_string),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{AGENT_ACTION_PREFIX}{action}"
        ))),
        search_value: Some(search_value.to_string()),
    }
}

fn agent_badge(spec: &vtcode_config::SubagentSpec) -> String {
    match spec.file_path {
        Some(_) => spec.source.label().to_string(),
        None => "Built-in".to_string(),
    }
}

fn agent_subtitle(spec: &vtcode_config::SubagentSpec, shadowed: bool) -> String {
    let mut parts = vec![spec.source.label().to_string(), spec.description.clone()];
    if spec.is_read_only() {
        parts.push("read-only".to_string());
    }
    if shadowed {
        parts.push("shadowed".to_string());
    }
    parts.join(" | ")
}

fn status_label(status: vtcode_core::subagents::SubagentStatus) -> &'static str {
    match status {
        vtcode_core::subagents::SubagentStatus::Queued => "[queued]",
        vtcode_core::subagents::SubagentStatus::Running => "[running]",
        vtcode_core::subagents::SubagentStatus::Waiting => "[waiting]",
        vtcode_core::subagents::SubagentStatus::Completed => "[completed]",
        vtcode_core::subagents::SubagentStatus::Failed => "[failed]",
        vtcode_core::subagents::SubagentStatus::Closed => "[closed]",
    }
}

async fn refresh_agent_palette(
    handle: &vtcode_tui::app::InlineHandle,
    controller: &vtcode_core::subagents::SubagentController,
) {
    let specs = controller.effective_specs().await;
    handle.configure_agent_palette(
        specs
            .into_iter()
            .map(|spec| AgentPaletteItem {
                name: spec.name,
                description: Some(spec.description),
            })
            .collect(),
    );
}

#[cfg(test)]
mod tests {
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
        ItemStartedEvent, ItemUpdatedEvent, ReasoningItem, ThreadEvent, ThreadItem,
        ThreadItemDetails, ToolCallStatus, ToolOutputItem,
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
}
