//! Unified tool output handler to eliminate code duplication
//!
//! This module provides a single, generic implementation for handling tool execution
//! outcomes that can be used by all the different context types.
#![allow(dead_code, clippy::too_many_arguments)]

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::tools::result_cache::ToolResultCache;
use vtcode_core::utils::ansi::AnsiRenderer;

use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::tool_summary::{
    render_tool_call_summary, stream_label_from_output,
};

/// Generic context trait that provides the common functionality needed for tool output handling
pub trait ToolOutputContext {
    fn session_stats(&mut self) -> &mut SessionStats;
    fn mcp_panel_state(&mut self) -> &mut McpPanelState;
    fn tool_result_cache(&self) -> Option<&Arc<RwLock<ToolResultCache>>>;
    fn decision_ledger(&self) -> Option<&Arc<RwLock<DecisionTracker>>>;
    fn renderer(&mut self) -> &mut AnsiRenderer;
    fn traj(&self) -> &TrajectoryLogger;
}

/// Unified tool output handler that eliminates code duplication across different contexts
pub async fn handle_tool_outcome_unified<C: ToolOutputContext>(
    ctx: &mut C,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_config: Option<&VTCodeConfig>,
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
            // Handle success case
            // Record tool usage (session stats)
            ctx.session_stats().record_tool(name);

            // Convert modified_files from Vec<String> to Vec<PathBuf>
            let modified_files_pathbuf: Vec<PathBuf> =
                modified_files.iter().map(PathBuf::from).collect();

            // Create a local copy to avoid borrowing issues
            let _command_success_val = *command_success;

            // Handle MCP events and rendering
            if let Some(tool_name) = name.strip_prefix("mcp_") {
                handle_mcp_tool_success(
                    ctx,
                    tool_name,
                    args_val,
                    output,
                    &modified_files_pathbuf,
                    vt_config,
                )
                .await?;
            } else {
                // Handle regular tool success
                handle_regular_tool_success(
                    ctx,
                    name,
                    args_val,
                    output,
                    stdout,
                    &modified_files_pathbuf,
                    *command_success,
                    vt_config,
                    &mut any_write_effect,
                    &mut turn_modified_files,
                    &mut last_tool_stdout,
                )
                .await?;
            }
        }
        ToolExecutionStatus::Failure { error } => {
            // Handle failure case
            // Record tool usage (session stats)
            ctx.session_stats().record_tool(name);

            // Handle MCP tool failure
            if let Some(tool_name) = name.strip_prefix("mcp_") {
                handle_mcp_tool_failure(
                    ctx,
                    tool_name,
                    args_val,
                    &error.to_string(),
                    &vtcode_core::tools::registry::classify_error(error),
                    vt_config,
                )
                .await?;
            } else {
                // Handle regular tool failure
                handle_regular_tool_failure(
                    ctx,
                    name,
                    args_val,
                    &error.to_string(),
                    &None,
                    &[],
                    &vtcode_core::tools::registry::classify_error(error),
                    vt_config,
                    &mut any_write_effect,
                    &mut turn_modified_files,
                    &mut last_tool_stdout,
                )
                .await?;
            }
        }
        ToolExecutionStatus::Timeout { error } => {
            // Handle timeout case - treat as failure
            ctx.session_stats().record_tool(name);

            if let Some(tool_name) = name.strip_prefix("mcp_") {
                handle_mcp_tool_failure(
                    ctx,
                    tool_name,
                    args_val,
                    &error.message,
                    &vtcode_core::tools::registry::ToolErrorType::Timeout,
                    vt_config,
                )
                .await?;
            } else {
                handle_regular_tool_failure(
                    ctx,
                    name,
                    args_val,
                    &error.message,
                    &None,
                    &[],
                    &vtcode_core::tools::registry::ToolErrorType::Timeout,
                    vt_config,
                    &mut any_write_effect,
                    &mut turn_modified_files,
                    &mut last_tool_stdout,
                )
                .await?;
            }
        }
        ToolExecutionStatus::Cancelled => {
            // Handle cancelled case - treat as failure
            ctx.session_stats().record_tool(name);

            let error_msg = "Tool execution was cancelled";
            if let Some(tool_name) = name.strip_prefix("mcp_") {
                handle_mcp_tool_failure(
                    ctx,
                    tool_name,
                    args_val,
                    error_msg,
                    &vtcode_core::tools::registry::ToolErrorType::ExecutionError,
                    vt_config,
                )
                .await?;
            } else {
                handle_regular_tool_failure(
                    ctx,
                    name,
                    args_val,
                    error_msg,
                    &None,
                    &[],
                    &vtcode_core::tools::registry::ToolErrorType::ExecutionError,
                    vt_config,
                    &mut any_write_effect,
                    &mut turn_modified_files,
                    &mut last_tool_stdout,
                )
                .await?;
            }
        }

    }

    Ok((any_write_effect, turn_modified_files, last_tool_stdout))
}

