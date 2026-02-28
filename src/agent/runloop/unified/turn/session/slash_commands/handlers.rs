use anyhow::{Context, Result};
use serde_json;
use vtcode_core::llm::provider::MessageRole;

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::EditingMode as ConfigEditingMode;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::transcript;

use crate::agent::runloop::slash_commands::SubagentConfigCommandAction;
use crate::agent::runloop::unified::state::{ModelPickerTarget, SessionStats};
use crate::agent::runloop::unified::team_state::TeamState;
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::hooks::lifecycle::SessionEndReason;
use vtcode_core::agent_teams::{TeamRole, TeamStorage, TeamTaskStatus};

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::palettes::{ActivePalette, show_config_palette};
use crate::agent::runloop::unified::turn::config_modal::load_config_modal_content;
#[path = "agents.rs"]
mod agents;
#[path = "apps.rs"]
mod apps;
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
#[path = "team.rs"]
mod team;
#[path = "ui.rs"]
mod ui;
#[path = "workspace.rs"]
mod workspace;
pub use agents::handle_manage_agents;
pub use apps::{handle_launch_editor, handle_launch_git, handle_new_session, handle_open_docs};
pub use diagnostics::{handle_run_doctor, handle_show_status, handle_start_terminal_setup};
pub use mcp::handle_manage_mcp;
pub use modes::{handle_cycle_mode, handle_toggle_autonomous_mode, handle_toggle_plan_mode};
pub use oauth::{handle_oauth_login, handle_oauth_logout, handle_show_auth_status};
pub use share_log::handle_share_log;
pub use skills::handle_manage_skills;
pub use team::handle_manage_teams;
pub use ui::{
    handle_start_file_browser, handle_start_model_selection, handle_start_sessions_palette,
    handle_start_theme_palette, handle_theme_changed,
};
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
            delegate_mode: ctx.session_stats.is_delegate_mode(),
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
        vtcode_tui::core_tui::session::mouse_selection::MouseSelectionState::copy_to_clipboard_osc52(
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

fn agent_teams_enabled(vt_cfg: &Option<VTCodeConfig>) -> bool {
    if let Ok(value) = std::env::var("VTCODE_EXPERIMENTAL_AGENT_TEAMS") {
        let normalized = value.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), "1" | "true" | "yes") {
            return true;
        }
    }

    vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent_teams.enabled)
        .unwrap_or(false)
}

fn resolve_max_teammates(vt_cfg: &Option<VTCodeConfig>) -> usize {
    let max_teammates = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent_teams.max_teammates)
        .unwrap_or(4);
    max_teammates.max(1)
}

fn resolve_default_team_model(
    vt_cfg: &Option<VTCodeConfig>,
    override_model: Option<String>,
) -> Option<String> {
    override_model.or_else(|| {
        vt_cfg
            .as_ref()
            .and_then(|cfg| cfg.agent_teams.default_model.clone())
    })
}

fn render_team_usage(renderer: &mut vtcode_core::utils::ansi::AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, "Agent Teams (experimental)")?;
    renderer.line(MessageStyle::Output, "")?;
    renderer.line(
        MessageStyle::Output,
        "  /team start [name] [count] [subagent_type] [--model MODEL]",
    )?;
    renderer.line(
        MessageStyle::Output,
        "  /team add <name> [subagent_type] [--model MODEL]",
    )?;
    renderer.line(MessageStyle::Output, "  /team remove <name>")?;
    renderer.line(
        MessageStyle::Output,
        "  /team task add <description> [--depends-on 1,2]",
    )?;
    renderer.line(MessageStyle::Output, "  /team task claim <task_id>")?;
    renderer.line(
        MessageStyle::Output,
        "  /team task complete <task_id> [summary]",
    )?;
    renderer.line(
        MessageStyle::Output,
        "  /team task fail <task_id> [summary]",
    )?;
    renderer.line(MessageStyle::Output, "  /team assign <task_id> <teammate>")?;
    renderer.line(
        MessageStyle::Output,
        "  /team message <teammate|lead> <message>",
    )?;
    renderer.line(MessageStyle::Output, "  /team broadcast <message>")?;
    renderer.line(MessageStyle::Output, "  /team tasks")?;
    renderer.line(MessageStyle::Output, "  /team teammates")?;
    renderer.line(MessageStyle::Output, "  /team model")?;
    renderer.line(MessageStyle::Output, "  /team stop")?;
    Ok(())
}

async fn ensure_team_state(ctx: &mut SlashCommandContext<'_>) -> Result<bool> {
    if ctx.session_stats.team_state.is_none() {
        let Some(team_context) = ctx.session_stats.team_context.clone() else {
            return Ok(false);
        };
        let storage = TeamStorage::from_config(ctx.vt_cfg.as_ref()).await?;
        match TeamState::load(storage, &team_context.team_name).await {
            Ok(team) => {
                ctx.session_stats.team_state = Some(team);
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to load team '{}': {}", team_context.team_name, err),
                )?;
                ctx.session_stats.team_context = None;
                return Ok(false);
            }
        }
    }

    Ok(ctx.session_stats.team_state.is_some())
}

fn resolve_teammate_mode(
    vt_cfg: &Option<VTCodeConfig>,
) -> vtcode_config::agent_teams::TeammateMode {
    let mode = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent_teams.teammate_mode)
        .unwrap_or(vtcode_config::agent_teams::TeammateMode::Auto);
    match mode {
        vtcode_config::agent_teams::TeammateMode::Auto => {
            if std::env::var("TMUX").is_ok() {
                vtcode_config::agent_teams::TeammateMode::Tmux
            } else {
                vtcode_config::agent_teams::TeammateMode::InProcess
            }
        }
        _ => mode,
    }
}

fn current_sender(ctx: &SlashCommandContext<'_>) -> String {
    match ctx.session_stats.team_context.as_ref() {
        Some(context) if context.role == TeamRole::Teammate => context
            .teammate_name
            .clone()
            .unwrap_or_else(|| "teammate".to_string()),
        _ => "lead".to_string(),
    }
}

fn is_teammate_idle(tasks: &vtcode_core::agent_teams::TeamTaskList, teammate: &str) -> bool {
    !tasks.tasks.iter().any(|task| {
        task.assigned_to.as_deref() == Some(teammate)
            && matches!(
                task.status,
                TeamTaskStatus::Pending | TeamTaskStatus::InProgress
            )
    })
}

pub async fn handle_manage_subagent_config(
    ctx: SlashCommandContext<'_>,
    action: SubagentConfigCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        SubagentConfigCommandAction::Model => {
            ctx.session_stats.model_picker_target = ModelPickerTarget::SubagentDefault;
            ui::start_model_picker(ctx).await
        }
    }
}
