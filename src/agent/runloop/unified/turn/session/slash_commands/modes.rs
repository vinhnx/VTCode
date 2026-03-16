use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::EditingMode as ConfigEditingMode;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::EditingMode;

use super::{SlashCommandContext, SlashCommandControl};

pub(crate) async fn handle_toggle_plan_mode(
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
        "plan mode preference",
    )?;

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_cycle_mode(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
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

    persist_mode_preference(
        ctx.renderer,
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        match new_mode {
            EditingMode::Plan => ConfigEditingMode::Plan,
            EditingMode::Edit => ConfigEditingMode::Edit,
        },
        "editing mode preference",
    )?;

    Ok(SlashCommandControl::Continue)
}

fn persist_mode_preference(
    renderer: &mut AnsiRenderer,
    workspace: &std::path::Path,
    vt_cfg: &mut Option<VTCodeConfig>,
    mode: ConfigEditingMode,
    preference_label: &str,
) -> Result<()> {
    if let Err(err) = super::persist_mode_settings(workspace, vt_cfg, Some(mode), None) {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist {preference_label}: {}", err),
        )?;
    }

    Ok(())
}
