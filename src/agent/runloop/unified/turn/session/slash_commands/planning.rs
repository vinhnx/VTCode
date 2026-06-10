use anyhow::Result;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::dot_config::load_workspace_trust_level;

use crate::agent::runloop::unified::state::should_enforce_safe_mode_prompts;

use super::{SlashCommandContext, SlashCommandControl};

pub(crate) async fn handle_toggle_planning_workflow(
    mut ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    let current = ctx.tool_registry.is_planning_active();
    let new_state = match enable {
        Some(value) => value,
        None => !current,
    };

    if new_state == current {
        sync_workspace_trust_prompt_policy(&mut ctx, false).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            if current {
                "Planning workflow is already active."
            } else {
                "Planning workflow is already inactive."
            },
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if new_state {
        crate::agent::runloop::unified::planning_workflow_state::transition_to_planning_workflow(
            ctx.tool_registry,
            ctx.session_stats,
            ctx.plan_session,
            ctx.handle,
            PlanningEntrySource::UserRequest,
            true,
            true,
        )
        .await;
        sync_workspace_trust_prompt_policy(&mut ctx, false).await?;
        ctx.renderer
            .line(MessageStyle::Info, "Planning workflow started")?;
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
        crate::agent::runloop::unified::planning_workflow_state::render_planning_workflow_next_step_hint(
            ctx.renderer,
        )?;
    } else {
        crate::agent::runloop::unified::planning_workflow_state::finish_planning_workflow(
            ctx.tool_registry,
            ctx.plan_session,
            ctx.handle,
            true,
        )
        .await;
        sync_workspace_trust_prompt_policy(&mut ctx, false).await?;
        ctx.renderer
            .line(MessageStyle::Info, "Planning workflow finished")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Mutating tools (edits, commands, tests) are now allowed, subject to normal permissions.",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

async fn sync_workspace_trust_prompt_policy(
    ctx: &mut SlashCommandContext<'_>,
    auto_permission_review_active: bool,
) -> Result<()> {
    let workspace_trust_level = match ctx.session_bootstrap.acp_workspace_trust {
        Some(level) => Some(level.to_workspace_trust_level()),
        None => load_workspace_trust_level(&ctx.config.workspace).await?,
    };
    let enforce_safe_mode_prompts = should_enforce_safe_mode_prompts(
        ctx.full_auto,
        auto_permission_review_active,
        workspace_trust_level,
    );
    ctx.tool_registry
        .set_enforce_safe_mode_prompts(enforce_safe_mode_prompts)
        .await;
    Ok(())
}