async fn handle_mcp_tool_success<C: ToolOutputContext>(
    ctx: &mut C,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    _modified_files: &[PathBuf],
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
        "mcp".to_string(),
        tool_name.to_string(),
        Some(args_val.to_string()),
    );
    mcp_event.success(None);

    render_tool_output(
        ctx.renderer(),
        Some(&format!("mcp_{}", tool_name)),
        output,
        vt_config,
    )
    .await?;
    mcp_event.success(None);

    ctx.mcp_panel_state().add_event(mcp_event);
    Ok(())
}

async fn handle_regular_tool_success<C: ToolOutputContext>(
    ctx: &mut C,
    name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    stdout: &Option<String>,
    modified_files: &[PathBuf],
    command_success: bool,
    _vt_config: Option<&VTCodeConfig>,
    any_write_effect: &mut bool,
    turn_modified_files: &mut Vec<PathBuf>,
    last_tool_stdout: &mut Option<String>,
) -> Result<()> {
    // Cache successful tool results
    if let Some(cache) = ctx.tool_result_cache() {
        let mut cache_guard = cache.write().await;
        let output_str = serde_json::to_string(output).unwrap_or_default();
        let cache_key =
            vtcode_core::tools::result_cache::ToolCacheKey::from_json(name, args_val, "");
        cache_guard.insert(cache_key, output_str);
    }

    // Record trajectory
    ctx.traj().log_tool_call(
        working_history_len_estimate(),
        name,
        &serde_json::to_value(output).unwrap_or(serde_json::json!({})),
        command_success,
    );

    // Handle modified files
    if !modified_files.is_empty() {
        *any_write_effect = true;
        turn_modified_files.extend(modified_files.iter().cloned());
    }

    // Store stdout for potential follow-up processing
    if let Some(stdout_text) = stdout {
        *last_tool_stdout = Some(stdout_text.clone());
    }

    // Render output with appropriate styling
    let stream_label = stream_label_from_output(output, command_success);
    render_tool_call_summary(ctx.renderer(), name, args_val, stream_label)?;
    Ok(())
}

async fn handle_mcp_tool_failure<C: ToolOutputContext>(
    ctx: &mut C,
    tool_name: &str,
    args_val: &serde_json::Value,
    error: &str,
    _error_kind: &vtcode_core::tools::registry::ToolErrorType,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
        "mcp".to_string(),
        tool_name.to_string(),
        Some(args_val.to_string()),
    );

    let error_json = serde_json::json!({ "error": error });
    mcp_event.failure(Some(error.to_string()));

    render_tool_output(
        ctx.renderer(),
        Some(&format!("mcp_{}", tool_name)),
        &error_json,
        vt_config,
    )
    .await?;
    mcp_event.success(None);

    ctx.mcp_panel_state().add_event(mcp_event);
    Ok(())
}

