use anyhow::Result;
use vtcode_core::config::PermissionMode;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::EditingMode as ConfigEditingMode;
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::load_workspace_trust_level;
use vtcode_tui::app::{EditingMode, InlineListItem, InlineListSelection};

use crate::agent::runloop::slash_commands::SessionModeCommand;
use crate::agent::runloop::unified::state::{SessionMode, should_enforce_safe_mode_prompts};

use super::{SlashCommandContext, SlashCommandControl};

pub(crate) async fn handle_toggle_plan_mode(
    mut ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    let current = ctx.session_stats.is_plan_mode();
    let new_state = match enable {
        Some(value) => value,
        None => !current,
    };

    if new_state == current {
        sync_workspace_trust_prompt_policy(
            &mut ctx,
            if current {
                SessionMode::Plan
            } else {
                SessionMode::Edit
            },
        )
        .await?;
        ctx.renderer.line(
            MessageStyle::Info,
            if current {
                "Plan Mode is already enabled (strict read-only)."
            } else {
                "Plan Mode is already disabled."
            },
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if new_state {
        crate::agent::runloop::unified::plan_mode_state::transition_to_plan_mode(
            ctx.tool_registry,
            ctx.session_stats,
            ctx.handle,
            PlanModeEntrySource::UserRequest,
            true,
            true,
        )
        .await;
        sync_workspace_trust_prompt_policy(&mut ctx, SessionMode::Plan).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Plan Mode enabled (planner profile active)",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  The agent will focus on analysis and planning with a structured plan.",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Mutating tools are blocked; optional plan-file writes under `.vtcode/plans/` (or an explicit custom plan path) remain allowed.",
        )?;
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Allowed tools: read_file, list_files, grep_file, unified_search, request_user_input",
        )?;
        crate::agent::runloop::unified::plan_mode_state::render_plan_mode_next_step_hint(
            ctx.renderer,
        )?;
    } else {
        crate::agent::runloop::unified::plan_mode_state::transition_to_edit_mode(
            ctx.tool_registry,
            ctx.session_stats,
            ctx.handle,
            true,
        )
        .await;
        sync_workspace_trust_prompt_policy(&mut ctx, SessionMode::Edit).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Edit Mode enabled (coder profile active)",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Mutating tools (edits, commands, tests) are now allowed, subject to normal permissions.",
        )?;
    }

    persist_mode_preference(
        ctx.renderer,
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        if new_state {
            ConfigEditingMode::Plan
        } else {
            ConfigEditingMode::Edit
        },
        Some(PermissionMode::Default),
        "plan mode preference",
    )?;

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_cycle_mode(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let new_mode = match ctx.session_stats.current_mode() {
        SessionMode::Edit => SessionMode::Auto,
        SessionMode::Auto => SessionMode::Plan,
        SessionMode::Plan => SessionMode::Edit,
    };
    apply_session_mode(ctx, new_mode).await
}

pub(crate) async fn handle_set_mode(
    mut ctx: SlashCommandContext<'_>,
    mode: SessionModeCommand,
) -> Result<SlashCommandControl> {
    let requested = match mode {
        SessionModeCommand::Edit => SessionMode::Edit,
        SessionModeCommand::Auto => SessionMode::Auto,
        SessionModeCommand::Plan => SessionMode::Plan,
    };

    if ctx.session_stats.current_mode() == requested {
        sync_workspace_trust_prompt_policy(&mut ctx, requested).await?;
        ctx.renderer
            .line(MessageStyle::Info, already_active_message(requested))?;
        return Ok(SlashCommandControl::Continue);
    }

    apply_session_mode(ctx, requested).await
}

