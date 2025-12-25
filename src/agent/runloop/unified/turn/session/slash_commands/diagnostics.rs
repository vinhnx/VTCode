use anyhow::{Result, Context};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::config::constants::tools as tools_consts;
use vtcode_core::llm::provider as uni;
use crate::agent::runloop::unified::ui_interaction::{display_session_status, display_token_cost};
use crate::agent::runloop::unified::diagnostics::run_doctor_diagnostics;

use super::{SlashCommandContext, SlashCommandControl, SlashCommandOutcome};

pub async fn handle_debug_agent(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    // Prefer tool-driven diagnostics when available
    if ctx.tool_registry.has_tool(tools_consts::DEBUG_AGENT).await {
        ctx.tool_registry
            .mark_tool_preapproved(tools_consts::DEBUG_AGENT);
        match ctx
            .tool_registry
            .execute_tool_ref(tools_consts::DEBUG_AGENT, &serde_json::json!({}))
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

    // Show current token budget status if enabled
    if ctx.token_budget_enabled {
        let token_budget = ctx.context_manager.token_budget();
        ctx.renderer.line(
            MessageStyle::Output,
            &format!(
                "  Current token budget: {}/{}",
                token_budget.get_stats().await.total_tokens,
                ctx.trim_config.max_tokens
            ),
        )?;
    }

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_show_status(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let token_budget = ctx.context_manager.token_budget();
    let tool_count = ctx.tools.read().await.len();
    display_session_status(
        ctx.renderer,
        crate::agent::runloop::unified::ui_interaction::SessionStatusContext {
            config: ctx.config,
            message_count: ctx.conversation_history.len(),
            stats: ctx.session_stats,
            token_budget: token_budget.as_ref(),
            token_budget_enabled: ctx.token_budget_enabled,
            max_tokens: ctx.trim_config.max_tokens,
            available_tools: tool_count,
        },
    )
    .await?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_show_cost(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let token_budget = ctx.context_manager.token_budget();
    ctx.renderer
        .line(MessageStyle::Info, "Token usage summary:")?;
    display_token_cost(
        ctx.renderer,
        token_budget.as_ref(),
        ctx.token_budget_enabled,
        ctx.trim_config.max_tokens,
        "",
    )
    .await?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_show_context(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    use crate::agent::runloop::context_usage::{ContextUsageInfo, render_context_usage};

    // Build context usage info from current session state
    let mut info = ContextUsageInfo::new(
        &ctx.config.model,
        ctx.trim_config.max_tokens,
    );

    // Get token budget stats
    let token_budget = ctx.context_manager.token_budget();
    let budget_stats = token_budget.get_stats().await;
    info.current_tokens = budget_stats.total_tokens;

    // Estimate system prompt tokens (approximately 10% of typical usage)
    info.system_prompt_tokens = ctx.trim_config.max_tokens / 30;

    // Count tools as system tools tokens
    let tool_count = ctx.tools.read().await.len();
    info.system_tools_tokens = tool_count * 50; // ~50 tokens per tool definition

    // Count messages tokens from conversation history
    info.messages_tokens = ctx.conversation_history.iter()
        .map(|msg| msg.content.as_text().len() / 4)
        .sum();

    // Add loaded skills
    let loaded_skills = ctx.loaded_skills.read().await;
    for (name, skill) in loaded_skills.iter() {
        let tokens = skill.instruction_tokens();
        // Determine scope based on path
        let path_str = skill.path.to_string_lossy();
        if path_str.contains("/.vtcode/skills") || path_str.contains("/.claude/skills") {
            info.user_skills.push(crate::agent::runloop::context_usage::ContextItem::new(name, tokens));
        } else {
            info.project_skills.push(crate::agent::runloop::context_usage::ContextItem::new(name, tokens));
        }
    }

    // Sort skills by token count
    info.user_skills.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    info.project_skills.sort_by(|a, b| b.tokens.cmp(&a.tokens));

    // Add MCP tools from tool registry
    if let Ok(mcp_tools) = ctx.tool_registry.list_mcp_tools().await {
        for tool in mcp_tools {
            // Estimate ~100 tokens per MCP tool
            info.add_mcp_tool(&tool.name, 100);
        }
    }

    // Render the context usage visualization
    render_context_usage(ctx.renderer, &info)?;

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
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
    )
    .await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}
