use anyhow::{Context, Result};
use serde_json;
use vtcode_core::llm::provider::MessageRole;

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::EditingMode as ConfigEditingMode;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::transcript;

use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::hooks::lifecycle::SessionEndReason;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::palettes::{ActivePalette, show_config_palette};
use crate::agent::runloop::unified::turn::config_modal::load_config_modal_content;
#[path = "apps.rs"]
mod apps;
#[path = "activation.rs"]
mod activation;
#[path = "diagnostics.rs"]
mod diagnostics;
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
pub use apps::{handle_launch_editor, handle_launch_git, handle_new_session, handle_open_docs};
pub use diagnostics::{handle_run_doctor, handle_show_status, handle_start_terminal_setup};
pub use mcp::handle_manage_mcp;
pub use modes::{handle_cycle_mode, handle_toggle_autonomous_mode, handle_toggle_plan_mode};
pub use oauth::{handle_oauth_login, handle_oauth_logout, handle_show_auth_status};
pub use share_log::handle_share_log;
pub use skills::handle_manage_skills;
pub use ui::{
    handle_start_file_browser, handle_start_model_selection, handle_start_resume_palette,
    handle_start_theme_palette, handle_theme_changed,
};
pub use update::handle_update;
pub use workspace::{
    handle_generate_agent_file, handle_initialize_workspace, handle_manage_workspace_directories,
};

pub(super) fn persist_mode_settings(
    workspace: &std::path::Path,
    vt_cfg: &mut Option<VTCodeConfig>,
    editing_mode: Option<ConfigEditingMode>,
    autonomous_mode: Option<bool>,
) -> Result<()> {
    if editing_mode.is_none() && autonomous_mode.is_none() {
        return Ok(());
    }

    let mut manager = ConfigManager::load().with_context(|| {
        format!(
            "Failed to load configuration for workspace {}",
            workspace.display()
        )
    })?;
    let mut config = manager.config().clone();

    if let Some(mode) = editing_mode {
        config.agent.default_editing_mode = mode;
    }

    if let Some(enabled) = autonomous_mode {
        config.agent.autonomous_mode = enabled;
    }

    manager
        .save_config(&config)
        .context("Failed to persist mode settings")?;

    if let Some(cfg) = vt_cfg.as_mut() {
        if let Some(mode) = editing_mode {
            cfg.agent.default_editing_mode = mode;
        }
        if let Some(enabled) = autonomous_mode {
            cfg.agent.autonomous_mode = enabled;
        }
    }

    Ok(())
}

