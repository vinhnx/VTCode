use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use vtcode_core::llm::provider::MessageRole;

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::EditingMode as ConfigEditingMode;
use vtcode_core::config::PermissionMode;
use vtcode_core::core::agent::snapshots::{
    CheckpointRestore, RevertScope, SnapshotManager, SnapshotMetadata,
};
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::transcript;
use vtcode_tui::app::RewindAction;
use vtcode_tui::app::{InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection};

use crate::agent::runloop::unified::state::SessionStats;
use vtcode_core::hooks::SessionEndReason;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::settings_interactive::{
    create_settings_palette_state, show_settings_palette,
};
use crate::agent::runloop::unified::state::CtrlCSignal;
use crate::agent::runloop::unified::stop_requests::request_local_stop;
#[path = "activation.rs"]
mod activation;
#[path = "apps.rs"]
mod apps;
#[path = "diagnostics.rs"]
mod diagnostics;
#[path = "interactive.rs"]
mod interactive;
#[path = "mcp.rs"]
mod mcp;
#[path = "modes.rs"]
mod modes;
#[path = "oauth.rs"]
mod oauth;
#[path = "share_log.rs"]
mod share_log;
#[path = "skills.rs"]
mod skills;
#[path = "ui.rs"]
mod ui;
#[path = "update.rs"]
mod update;
#[path = "workspace.rs"]
mod workspace;
pub(super) use apps::{
    handle_launch_editor, handle_launch_git, handle_new_session, handle_open_docs,
};
pub(super) use diagnostics::{
    handle_run_doctor, handle_show_status, handle_start_doctor_interactive,
    handle_start_terminal_setup,
};
pub(super) use interactive::{
    handle_show_jobs_panel, handle_toggle_tasks_panel, handle_trigger_prompt_suggestions,
};
pub(super) use mcp::handle_manage_mcp;
pub(super) use modes::{
    handle_cycle_mode, handle_set_mode, handle_start_mode_selection, handle_toggle_plan_mode,
};
pub(super) use oauth::{
    handle_oauth_login, handle_oauth_logout, handle_refresh_oauth, handle_show_auth_status,
    handle_start_oauth_provider_picker,
};
pub(super) use share_log::handle_share_log;
pub(super) use skills::handle_manage_skills;
pub(super) use ui::{
    handle_start_file_browser, handle_start_history_picker, handle_start_model_selection,
    handle_start_session_palette, handle_start_statusline_setup, handle_start_theme_palette,
    handle_theme_changed, handle_toggle_ide_context, handle_toggle_vim_mode,
};
pub(super) use update::handle_update;
pub(super) use workspace::{handle_initialize_workspace, handle_manage_workspace_directories};

pub(super) fn persist_mode_settings(
    workspace: &std::path::Path,
    vt_cfg: &mut Option<VTCodeConfig>,
    editing_mode: Option<ConfigEditingMode>,
    permission_mode: Option<PermissionMode>,
) -> Result<()> {
    if editing_mode.is_none() && permission_mode.is_none() {
        return Ok(());
    }

    let mut manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "Failed to load configuration for workspace {}",
            workspace.display()
        )
    })?;
    let mut config = manager.config().clone();

    if let Some(mode) = editing_mode {
        config.agent.default_editing_mode = mode;
    }

    if let Some(mode) = permission_mode {
        config.permissions.default_mode = mode;
    }

    manager
        .save_config(&config)
        .context("Failed to persist mode settings")?;

    if let Some(cfg) = vt_cfg.as_mut() {
        if let Some(mode) = editing_mode {
            cfg.agent.default_editing_mode = mode;
        }
        if let Some(mode) = permission_mode {
            cfg.permissions.default_mode = mode;
        }
    }

    Ok(())
}

pub(super) async fn handle_show_settings(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    handle_show_settings_at_path(ctx, None).await
}

pub(super) async fn handle_show_permissions(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    handle_show_settings_at_path(ctx, Some("permissions")).await
}

