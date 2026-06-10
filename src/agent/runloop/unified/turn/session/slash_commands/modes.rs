use anyhow::Result;
use vtcode_core::config::PermissionMode;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::load_workspace_trust_level;

use crate::agent::runloop::unified::state::should_enforce_safe_mode_prompts;

use super::{SlashCommandContext, SlashCommandControl};

pub(crate) async fn handle_toggle_plan_mode(
    mut ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    let current = ctx.tool_registry.is_plan_mode();
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
        crate::agent::runloop::unified::plan_mode_state::transition_to_plan_mode(
            ctx.tool_registry,
            ctx.session_stats,
            ctx.plan_session,
            ctx.handle,
            PlanModeEntrySource::UserRequest,
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
        crate::agent::runloop::unified::plan_mode_state::render_plan_mode_next_step_hint(
            ctx.renderer,
        )?;
    } else {
        crate::agent::runloop::unified::plan_mode_state::transition_to_edit_mode(
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

    persist_mode_preference(
        ctx.renderer,
        ctx.config.workspace.as_path(),
        ctx.vt_cfg,
        Some(if new_state {
            PermissionMode::Plan
        } else {
            PermissionMode::Default
        }),
        "planning workflow preference",
    )?;

    Ok(SlashCommandControl::Continue)
}

fn persist_mode_preference(
    renderer: &mut AnsiRenderer,
    workspace: &std::path::Path,
    vt_cfg: &mut Option<VTCodeConfig>,
    permission_mode: Option<PermissionMode>,
    preference_label: &str,
) -> Result<()> {
    if let Err(err) = super::persist_mode_settings(workspace, vt_cfg, permission_mode) {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist {preference_label}: {}", err),
        )?;
    }

    Ok(())
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