pub async fn handle_show_config(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if ctx.renderer.supports_inline_ui() {
        if ctx.model_picker_state.is_some() {
            ctx.renderer.line(
                MessageStyle::Error,
                "Close the active model picker before viewing configuration.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }
        if ctx.palette_state.is_some() {
            ctx.renderer.line(
                MessageStyle::Error,
                "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }

        let workspace_path = ctx.config.workspace.clone();
        let vt_snapshot = ctx.vt_cfg.clone();
        if show_config_palette(ctx.renderer, &workspace_path, &vt_snapshot, None)? {
            *ctx.palette_state = Some(ActivePalette::Config {
                workspace: workspace_path,
                vt_snapshot: Box::new(vt_snapshot),
                selected: None,
            });
        }

        return Ok(SlashCommandControl::Continue);
    }

    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before viewing configuration.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let workspace_path = ctx.config.workspace.clone();
    let vt_snapshot = ctx.vt_cfg.clone();
    match load_config_modal_content(workspace_path, vt_snapshot).await {
        Ok(content) => {
            ctx.renderer
                .line(MessageStyle::Info, &content.source_label)?;
            for line in &content.config_lines {
                ctx.renderer.line(MessageStyle::Info, line)?;
            }
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to load configuration for display: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_execute_tool(
    ctx: SlashCommandContext<'_>,
    name: String,
    args: serde_json::Value,
) -> Result<SlashCommandControl> {
    match ensure_tool_permission(
        crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
            tool_registry: ctx.tool_registry,
            renderer: ctx.renderer,
            handle: ctx.handle,
            session: ctx.session,
            default_placeholder: ctx.default_placeholder.clone(),
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            hooks: ctx.lifecycle_hooks,
            justification: None,
            approval_recorder: ctx.approval_recorder,
            decision_ledger: Some(ctx.decision_ledger),
            tool_permission_cache: Some(ctx.tool_permission_cache),
            hitl_notification_bell: ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
            autonomous_mode: ctx.session_stats.is_autonomous_mode(),
            human_in_the_loop: ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.security.human_in_the_loop)
                .unwrap_or(true),
            skip_confirmations: false,
        },
        &name,
        Some(&args),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => Ok(SlashCommandControl::Continue),
        Ok(ToolPermissionFlow::Denied) => Ok(SlashCommandControl::Continue),
        Ok(ToolPermissionFlow::Exit) => {
            Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
        }
        Ok(ToolPermissionFlow::Interrupted) => Ok(SlashCommandControl::BreakWithoutReason),
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub async fn handle_clear_conversation(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.conversation_history.clear();
    *ctx.session_stats = SessionStats::default();
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

pub async fn handle_compact_conversation(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if ctx.conversation_history.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No conversation history to compact.")?;
        return Ok(SlashCommandControl::Continue);
    }

    if !ctx
        .provider_client
        .supports_responses_compaction(&ctx.config.model)
    {
        ctx.renderer.line(
            MessageStyle::Warning,
            "Compaction is unavailable for this provider/endpoint.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let original_len = ctx.conversation_history.len();
    let compacted = match vtcode_core::compaction::compact_history(
        ctx.provider_client.as_ref(),
        &ctx.config.model,
        ctx.conversation_history,
        &vtcode_core::compaction::CompactionConfig::default(),
    )
    .await
    {
        Ok(history) => history,
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Compaction failed: {}", err))?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    if compacted == *ctx.conversation_history {
        ctx.renderer
            .line(MessageStyle::Info, "Conversation is already compact.")?;
        return Ok(SlashCommandControl::Continue);
    }

    *ctx.conversation_history = compacted;
    ctx.session_stats.clear_previous_response_chain();
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Compacted conversation history ({} -> {} messages).",
            original_len,
            ctx.conversation_history.len()
        ),
    )?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_clear_screen(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer.clear_screen();
    ctx.renderer.line(
        MessageStyle::Info,
        "Cleared screen. Conversation context is preserved.",
    )?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_copy_latest_assistant_reply(
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
        vtcode_tui::core_tui::session::mouse_selection::MouseSelectionState::copy_to_clipboard(
            &reply,
        );
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

pub async fn handle_rewind_latest(
    ctx: SlashCommandContext<'_>,
    scope: vtcode_core::core::agent::snapshots::RevertScope,
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

pub async fn handle_rewind_to_turn(
    ctx: SlashCommandContext<'_>,
    turn: usize,
    scope: vtcode_core::core::agent::snapshots::RevertScope,
) -> Result<SlashCommandControl> {
    // Check if checkpoint manager is available
    if let Some(manager) = ctx.checkpoint_manager {
        // Attempt to restore the snapshot
        match manager.restore_snapshot(turn, scope).await {
            Ok(Some(restored)) => {
                // Update conversation history if scope includes conversation
                if scope.includes_conversation() {
                    *ctx.conversation_history = restored
                        .conversation
                        .iter()
                        .map(uni::Message::from)
                        .collect();
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Restored conversation history from turn {} ({} messages)",
                            turn,
                            restored.conversation.len()
                        ),
                    )?;
                }

                // Report code changes if scope includes code
                if scope.includes_code() {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Applied code changes from turn {}", turn),
                    )?;
                }

                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Successfully rewound to turn {} with scope {:?}",
                        turn, scope
                    ),
                )?;
            }
            Ok(None) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("No checkpoint found for turn {}", turn),
                )?;
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to restore checkpoint for turn {}: {}", turn, err),
                )?;
            }
        }
    } else {
        // Fallback to CLI command guidance if checkpoint manager is not available
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("Rewinding to turn {} with scope {:?}...", turn, scope),
        )?;

        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Use: `vtcode revert --turn {} --partial {}` from command line",
                turn,
                match scope {
                    vtcode_core::core::agent::snapshots::RevertScope::Conversation =>
                        "conversation",
                    vtcode_core::core::agent::snapshots::RevertScope::Code => "code",
                    vtcode_core::core::agent::snapshots::RevertScope::Both => "both",
                }
            ),
        )?;

        ctx.renderer.line(
            MessageStyle::Info,
            "Note: In-chat rewind requires access to the checkpoint manager.",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_exit(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer.line(MessageStyle::Info, "✓")?;
    Ok(SlashCommandControl::BreakWithReason(SessionEndReason::Exit))
}
