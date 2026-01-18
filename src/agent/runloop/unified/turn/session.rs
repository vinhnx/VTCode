pub mod interaction_loop;
pub mod slash_commands;
pub mod tool_dispatch;
pub mod slash_command_handler;
pub mod mcp_lifecycle;

use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;
// session_loop is its own module at turn/session_loop.rs

pub struct ToolFailureDetails<'a> {
    pub name: &'a str,
    pub args_val: &'a serde_json::Value,
    pub error: &'a anyhow::Error,
    pub dec_id: &'a str,
    pub call_id: &'a str,
}

/// Centralized handling for tool failures discovered in the run loop.
/// This function renders an informative error, adds an MCP event if applicable,
/// updates the decision ledger, and appends a tool response to the conversation history.
#[allow(dead_code)]
pub(crate) async fn handle_tool_failure(
    ctx: &mut RunLoopContext<'_>,
    details: ToolFailureDetails<'_>,
    vt_cfg: Option<&VTCodeConfig>,
    working_history: &mut Vec<uni::Message>,
    last_tool_stdout: &mut Option<String>,
) -> Result<()> {
    use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output;
    use vtcode_core::tools::error_context::ToolErrorContext;
    use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, classify_error};

    // Record in trajectory
    ctx.traj
        .log_tool_call(working_history.len(), details.name, details.args_val, false);

    // Build structured error
    let error_chain: Vec<String> = details.error.chain().map(|s| s.to_string()).collect();
    let error_summary = error_chain
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown tool error".to_string());
    let original_details = if error_chain.len() <= 1 {
        error_summary.clone()
    } else {
        error_chain.join(" -> ")
    };

    let classified = classify_error(details.error);
    let structured = ToolExecutionError::with_original_error(
        details.name.to_string(),
        classified,
        error_summary.clone(),
        original_details.clone(),
    );
    let error_json = structured.to_json_value();
    let error_message = structured.message.clone();

    // MCP event
    if let Some(tool_name) = details.name.strip_prefix("mcp_") {
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
            Some(details.args_val.to_string()),
        );
        mcp_event.failure(Some(error_message.clone()));
        ctx.mcp_panel_state.add_event(mcp_event);
    }

    // Show friendly message
    let error_ctx =
        ToolErrorContext::new(details.name.to_string(), error_message.clone()).with_auto_recovery();
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

    // Render tool output with standardized view via the generic handler
    use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: error_json.clone(),
        stdout: None,
        modified_files: vec![],
        command_success: false,
        has_more: false,
    });
    handle_pipeline_output(&mut *ctx, details.name, details.args_val, &outcome, vt_cfg).await?;

    let error_content = serde_json::to_string(&error_json).unwrap_or_else(|_| "{}".to_string());

    working_history.push(uni::Message::tool_response_with_origin(
        details.call_id.to_string(),
        error_content,
        details.name.to_string(),
    ));
    let _ = last_tool_stdout.take();

    crate::agent::runloop::unified::tool_ledger::record_outcome_for_decision(
        ctx.decision_ledger,
        details.dec_id,
        false,
        Some(error_message.clone()),
    )
    .await?;

    Ok(())
}