async fn handle_show_settings_at_path(
    mut ctx: SlashCommandContext<'_>,
    view_path: Option<&str>,
) -> Result<SlashCommandControl> {
    if !ui::ensure_selection_ui_available(&mut ctx, "configuring settings")? {
        return Ok(SlashCommandControl::Continue);
    }

    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Interactive settings require inline UI; use /config to inspect effective values.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let workspace_path = ctx.config.workspace.clone();
    let vt_snapshot = ctx.vt_cfg.clone();
    let mut settings_state = create_settings_palette_state(&workspace_path, &vt_snapshot)?;
    settings_state.view_path = view_path.map(str::to_string);

    if show_settings_palette(ctx.renderer, &settings_state, None)? {
        *ctx.palette_state = Some(ActivePalette::Settings {
            state: Box::new(settings_state),
            esc_armed: false,
        });
    }

    Ok(SlashCommandControl::Continue)
}

pub(super) async fn handle_stop_agent(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if ctx.tool_registry.active_pty_sessions() == 0
        && !ctx.ctrl_c_state.is_cancel_requested()
        && !ctx.ctrl_c_state.is_exit_requested()
    {
        ctx.renderer
            .line(MessageStyle::Info, "No active run to stop.")?;
        return Ok(SlashCommandControl::Continue);
    }

    match request_local_stop(ctx.ctrl_c_state, ctx.ctrl_c_notify) {
        CtrlCSignal::Cancel => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Stop requested. VT Code is cancelling the current turn.",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        CtrlCSignal::Exit => Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit)),
    }
}

pub(super) async fn handle_clear_conversation(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let vim_mode_enabled = ctx.session_stats.vim_mode_enabled;
    ctx.conversation_history.clear();
    *ctx.session_stats = SessionStats::default();
    ctx.session_stats.vim_mode_enabled = vim_mode_enabled;
    ctx.handle.hide_task_panel();
    ctx.handle.update_task_panel(Vec::new());
    {
        let mut ledger = ctx.decision_ledger.write().await;
        *ledger = DecisionTracker::new();
    }
    transcript::clear();
    ctx.renderer.clear_screen();
    ctx.renderer
        .line(MessageStyle::Info, "Cleared conversation history.")?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(super) async fn handle_compact_conversation(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if ctx.conversation_history.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No conversation history to compact.")?;
        return Ok(SlashCommandControl::Continue);
    }

    let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
    let outcome =
        match crate::agent::runloop::unified::turn::compaction::compact_history_in_place_with_events(
            crate::agent::runloop::unified::turn::compaction::CompactionContext::new(
                ctx.provider_client.as_ref(),
                &ctx.config.model,
                &harness_snapshot.session_id,
                ctx.thread_id,
                &ctx.config.workspace,
                ctx.vt_cfg.as_ref(),
                ctx.lifecycle_hooks,
                ctx.harness_emitter,
            ),
            crate::agent::runloop::unified::turn::compaction::CompactionState::new(
                ctx.conversation_history,
                ctx.session_stats,
                ctx.context_manager,
            ),
            vtcode_core::exec::events::CompactionTrigger::Manual,
        )
        .await
        {
        Ok(outcome) => outcome,
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Compaction failed: {}", err))?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let Some(outcome) = outcome else {
        ctx.renderer
            .line(MessageStyle::Info, "Conversation is already compact.")?;
        return Ok(SlashCommandControl::Continue);
    };
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Compacted conversation history ({} -> {} messages).",
            outcome.original_len, outcome.compacted_len
        ),
    )?;
    Ok(SlashCommandControl::Continue)
}