pub(crate) async fn handle_start_mode_selection(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Use `/mode edit`, `/mode auto`, `/mode plan`, or `/mode cycle` outside inline UI.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "choosing a session mode")? {
        return Ok(SlashCommandControl::Continue);
    }

    let current_mode = ctx.session_stats.current_mode();
    let items = vec![
        mode_item(
            "Edit Mode",
            "Full tool access with standard confirmation prompts",
            "mode:edit",
            "mode edit normal confirmations standard",
            current_mode == SessionMode::Edit,
        ),
        mode_item(
            "Auto Mode",
            "Classifier-backed approvals with deny-and-continue recovery",
            "mode:auto",
            "mode auto classifier approvals autonomous",
            current_mode == SessionMode::Auto,
        ),
        mode_item(
            "Plan Mode",
            "Read-only planning and analysis; mutating tools disabled",
            "mode:plan",
            "mode plan readonly planning analysis",
            current_mode == SessionMode::Plan,
        ),
    ];

    ctx.handle.show_list_modal(
        "Session mode".to_string(),
        vec![
            "Choose how VT Code should run this session.".to_string(),
            "Edit uses normal confirmations, Auto uses background classifier checks, and Plan is read-only.".to_string(),
        ],
        items,
        Some(mode_selection_for(current_mode)),
        None,
    );

    let Some(selection) = super::ui::wait_for_list_modal_selection(&mut ctx).await else {
        ctx.renderer
            .line(MessageStyle::Info, "Mode selection cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };

    let requested = match selection {
        InlineListSelection::ConfigAction(action) if action == "mode:edit" => SessionMode::Edit,
        InlineListSelection::ConfigAction(action) if action == "mode:auto" => SessionMode::Auto,
        InlineListSelection::ConfigAction(action) if action == "mode:plan" => SessionMode::Plan,
        _ => {
            ctx.renderer.line(
                MessageStyle::Error,
                "Unsupported mode selection received from inline UI.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    if requested == current_mode {
        ctx.renderer
            .line(MessageStyle::Info, already_active_message(requested))?;
        return Ok(SlashCommandControl::Continue);
    }

    apply_session_mode(ctx, requested).await
}

fn persist_mode_preference(
    renderer: &mut AnsiRenderer,
    workspace: &std::path::Path,
    vt_cfg: &mut Option<VTCodeConfig>,
    mode: ConfigEditingMode,
    permission_mode: Option<PermissionMode>,
    preference_label: &str,
) -> Result<()> {
    if let Err(err) = super::persist_mode_settings(workspace, vt_cfg, Some(mode), permission_mode) {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist {preference_label}: {}", err),
        )?;
    }

    Ok(())
}

async fn apply_session_mode(
    mut ctx: SlashCommandContext<'_>,
    new_mode: SessionMode,
) -> Result<SlashCommandControl> {
    match new_mode {
        SessionMode::Plan => {
            crate::agent::runloop::unified::plan_mode_state::transition_to_plan_mode(
                ctx.tool_registry,
                ctx.session_stats,
                ctx.handle,
                PlanModeEntrySource::UserRequest,
                false,
                false,
            )
            .await;
        }
        SessionMode::Edit | SessionMode::Auto => {
            ctx.tool_registry.disable_plan_mode();
            let plan_state = ctx.tool_registry.plan_mode_state();
            plan_state.disable();
            plan_state.set_plan_file(None).await;
            ctx.session_stats.set_plan_mode(false);
            ctx.session_stats
                .set_autonomous_mode(matches!(new_mode, SessionMode::Auto));
            ctx.handle.set_editing_mode(EditingMode::Edit);
            ctx.handle
                .set_autonomous_mode(matches!(new_mode, SessionMode::Auto));
        }
    }

    sync_workspace_trust_prompt_policy(&mut ctx, new_mode).await?;

    match new_mode {
        SessionMode::Edit => {
            ctx.renderer
                .line(MessageStyle::Info, "Switched to Edit Mode")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Full tool access with standard confirmation prompts.",
            )?;
        }
        SessionMode::Auto => {
            ctx.renderer
                .line(MessageStyle::Info, "Switched to Auto Mode")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Classifier-backed permission checks run in the background; blocked actions should retry with a safer path.",
            )?;
        }
        SessionMode::Plan => {
            ctx.renderer
                .line(MessageStyle::Info, "Switched to Plan Mode")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Read-only mode for analysis and planning. Mutating tools disabled.",
            )?;
            crate::agent::runloop::unified::plan_mode_state::render_plan_mode_next_step_hint(
                ctx.renderer,
            )?;
        }
    }

    persist_mode_preference(
        ctx.renderer,
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        match new_mode {
            SessionMode::Plan => ConfigEditingMode::Plan,
            SessionMode::Edit | SessionMode::Auto => ConfigEditingMode::Edit,
        },
        match new_mode {
            SessionMode::Auto => Some(PermissionMode::Auto),
            SessionMode::Edit | SessionMode::Plan => Some(PermissionMode::Default),
        },
        "editing mode preference",
    )?;

    Ok(SlashCommandControl::Continue)
}

async fn sync_workspace_trust_prompt_policy(
    ctx: &mut SlashCommandContext<'_>,
    mode: SessionMode,
) -> Result<()> {
    let workspace_trust_level = match ctx.session_bootstrap.acp_workspace_trust {
        Some(level) => Some(level.to_workspace_trust_level()),
        None => load_workspace_trust_level(&ctx.config.workspace).await?,
    };
    let enforce_safe_mode_prompts = should_enforce_safe_mode_prompts(
        ctx.full_auto,
        matches!(mode, SessionMode::Auto),
        workspace_trust_level,
    );
    ctx.tool_registry
        .set_enforce_safe_mode_prompts(enforce_safe_mode_prompts)
        .await;
    Ok(())
}

fn mode_item(
    title: &str,
    subtitle: &str,
    action: &str,
    search_value: &str,
    current: bool,
) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: current.then_some("Current".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(action.to_string())),
        search_value: Some(search_value.to_string()),
    }
}

fn mode_selection_for(mode: SessionMode) -> InlineListSelection {
    match mode {
        SessionMode::Edit => InlineListSelection::ConfigAction("mode:edit".to_string()),
        SessionMode::Auto => InlineListSelection::ConfigAction("mode:auto".to_string()),
        SessionMode::Plan => InlineListSelection::ConfigAction("mode:plan".to_string()),
    }
}

fn already_active_message(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Edit => "Edit Mode is already active.",
        SessionMode::Auto => "Auto Mode is already active.",
        SessionMode::Plan => "Plan Mode is already active.",
    }
}
