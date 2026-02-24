use anyhow::Result;
use vtcode_core::config::types::EditingMode as ConfigEditingMode;
use vtcode_core::ui::tui::EditingMode;
use vtcode_core::utils::ansi::MessageStyle;

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_toggle_plan_mode(
    ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    let current = ctx.session_stats.is_plan_mode();
    let new_state = match enable {
        Some(value) => value,
        None => !current,
    };

    if new_state == current {
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
            true,
            true,
        )
        .await;
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
            "  Mutating tools are blocked; optional plan-file writes under `.vtcode/plans/` remain allowed.",
        )?;
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Allowed tools: read_file, list_files, grep_file, code_intelligence, unified_search, request_user_input, spawn_subagent",
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
        ctx.renderer.line(
            MessageStyle::Info,
            "Edit Mode enabled (coder profile active)",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Mutating tools (edits, commands, tests) are now allowed, subject to normal permissions.",
        )?;
    }

    let persisted_mode = if new_state {
        ConfigEditingMode::Plan
    } else {
        ConfigEditingMode::Edit
    };
    if let Err(err) = super::persist_mode_settings(
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        Some(persisted_mode),
        None,
    ) {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist plan mode preference: {}", err),
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_toggle_autonomous_mode(
    ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    let current = ctx.session_stats.is_autonomous_mode();
    let new_state = match enable {
        Some(value) => value,
        None => !current,
    };

    if new_state == current {
        ctx.renderer.line(
            MessageStyle::Info,
            if current {
                "Autonomous Mode is already enabled."
            } else {
                "Autonomous Mode is already disabled."
            },
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.session_stats.set_autonomous_mode(new_state);
    ctx.handle.set_autonomous_mode(new_state);

    if new_state {
        ctx.renderer
            .line(MessageStyle::Info, "Autonomous Mode enabled")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  The agent will work more autonomously with fewer confirmation prompts.",
        )?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Safe tools (read/search) are auto-approved. Use with caution.",
        )?;
    } else {
        ctx.renderer
            .line(MessageStyle::Info, "Autonomous Mode disabled")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Standard human-in-the-loop prompts are now active for all mutating actions.",
        )?;
    }

    if let Err(err) = super::persist_mode_settings(
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        None,
        Some(new_state),
    ) {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist autonomous mode preference: {}", err),
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

pub async fn handle_cycle_mode(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let new_mode = ctx.session_stats.cycle_mode();
    if new_mode == EditingMode::Plan {
        crate::agent::runloop::unified::plan_mode_state::transition_to_plan_mode(
            ctx.tool_registry,
            ctx.session_stats,
            ctx.handle,
            false,
            false,
        )
        .await;
    } else {
        crate::agent::runloop::unified::plan_mode_state::transition_to_edit_mode(
            ctx.tool_registry,
            ctx.session_stats,
            ctx.handle,
            true,
        )
        .await;
    }

    match new_mode {
        EditingMode::Edit => {
            ctx.renderer
                .line(MessageStyle::Info, "Switched to Edit Mode")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  Full tool access with standard confirmation prompts.",
            )?;
        }
        EditingMode::Plan => {
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

    let persisted_mode = match new_mode {
        EditingMode::Plan => ConfigEditingMode::Plan,
        EditingMode::Edit => ConfigEditingMode::Edit,
    };
    if let Err(err) = super::persist_mode_settings(
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        Some(persisted_mode),
        None,
    ) {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist editing mode preference: {}", err),
        )?;
    }

    Ok(SlashCommandControl::Continue)
}
