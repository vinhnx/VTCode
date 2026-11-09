use anyhow::Result;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::dot_config::load_workspace_trust_level;

use crate::agent::runloop::unified::planning_workflow::PlanningFinishReason;
use crate::agent::runloop::unified::planning_workflow_state::{
    PLAN_PRIMARY_AGENT_NAME, apply_agent_header, apply_plan_agent_header,
};
use crate::agent::runloop::unified::state::should_enforce_safe_mode_prompts;
use crate::agent::runloop::unified::turn::primary_agent_runtime::load_primary_agent_specs;

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
        if current && enable == Some(true) {
            ctx.plan_session.mark_explicit_plan_request();
        }
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
        // Capture the execution agent before the plan switch so `/plan off`
        // can restore it. After selection the active agent is `plan`.
        let previous = ctx.active_primary_agent.active().name().to_string();
        select_plan_primary_agent(&mut ctx).await?;
        crate::agent::runloop::unified::planning_workflow_state::transition_to_planning_workflow(
            ctx.tool_registry,
            ctx.session_stats,
            ctx.plan_session,
            ctx.handle,
            PlanningEntrySource::UserRequest,
            Some(previous),
            ctx.vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
            true,
            true,
        )
        .await;
        apply_plan_agent_header(ctx.handle);
        sync_workspace_trust_prompt_policy(&mut ctx, false).await?;
        ctx.renderer.line(MessageStyle::Info, "Planning workflow started")?;
        // No researching indicator here: no planning request exists yet. The
        // indicator is rendered once per planning turn when work starts.
        ctx.renderer
            .line(MessageStyle::Output, "  The agent will focus on analysis and planning with a structured plan.")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Mutating tools are blocked; optional plan-file writes under `.vtcode/plans/` (or an explicit custom plan path) remain allowed.",
        )?;
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Allowed tools: exec_command for shell inspection, code_search for a focused literal query with bounded filters, apply_patch for allowed plan file edits, request_user_input for planning interview prompts",
        )?;
        crate::agent::runloop::unified::planning_workflow_state::render_planning_workflow_next_step_hint(ctx.renderer)?;
    } else {
        // Capture restore target before `exit()` clears planning session state.
        let restore_agent = ctx.plan_session.restore_agent_after_planning().map(str::to_owned);
        crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
            ctx.plan_session,
            ctx.harness_emitter,
            ctx.thread_id,
            ctx.thread_id,
            vtcode_core::exec::events::PlanApprovalDecision::Cancel,
            false,
        );
        let removed = crate::agent::runloop::unified::planning_workflow::clear_stale_recovery_directives_for_execution(
            ctx.conversation_history,
        );
        if removed > 0 {
            tracing::info!(removed, "Cleared stale recovery directives when planning workflow was disabled");
        }
        crate::agent::runloop::unified::planning_workflow::finish_planning_workflow(
            ctx.tool_registry,
            ctx.plan_session,
            ctx.handle,
            PlanningFinishReason::Cancelled,
        )
        .await?;
        if let Some(agent) = restore_agent.clone() {
            restore_execution_primary_agent(&mut ctx, &agent).await?;
        }
        // Always rewrite the header from the exit name so a Plan badge cannot
        // stick after a no-op restore or a failed agent selection.
        let active_display = ctx.active_primary_agent.active().display_name.clone();
        let exit_name = crate::agent::runloop::unified::planning_workflow_state::plan_exit_header_name(
            restore_agent.as_deref(),
            &active_display,
        )
        .to_owned();
        let color = ctx.active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
        ctx.header_context.primary_agent = Some(exit_name.clone());
        ctx.header_context.primary_agent_color = color.clone();
        apply_agent_header(ctx.handle, &exit_name, color);
        tracing::info!(
            target: "vtcode.planning_workflow",
            switch_path = "plan_exit",
            restored_agent = ?restore_agent,
            header_agent = %exit_name,
            "Planning workflow finished via /plan off"
        );
        sync_workspace_trust_prompt_policy(&mut ctx, false).await?;
        ctx.renderer.line(MessageStyle::Info, "Planning workflow finished")?;
        ctx.renderer.line(
            MessageStyle::Output,
            "  Mutating tools (edits, commands, tests) are now allowed, subject to normal permissions.",
        )?;
    }

    Ok(SlashCommandControl::Continue)
}

