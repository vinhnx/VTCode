pub(crate) mod slash_commands;

use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::llm::TokenCounter;
use vtcode_core::llm::provider as uni;

/// Centralized handling for tool failures discovered in the run loop.
/// This function renders an informative error, adds an MCP event if applicable,
/// counts tokens, updates the decision ledger, and appends a tool response to the conversation history.
pub(crate) async fn handle_tool_failure(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    error: &anyhow::Error,
    vt_cfg: Option<&VTCodeConfig>,
    token_budget: &Arc<TokenBudgetManager>,
    token_counter: &Arc<RwLock<TokenCounter>>,
    working_history: &mut Vec<uni::Message>,
    last_tool_stdout: &mut Option<String>,
    decision_ledger: &Arc<RwLock<DecisionTracker>>,
    dec_id: &str,
    call_id: &str,
) -> Result<()> {
    use crate::agent::runloop::tool_output::render_tool_output;
    use vtcode_core::tools::error_context::ToolErrorContext;
    use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, classify_error};

    // Record in trajectory
    ctx.traj
        .log_tool_call(working_history.len(), name, args_val, false);

    // Build structured error
    let error_chain: Vec<String> = error.chain().map(|s| s.to_string()).collect();
    let error_summary = error_chain
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown tool error".to_string());
    let original_details = if error_chain.len() <= 1 {
        error_summary.clone()
    } else {
        error_chain.join(" -> ")
    };

    let classified = classify_error(error);
    let structured = ToolExecutionError::with_original_error(
        name.to_string(),
        classified.clone(),
        error_summary.clone(),
        original_details.clone(),
    );
    let error_json = structured.to_json_value();
    let error_message = structured.message.clone();

    // MCP event
    if let Some(tool_name) = name.strip_prefix("mcp_") {
        ctx.renderer
            .line_if_not_empty(vtcode_core::utils::ansi::MessageStyle::Output)?;
        ctx.renderer.line(
            vtcode_core::utils::ansi::MessageStyle::Error,
            &format!("MCP tool {} failed: {}", tool_name, error_message),
        )?;
        ctx.handle.force_redraw();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
            "mcp".to_string(),
            tool_name.to_string(),
            Some(args_val.to_string()),
        );
        mcp_event.failure(Some(error_message.clone()));
        ctx.mcp_panel_state.add_event(mcp_event);
    }

    // Show friendly message
    let error_ctx =
        ToolErrorContext::new(name.to_string(), error_message.clone()).with_auto_recovery();
    ctx.renderer.line(
        vtcode_core::utils::ansi::MessageStyle::Error,
        &error_ctx.format_for_user(),
    )?;

    // Show brief error type
    let error_type_msg = match classified {
        ToolErrorType::InvalidParameters => "Invalid parameters provided",
        ToolErrorType::ToolNotFound => "Tool not found",
        ToolErrorType::ResourceNotFound => "Resource not found",
        ToolErrorType::PermissionDenied => "Permission denied",
        ToolErrorType::ExecutionError => "Execution error",
        ToolErrorType::PolicyViolation => "Policy violation",
        ToolErrorType::Timeout => "Operation timed out",
        ToolErrorType::NetworkError => "Network error",
    };
    ctx.renderer.line(
        vtcode_core::utils::ansi::MessageStyle::Info,
        &format!("Type: {}", error_type_msg),
    )?;

    // Render tool output with standardized view
    render_tool_output(
        &mut ctx.renderer,
        Some(name),
        &error_json,
        vt_cfg,
        Some(&token_budget),
    )
    .await?;

    let error_content = serde_json::to_string(&error_json).unwrap_or_else(|_| "{}".to_string());
    {
        let mut counter = token_counter.write().await;
        counter.count_with_profiling("tool_output", &error_content);
    }

    working_history.push(uni::Message::tool_response_with_origin(
        call_id.to_string(),
        error_content,
        name.to_string(),
    ));
    let _ = last_tool_stdout.take();

    crate::agent::runloop::unified::tool_ledger::record_outcome_for_decision(
        &decision_ledger,
        dec_id,
        false,
        Some(error_message.clone()),
    )
    .await?;

    Ok(())
}
