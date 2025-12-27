use anyhow::{Result, Context};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::config::constants::tools as tools_consts;
use vtcode_core::llm::provider as uni;
use crate::agent::runloop::unified::ui_interaction::display_session_status;
use crate::agent::runloop::unified::diagnostics::run_doctor_diagnostics;

use super::{SlashCommandContext, SlashCommandControl, SlashCommandOutcome};

pub async fn handle_debug_agent(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    // Prefer tool-driven diagnostics when available
    if ctx.tool_registry.has_tool(tools_consts::AGENT_INFO).await {
        ctx.tool_registry
            .mark_tool_preapproved(tools_consts::AGENT_INFO);
        match ctx
            .tool_registry
            .execute_tool_ref(tools_consts::AGENT_INFO, &serde_json::json!({"mode": "debug"}))
            .await
        {
            Ok(value) => {
                ctx.renderer
                    .line(MessageStyle::Info, "Debug information (tool):")?;
                ctx.renderer
                    .line(MessageStyle::Output, &value.to_string())?;
                return Ok(SlashCommandControl::Continue);
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to invoke debug_agent tool: {}", err),
                )?;
            }
        }
    }

    ctx.renderer
        .line(MessageStyle::Info, "Debug information:")?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Current model: {}", ctx.config.model),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Workspace: {}", ctx.config.workspace.display()),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "  Conversation history: {} messages",
            ctx.conversation_history.len()
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "  Available tools: {} registered",
            ctx.tools.read().await.len()
        ),
    )?;
    // Show recent decisions
    let ledger = ctx.decision_ledger.read().await;
    if !ledger.get_decisions().is_empty() {
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("  Recent decisions: {}", ledger.get_decisions().len()),
        )?;
        // Show last few decisions
        let recent = ledger.get_decisions().iter().rev().take(3);
        for (idx, decision) in recent.enumerate() {
            ctx.renderer.line(
                MessageStyle::Output,
                &format!("    {}: {:?}", idx + 1, decision.action),
            )?;
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_analyze_agent(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    // Prefer tool-driven analysis when available
    if ctx
        .tool_registry
        .has_tool(tools_consts::ANALYZE_AGENT)
        .await
    {
        ctx.tool_registry
            .mark_tool_preapproved(tools_consts::ANALYZE_AGENT);
        match ctx
            .tool_registry
            .execute_tool_ref(tools_consts::ANALYZE_AGENT, &serde_json::json!({}))
            .await
        {
            Ok(value) => {
                ctx.renderer
                    .line(MessageStyle::Info, "Agent analysis (tool):")?;
                ctx.renderer
                    .line(MessageStyle::Output, &value.to_string())?;
                return Ok(SlashCommandControl::Continue);
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to invoke analyze_agent tool: {}", err),
                )?;
            }
        }
    }

    ctx.renderer.line(MessageStyle::Info, "Agent analysis:")?;
    ctx.renderer.line(
        MessageStyle::Output,
        "  Analyzing current AI behavior patterns...",
    )?;

    // Calculate some statistics
    let total_messages = ctx.conversation_history.len();
    let tool_calls: usize = ctx
        .conversation_history
        .iter()
        .filter(|msg| msg.role == uni::MessageRole::Assistant)
        .map(|msg| msg.tool_calls.as_ref().map_or(0, |calls| calls.len()))
        .sum();

    let user_messages = ctx
        .conversation_history
        .iter()
        .filter(|msg| msg.role == uni::MessageRole::User)
        .count();

    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Total messages in conversation: {}", total_messages),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  User messages: {}", user_messages),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Assistant tool calls: {}", tool_calls),
    )?;

    if total_messages > 0 {
        let tool_call_ratio = (tool_calls as f64) / (total_messages as f64) * 100.0;
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("  Tool usage ratio: {:.1}%", tool_call_ratio),
        )?;
    }

    // Token budget is disabled in VT Code
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_show_status(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let tool_count = ctx.tools.read().await.len();
    display_session_status(
        ctx.renderer,
        crate::agent::runloop::unified::ui_interaction::SessionStatusContext {
            config: ctx.config,
            message_count: ctx.conversation_history.len(),
            stats: ctx.session_stats,
            token_budget: None,
            token_budget_enabled: false,
            max_tokens: ctx.trim_config.max_tokens,
            available_tools: tool_count,
        },
    )
    .await?;
    Ok(SlashCommandControl::Continue)
}



pub async fn handle_show_pruning_report(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer.line(MessageStyle::Info, "Pruning Report:")?;
    let ledger = ctx.pruning_ledger.read().await;
    let report = ledger.generate_report();

    // Display summary statistics
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "  Total messages evaluated: {}",
            report.statistics.total_messages_evaluated
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Messages kept: {}", report.statistics.messages_kept),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Messages removed: {}", report.statistics.messages_removed),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "  Retention ratio: {:.1}%",
            report.message_retention_ratio * 100.0
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("  Semantic efficiency: {:.2}", report.semantic_efficiency),
    )?;

    // Display brief ledger summary
    let brief = ledger.render_ledger_brief(10);
    if !brief.is_empty() {
        ctx.renderer.line(MessageStyle::Output, "")?;
        ctx.renderer
            .line(MessageStyle::Output, "Recent pruning decisions:")?;
        for line in brief.lines().take(10) {
            ctx.renderer
                .line(MessageStyle::Output, &format!("  {}", line))?;
        }
    }

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_run_doctor(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let provider_runtime = ctx.provider_client.name().to_string();
    run_doctor_diagnostics(
        ctx.renderer,
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &provider_runtime,
        ctx.async_mcp_manager.map(|m| m.as_ref()),
        ctx.linked_directories,
        Some(ctx.loaded_skills),
    )
    .await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}