async fn handle_regular_tool_failure<C: ToolOutputContext>(
    ctx: &mut C,
    name: &str,
    args_val: &serde_json::Value,
    error: &str,
    stdout: &Option<String>,
    modified_files: &[PathBuf],
    _error_kind: &vtcode_core::tools::registry::ToolErrorType,
    _vt_config: Option<&VTCodeConfig>,
    any_write_effect: &mut bool,
    turn_modified_files: &mut Vec<PathBuf>,
    last_tool_stdout: &mut Option<String>,
) -> Result<()> {
    // Record trajectory
    let error_json = serde_json::json!({ "error": error });
    ctx.traj()
        .log_tool_call(working_history_len_estimate(), name, &error_json, false);

    // Handle modified files (even on failure, some files might have been changed)
    if !modified_files.is_empty() {
        *any_write_effect = true;
        turn_modified_files.extend(modified_files.iter().cloned());
    }

    // Store stdout for potential follow-up processing
    if let Some(stdout_text) = stdout {
        *last_tool_stdout = Some(stdout_text.clone());
    }

    // Render error output
    let stream_label = stream_label_from_output(&error_json, false);
    render_tool_call_summary(ctx.renderer(), name, args_val, stream_label)?;
    Ok(())
}

async fn handle_tool_blocked<C: ToolOutputContext>(
    ctx: &mut C,
    name: &str,
    args_val: &serde_json::Value,
    reason: &str,
    _vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    // Record trajectory
    let blocked_message = format!("Tool '{}' was blocked: {}", name, reason);
    let blocked_json = serde_json::json!({ "blocked": blocked_message });

    ctx.traj()
        .log_tool_call(working_history_len_estimate(), name, &blocked_json, false);

    // Render blocked output
    let stream_label = stream_label_from_output(&blocked_json, false);
    render_tool_call_summary(ctx.renderer(), name, args_val, stream_label)?;
    Ok(())
}

// Helper function to estimate working history length when we don't have direct access
fn working_history_len_estimate() -> usize {
    // This is a rough estimate - in practice, we might want to pass this through
    // the context or use a different approach
    0
}

// Implementations for the specific context types
impl ToolOutputContext for crate::agent::runloop::unified::run_loop_context::RunLoopContext<'_> {
    fn session_stats(&mut self) -> &mut SessionStats {
        self.session_stats
    }

    fn mcp_panel_state(&mut self) -> &mut McpPanelState {
        self.mcp_panel_state
    }

    fn tool_result_cache(&self) -> Option<&Arc<RwLock<ToolResultCache>>> {
        Some(self.tool_result_cache)
    }

    fn decision_ledger(&self) -> Option<&Arc<RwLock<DecisionTracker>>> {
        Some(self.decision_ledger)
    }

    fn renderer(&mut self) -> &mut AnsiRenderer {
        self.renderer
    }

    fn traj(&self) -> &TrajectoryLogger {
        self.traj
    }
}

// Implementation for TurnLoopContext - this one needs special handling
pub struct TurnContextAdapter<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub tool_result_cache: Option<&'a Arc<RwLock<ToolResultCache>>>,
    pub decision_ledger: Option<&'a Arc<RwLock<DecisionTracker>>>,
    pub traj: &'a TrajectoryLogger,
}

impl<'a> ToolOutputContext for TurnContextAdapter<'a> {
    fn session_stats(&mut self) -> &mut SessionStats {
        self.session_stats
    }

    fn mcp_panel_state(&mut self) -> &mut McpPanelState {
        self.mcp_panel_state
    }

    fn tool_result_cache(&self) -> Option<&Arc<RwLock<ToolResultCache>>> {
        self.tool_result_cache
    }

    fn decision_ledger(&self) -> Option<&Arc<RwLock<DecisionTracker>>> {
        self.decision_ledger
    }

    fn renderer(&mut self) -> &mut AnsiRenderer {
        self.renderer
    }

    fn traj(&self) -> &TrajectoryLogger {
        self.traj
    }
}
