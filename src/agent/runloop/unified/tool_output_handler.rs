use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::utils::ansi::MessageStyle;
// use vtcode_core::tools::result_cache::CacheKey; // Not used yet

use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::tool_summary::render_tool_call_summary_with_status;

pub(crate) async fn handle_pipeline_output(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_cfg: Option<&VTCodeConfig>,
    token_budget: &TokenBudgetManager,
) -> Result<(bool, Vec<PathBuf>, Option<String>)> {
    let mut any_write_effect = false;
    let mut turn_modified_files: Vec<PathBuf> = Vec::new();
    let mut last_tool_stdout: Option<String> = None;

    match &outcome.status {
        ToolExecutionStatus::Success {
            output,
            stdout,
            modified_files,
            command_success,
            has_more: _,
        } => {
            // Record tool usage (session stats)
            ctx.session_stats.record_tool(name);

            // Handle MCP events and rendering
            if let Some(tool_name) = name.strip_prefix("mcp_") {
                let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
                    "mcp".to_string(),
                    tool_name.to_string(),
                    Some(args_val.to_string()),
                );
                mcp_event.success(None);
                ctx.mcp_panel_state.add_event(mcp_event);
            } else {
                let exit_code = output.get("exit_code").and_then(|v| v.as_i64());
                let status_icon = if *command_success { "✓" } else { "✗" };
                render_tool_call_summary_with_status(
                    ctx.renderer,
                    name,
                    &args_val,
                    status_icon,
                    exit_code,
                )?;
            }

            render_tool_output(
                ctx.renderer,
                Some(name),
                &output,
                vt_cfg,
                Some(token_budget),
            )
            .await?;
            last_tool_stdout = if *command_success {
                stdout.clone()
            } else {
                None
            };

            if matches!(
                name,
                "write_file" | "edit_file" | "create_file" | "delete_file"
            ) {
                any_write_effect = true;
            }

            if !modified_files.is_empty() {
                for file in modified_files.iter() {
                    turn_modified_files.push(PathBuf::from(file));
                    // invalidate cache for modified files
                    let mut cache = ctx.tool_result_cache.write().await;
                    cache.invalidate_for_path(file);
                }
            }
        }
        ToolExecutionStatus::Failure { error } => {
            let err_msg = format!("Tool '{}' failure: {:?}", name, error);
            ctx.renderer.line(MessageStyle::Error, &err_msg)?;
        }
        ToolExecutionStatus::Timeout { error } => {
            let err_msg = format!("Tool '{}' timed out: {:?}", name, error);
            ctx.renderer.line(MessageStyle::Error, &err_msg)?;
        }
        ToolExecutionStatus::Cancelled => {
            ctx.renderer
                .line(MessageStyle::Info, "Tool execution cancelled")?;
        }
        ToolExecutionStatus::Progress(_) => {
            // progress handled in pipeline
        }
    }

    Ok((any_write_effect, turn_modified_files, last_tool_stdout))
}