/// Full switch to the built-in plan primary agent so prompt, tool catalog, and
/// header badge agree with the planning workflow. Mirrors Tab / `/mode plan`.
async fn select_plan_primary_agent(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    if ctx
        .active_primary_agent
        .active()
        .identity
        .name
        .eq_ignore_ascii_case(PLAN_PRIMARY_AGENT_NAME)
    {
        apply_plan_agent_header(ctx.handle);
        return Ok(());
    }

    let specs = load_primary_agent_specs_or_builtin(ctx).await;
    match ctx.active_primary_agent.select_from_specs(&specs, PLAN_PRIMARY_AGENT_NAME) {
        Ok(active) => {
            let display_name = active.display_name.clone();
            let color = active.color.clone().filter(|c| !c.trim().is_empty());
            let policy_overrides = active.tool_policy_overrides.clone();
            for (tool_name, policy) in policy_overrides {
                if let Err(err) = ctx.tool_registry.set_tool_policy(&tool_name, policy).await {
                    tracing::warn!("Failed to apply tool policy override for '{tool_name}' on plan entry: {err}");
                }
            }
            ctx.header_context.primary_agent = Some(display_name.clone());
            ctx.header_context.primary_agent_color = color.clone();
            apply_agent_header(ctx.handle, &display_name, color);
            tracing::info!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_entry",
                agent = %display_name,
                "Selected plan primary agent via /plan"
            );
            Ok(())
        }
        Err(err) => {
            tracing::warn!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_entry",
                error = %err,
                "Failed to select plan primary agent via /plan; planning workflow stays active"
            );
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to select plan primary agent: {err}"))?;
            Ok(())
        }
    }
}

async fn restore_execution_primary_agent(ctx: &mut SlashCommandContext<'_>, name: &str) -> Result<()> {
    if ctx.active_primary_agent.active().identity.name.eq_ignore_ascii_case(name) {
        // Agent already matches, but the Plan badge may still be showing if an
        // earlier entry path set it without completing the agent switch.
        refresh_header_from_active_agent(ctx);
        return Ok(());
    }
    let specs = load_primary_agent_specs_or_builtin(ctx).await;
    match ctx.active_primary_agent.select_from_specs(&specs, name) {
        Ok(active) => {
            let display_name = active.display_name.clone();
            let color = active.color.clone().filter(|c| !c.trim().is_empty());
            let policy_overrides = active.tool_policy_overrides.clone();
            for (tool_name, policy) in policy_overrides {
                if let Err(err) = ctx.tool_registry.set_tool_policy(&tool_name, policy).await {
                    tracing::warn!("Failed to apply tool policy override for '{tool_name}' on plan exit: {err}");
                }
            }
            ctx.header_context.primary_agent = Some(display_name.clone());
            ctx.header_context.primary_agent_color = color.clone();
            apply_agent_header(ctx.handle, &display_name, color);
            tracing::info!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_exit",
                agent = %display_name,
                "Restored execution primary agent after planning"
            );
            Ok(())
        }
        Err(err) => {
            tracing::warn!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_exit",
                requested_agent = %name,
                error = %err,
                "Could not restore primary agent after planning; header falls back to active agent"
            );
            ctx.renderer.line(
                MessageStyle::Warning,
                &format!("Could not restore primary agent '{name}' after planning: {err}"),
            )?;
            refresh_header_from_active_agent(ctx);
            Ok(())
        }
    }
}

fn refresh_header_from_active_agent(ctx: &mut SlashCommandContext<'_>) {
    let display_name = ctx.active_primary_agent.active().display_name.clone();
    let color = ctx.active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
    ctx.header_context.primary_agent = Some(display_name.clone());
    ctx.header_context.primary_agent_color = color.clone();
    apply_agent_header(ctx.handle, &display_name, color);
}

async fn load_primary_agent_specs_or_builtin(ctx: &SlashCommandContext<'_>) -> Vec<vtcode_config::SubagentSpec> {
    use crate::agent::runloop::unified::turn::primary_agent_runtime::builtin_primary_agent_specs;
    match load_primary_agent_specs(ctx.tool_registry, &ctx.config.workspace).await {
        Ok(specs) if !specs.is_empty() => specs,
        Ok(_) => builtin_primary_agent_specs(),
        Err(err) => {
            tracing::warn!(error = %err, "Primary-agent discovery failed during planning toggle; using built-ins");
            builtin_primary_agent_specs()
        }
    }
}

async fn sync_workspace_trust_prompt_policy(
    ctx: &mut SlashCommandContext<'_>,
    auto_permission_review_active: bool,
) -> Result<()> {
    let workspace_trust_level = match ctx.session_bootstrap.acp_workspace_trust {
        Some(level) => Some(level.to_workspace_trust_level()),
        None => load_workspace_trust_level(&ctx.config.workspace).await?,
    };
    let enforce_safe_mode_prompts =
        should_enforce_safe_mode_prompts(ctx.full_auto, auto_permission_review_active, workspace_trust_level);
    ctx.tool_registry.set_enforce_safe_mode_prompts(enforce_safe_mode_prompts).await;
    Ok(())
}