pub(super) async fn handle_clear_screen(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.renderer.clear_screen();
    ctx.renderer.line(
        MessageStyle::Info,
        "Cleared screen. Conversation context is preserved.",
    )?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(super) async fn handle_copy_latest_assistant_reply(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let latest_reply = ctx.conversation_history.iter().rev().find_map(|message| {
        if message.role != MessageRole::Assistant {
            return None;
        }
        if message
            .tool_calls
            .as_ref()
            .is_some_and(|calls| !calls.is_empty())
        {
            return None;
        }
        let text = message.content.as_text();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    if let Some(reply) = latest_reply {
        vtcode_tui::core::MouseSelectionState::copy_to_clipboard(&reply);
        ctx.renderer.line(
            MessageStyle::Info,
            "Copied latest assistant reply to clipboard.",
        )?;
    } else {
        ctx.renderer.line(
            MessageStyle::Warning,
            "No complete assistant reply found to copy yet.",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

fn resolve_prompt_boundary_in_history(
    metadata: &SnapshotMetadata,
    history: &[uni::Message],
) -> Option<usize> {
    let prompt_text = metadata
        .prompt_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(index) = metadata
        .prompt_message_index
        .filter(|index| *index < history.len())
    {
        let message = &history[index];
        let matches_prompt =
            prompt_text.is_none_or(|text| message.content.as_text().trim() == text);
        if message.role == uni::MessageRole::User && matches_prompt {
            return Some(index);
        }
    }

    prompt_text.and_then(|prompt_text| {
        history
            .iter()
            .enumerate()
            .filter(|(_, message)| {
                message.role == uni::MessageRole::User
                    && message.content.as_text().trim() == prompt_text
            })
            .min_by_key(|(index, _)| {
                metadata
                    .prompt_message_index
                    .map_or(usize::MAX / 2, |target| target.abs_diff(*index))
            })
            .map(|(index, _)| index)
    })
}

fn rewind_checkpoint_title(metadata: &SnapshotMetadata) -> String {
    if metadata.description.trim().is_empty() {
        metadata
            .prompt_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("turn {}", metadata.turn_number))
    } else {
        metadata.description.clone()
    }
}

fn rewind_checkpoint_subtitle(metadata: &SnapshotMetadata) -> String {
    let created = DateTime::<Utc>::from_timestamp(metadata.created_at as i64, 0)
        .map(|dt| dt.with_timezone(&Local))
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| metadata.created_at.to_string());
    format!(
        "turn {}  {created}  {} msgs  {} files",
        metadata.turn_number, metadata.message_count, metadata.file_count
    )
}

fn show_rewind_checkpoint_modal(handle: &InlineHandle, snapshots: &[SnapshotMetadata]) {
    let items = snapshots
        .iter()
        .map(|snapshot| InlineListItem {
            title: rewind_checkpoint_title(snapshot),
            subtitle: Some(rewind_checkpoint_subtitle(snapshot)),
            badge: Some(format!("turn {}", snapshot.turn_number)),
            indent: 0,
            selection: Some(InlineListSelection::RewindCheckpoint(snapshot.turn_number)),
            search_value: Some(format!(
                "{} {} {}",
                snapshot.turn_number,
                snapshot.prompt_text.clone().unwrap_or_default(),
                snapshot.description
            )),
        })
        .collect();
    handle.show_list_modal(
        "Rewind".to_string(),
        vec![
            "Select a checkpoint prompt from this session.".to_string(),
            "Then choose whether to restore code, restore conversation, or summarize from that point.".to_string(),
        ],
        items,
        snapshots
            .first()
            .map(|snapshot| InlineListSelection::RewindCheckpoint(snapshot.turn_number)),
        Some(InlineListSearchConfig {
            label: "Checkpoint filter".to_string(),
            placeholder: Some("Search by prompt text or turn".to_string()),
        }),
    );
}

fn show_rewind_action_modal(handle: &InlineHandle, snapshot: &SnapshotMetadata) {
    let items = vec![
        InlineListItem {
            title: "Restore code and conversation".to_string(),
            subtitle: Some("Revert both files and conversation to this checkpoint.".to_string()),
            badge: Some("Both".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(RewindAction::RestoreBoth)),
            search_value: Some("restore both code conversation".to_string()),
        },
        InlineListItem {
            title: "Restore conversation".to_string(),
            subtitle: Some("Rewind the transcript but keep current files on disk.".to_string()),
            badge: Some("Chat".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(
                RewindAction::RestoreConversation,
            )),
            search_value: Some("restore conversation chat".to_string()),
        },
        InlineListItem {
            title: "Restore code".to_string(),
            subtitle: Some(
                "Revert tracked file edits but keep the current conversation.".to_string(),
            ),
            badge: Some("Code".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(RewindAction::RestoreCode)),
            search_value: Some("restore code files".to_string()),
        },
        InlineListItem {
            title: "Summarize from here".to_string(),
            subtitle: Some(
                "Compact the selected prompt onward without changing files.".to_string(),
            ),
            badge: Some("Summary".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(
                RewindAction::SummarizeFromHere,
            )),
            search_value: Some("summarize compact history".to_string()),
        },
        InlineListItem {
            title: "Never mind".to_string(),
            subtitle: Some("Close the rewind picker without changing anything.".to_string()),
            badge: Some("Cancel".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(RewindAction::NeverMind)),
            search_value: Some("cancel never mind".to_string()),
        },
    ];
    handle.show_list_modal(
        format!("Rewind turn {}", snapshot.turn_number),
        vec![
            rewind_checkpoint_title(snapshot),
            "Choose what to do with the selected checkpoint.".to_string(),
        ],
        items,
        Some(InlineListSelection::RewindAction(RewindAction::RestoreBoth)),
        None,
    );
}

fn restore_prompt_input(
    handle: &InlineHandle,
    metadata: &SnapshotMetadata,
    conversation: &[vtcode_core::utils::session_archive::SessionMessage],
) -> bool {
    let Some(prompt) = metadata.resolved_prompt_text(conversation) else {
        return false;
    };
    handle.set_input(prompt.to_string());
    handle.force_redraw();
    true
}

fn restore_prompt_input_and_report(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    metadata: &SnapshotMetadata,
    conversation: &[vtcode_core::utils::session_archive::SessionMessage],
) -> Result<()> {
    if restore_prompt_input(handle, metadata, conversation) {
        renderer.line(
            MessageStyle::Info,
            "Restored the selected prompt into the input field.",
        )?;
    }
    Ok(())
}

pub(super) async fn handle_open_rewind_picker(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Interactive rewind picker is available in inline UI only. Use `/rewind <turn> [conversation|code|both]`.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !ui::ensure_selection_ui_available(&mut ctx, "opening rewind picker")? {
        return Ok(SlashCommandControl::Continue);
    }

    let snapshots = match ctx.checkpoint_manager {
        Some(manager) => manager.list_snapshots().await,
        None => {
            ctx.renderer.line(
                MessageStyle::Info,
                "In-chat rewind requires access to the checkpoint manager.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let snapshots = match snapshots {
        Ok(snapshots) => snapshots,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to list checkpoints: {}", err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    if snapshots.is_empty() {
        ctx.renderer
            .line(MessageStyle::Warning, "No checkpoints available to rewind.")?;
        return Ok(SlashCommandControl::Continue);
    }

    show_rewind_checkpoint_modal(ctx.handle, &snapshots);
    let Some(selection) = ui::wait_for_list_modal_selection(&mut ctx).await else {
        ctx.renderer
            .line(MessageStyle::Info, "Rewind picker cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::RewindCheckpoint(turn) = selection else {
        ctx.renderer.line(
            MessageStyle::Error,
            "Unsupported rewind checkpoint selection.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };
    let Some(snapshot) = snapshots
        .iter()
        .find(|snapshot| snapshot.turn_number == turn)
    else {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Checkpoint turn {} is no longer available.", turn),
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    show_rewind_action_modal(ctx.handle, snapshot);
    let Some(selection) = ui::wait_for_list_modal_selection(&mut ctx).await else {
        ctx.renderer
            .line(MessageStyle::Info, "Rewind action cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::RewindAction(action) = selection else {
        ctx.renderer
            .line(MessageStyle::Error, "Unsupported rewind action selection.")?;
        return Ok(SlashCommandControl::Continue);
    };

    match action {
        RewindAction::RestoreBoth => handle_rewind_to_turn(ctx, turn, RevertScope::Both).await,
        RewindAction::RestoreConversation => {
            handle_rewind_to_turn(ctx, turn, RevertScope::Conversation).await
        }
        RewindAction::RestoreCode => handle_rewind_to_turn(ctx, turn, RevertScope::Code).await,
        RewindAction::SummarizeFromHere => summarize_rewind_from_checkpoint(ctx, turn).await,
        RewindAction::NeverMind => {
            ctx.renderer.line(MessageStyle::Info, "Rewind cancelled.")?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub(super) async fn handle_rewind_latest(
    ctx: SlashCommandContext<'_>,
    scope: RevertScope,
) -> Result<SlashCommandControl> {
    let Some(manager) = ctx.checkpoint_manager else {
        ctx.renderer.line(
            MessageStyle::Info,
            "In-chat rewind requires access to the checkpoint manager.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let snapshots = match manager.list_snapshots().await {
        Ok(snapshots) => snapshots,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to list checkpoints: {}", err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let Some(latest) = snapshots.first() else {
        ctx.renderer
            .line(MessageStyle::Warning, "No checkpoints available to rewind.")?;
        return Ok(SlashCommandControl::Continue);
    };

    handle_rewind_to_turn(ctx, latest.turn_number, scope).await
}

pub(super) async fn handle_rewind_to_turn(
    ctx: SlashCommandContext<'_>,
    turn: usize,
    scope: RevertScope,
) -> Result<SlashCommandControl> {
    if let Some(manager) = ctx.checkpoint_manager {
        restore_rewind_from_checkpoint(
            ctx.renderer,
            ctx.handle,
            ctx.conversation_history,
            manager,
            turn,
            scope,
        )
        .await?;
    } else {
        render_rewind_cli_guidance(ctx.renderer, turn, scope)?;
    }

    Ok(SlashCommandControl::Continue)
}

async fn restore_rewind_from_checkpoint(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    conversation_history: &mut Vec<uni::Message>,
    manager: &SnapshotManager,
    turn: usize,
    scope: RevertScope,
) -> Result<()> {
    match manager.restore_snapshot(turn, scope).await {
        Ok(Some(restored)) => render_rewind_restore_success(
            renderer,
            handle,
            conversation_history,
            turn,
            scope,
            restored,
        ),
        Ok(None) => renderer.line(
            MessageStyle::Error,
            &format!("No checkpoint found for turn {}", turn),
        ),
        Err(err) => renderer.line(
            MessageStyle::Error,
            &format!("Failed to restore checkpoint for turn {}: {}", turn, err),
        ),
    }
}

fn render_rewind_restore_success(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    conversation_history: &mut Vec<uni::Message>,
    turn: usize,
    scope: RevertScope,
    restored: CheckpointRestore,
) -> Result<()> {
    if scope.includes_conversation() {
        *conversation_history = restored
            .conversation
            .iter()
            .map(uni::Message::from)
            .collect();
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Restored conversation history from turn {} ({} messages)",
                turn,
                restored.conversation.len()
            ),
        )?;
        restore_prompt_input_and_report(
            renderer,
            handle,
            &restored.metadata,
            &restored.conversation,
        )?;
    }

    if scope.includes_code() {
        renderer.line(
            MessageStyle::Info,
            &format!("Applied code changes from turn {}", turn),
        )?;
    }

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Successfully rewound to turn {} with scope {:?}",
            turn, scope
        ),
    )?;
    Ok(())
}

async fn summarize_rewind_from_checkpoint(
    ctx: SlashCommandContext<'_>,
    turn: usize,
) -> Result<SlashCommandControl> {
    let Some(manager) = ctx.checkpoint_manager else {
        render_rewind_cli_guidance(ctx.renderer, turn, RevertScope::Conversation)?;
        return Ok(SlashCommandControl::Continue);
    };
    let restored = match manager.load_snapshot(turn).await {
        Ok(Some(restored)) => restored,
        Ok(None) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("No checkpoint found for turn {}", turn),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to load checkpoint for turn {}: {}", turn, err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let Some(start_index) =
        resolve_prompt_boundary_in_history(&restored.metadata, ctx.conversation_history.as_slice())
    else {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Could not locate the prompt boundary for turn {}.", turn),
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
    let outcome =
        crate::agent::runloop::unified::turn::compaction::compact_history_from_index_in_place(
            ctx.provider_client.as_ref(),
            &ctx.config.model,
            &harness_snapshot.session_id,
            &ctx.config.workspace,
            ctx.vt_cfg.as_ref(),
            ctx.conversation_history,
            start_index,
            ctx.session_stats,
            ctx.context_manager,
        )
        .await;

    match outcome {
        Ok(Some(result)) => {
            restore_prompt_input_and_report(
                ctx.renderer,
                ctx.handle,
                &restored.metadata,
                &restored.conversation,
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Summarized conversation from turn {} ({} -> {} messages).",
                    turn, result.original_len, result.compacted_len
                ),
            )?;
            ctx.renderer
                .line(MessageStyle::Info, "Files on disk were left unchanged.")?;
        }
        Ok(None) => {
            restore_prompt_input_and_report(
                ctx.renderer,
                ctx.handle,
                &restored.metadata,
                &restored.conversation,
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Conversation is already compact from that point.",
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to summarize from turn {}: {}", turn, err),
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

fn render_rewind_cli_guidance(
    renderer: &mut AnsiRenderer,
    turn: usize,
    scope: RevertScope,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!("Rewinding to turn {} with scope {:?}...", turn, scope),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Use: `vtcode revert --turn {} --partial {}` from command line",
            turn,
            rewind_partial_arg(scope)
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        "Note: In-chat rewind requires access to the checkpoint manager.",
    )?;
    Ok(())
}

fn rewind_partial_arg(scope: RevertScope) -> &'static str {
    match scope {
        RevertScope::Conversation => "conversation",
        RevertScope::Code => "code",
        RevertScope::Both => "both",
    }
}

pub(super) async fn handle_exit(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer.line(MessageStyle::Info, "✓")?;
    Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
}

#[cfg(test)]
mod tests {
    use super::{persist_mode_settings, resolve_prompt_boundary_in_history, rewind_partial_arg};
    use tempfile::TempDir;
    use vtcode_core::config::PermissionMode;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::core::agent::snapshots::{RevertScope, SnapshotMetadata};

    #[test]
    fn rewind_partial_arg_matches_cli_scope_values() {
        assert_eq!(
            rewind_partial_arg(RevertScope::Conversation),
            "conversation"
        );
        assert_eq!(rewind_partial_arg(RevertScope::Code), "code");
        assert_eq!(rewind_partial_arg(RevertScope::Both), "both");
    }

    #[test]
    fn resolve_prompt_boundary_prefers_metadata_index_when_it_matches() {
        let history = vec![
            vtcode_core::llm::provider::Message::user("first".to_string()),
            vtcode_core::llm::provider::Message::assistant("reply".to_string()),
            vtcode_core::llm::provider::Message::user("target".to_string()),
        ];
        let metadata = SnapshotMetadata {
            id: "turn_2".to_string(),
            turn_number: 2,
            created_at: 0,
            description: "target".to_string(),
            message_count: 3,
            file_count: 0,
            prompt_text: Some("target".to_string()),
            prompt_message_index: Some(2),
        };

        assert_eq!(
            resolve_prompt_boundary_in_history(&metadata, &history),
            Some(2)
        );
    }

    #[test]
    fn resolve_prompt_boundary_falls_back_to_nearest_prompt_match() {
        let history = vec![
            vtcode_core::llm::provider::Message::user("target".to_string()),
            vtcode_core::llm::provider::Message::assistant("reply".to_string()),
            vtcode_core::llm::provider::Message::user("target".to_string()),
        ];
        let metadata = SnapshotMetadata {
            id: "turn_2".to_string(),
            turn_number: 2,
            created_at: 0,
            description: "target".to_string(),
            message_count: 3,
            file_count: 0,
            prompt_text: Some("target".to_string()),
            prompt_message_index: Some(2),
        };

        assert_eq!(
            resolve_prompt_boundary_in_history(&metadata, &history),
            Some(2)
        );
    }

    #[test]
    fn persist_mode_settings_updates_only_permissions_default_mode() {
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path();
        let initial = VTCodeConfig::default();
        std::fs::write(
            workspace.join("vtcode.toml"),
            toml::to_string(&initial).expect("serialize config"),
        )
        .expect("write config");

        let mut vt_cfg = Some(initial.clone());
        persist_mode_settings(workspace, &mut vt_cfg, None, Some(PermissionMode::Auto))
            .expect("persist mode settings");

        let persisted = std::fs::read_to_string(workspace.join("vtcode.toml")).expect("config");
        assert!(persisted.contains("default_mode = \"auto\""));
        assert!(!persisted.contains("autonomous_mode = true"));
        assert!(
            vt_cfg.is_some_and(|cfg| {
                cfg.permissions.default_mode == PermissionMode::Auto && !cfg.agent.autonomous_mode
            })
        );
    }
}
