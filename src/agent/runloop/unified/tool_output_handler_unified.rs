//! Unified tool output handler to eliminate code duplication
//!
//! This module provides a single, generic implementation for handling tool execution
//! outcomes that can be used by all the different context types.

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::tools::result_cache::ToolResultCache;
use vtcode_core::utils::ansi::AnsiRenderer;

use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::tool_summary::render_tool_call_summary_with_status;

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
            ctx.session_stats().record_tool(name);

            // Handle MCP events and rendering
            if let Some(tool_name) = name.strip_prefix("mcp_") {
                handle_mcp_tool_success(ctx, tool_name, args_val, output, modified_files).await?;
            } else {
                // Handle regular tool success
                handle_regular_tool_success(
                    ctx,
                    name,
                    args_val,
                    output,
                    stdout,
                    modified_files,
                    *command_success,
                    vt_config,
                    token_budget,
                    &mut any_write_effect,
                    &mut turn_modified_files,
                    &mut last_tool_stdout,
                ).await?;
            }
        }
        ToolExecutionStatus::Failure {
            error,
            stdout,
            modified_files,
            error_kind,
        } => {
            // Record tool usage (session stats)
            ctx.session_stats().record_tool(name);

            // Handle MCP tool failure
            if let Some(tool_name) = name.strip_prefix("mcp_") {
                handle_mcp_tool_failure(ctx, tool_name, args_val, error, error_kind).await?;
            } else {
                // Handle regular tool failure
                handle_regular_tool_failure(
                    ctx,
                    name,
                    args_val,
                    error,
                    stdout,
                    modified_files,
                    error_kind,
                    vt_config,
                    token_budget,
                    &mut any_write_effect,
                    &mut turn_modified_files,
                    &mut last_tool_stdout,
                ).await?;
            }
        }
        ToolExecutionStatus::Blocked { reason } => {
            // Record tool usage (session stats)
            ctx.session_stats().record_tool(name);

            handle_tool_blocked(
                ctx,
                name,
                args_val,
                reason,
                vt_config,
                token_budget,
            ).await?;
        }
    }

    Ok((any_write_effect, turn_modified_files, last_tool_stdout))
}

async fn handle_mcp_tool_success<C: ToolOutputContext>(
    ctx: &mut C,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    modified_files: &[PathBuf],
) -> Result<()> {
    let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
        "mcp".to_string(),
        tool_name.to_string(),
        Some(args_val.to_string()),
    );
    mcp_event.success(None);

    if let Ok(rendered) = render_tool_output(
        &format!("mcp_{}", tool_name),
        args_val,
        output,
        modified_files,
    ) {
        mcp_event.output = Some(rendered);
    }

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
    vt_config: Option<&VTCodeConfig>,
    token_budget: &TokenBudgetManager,
    any_write_effect: &mut bool,
    turn_modified_files: &mut Vec<PathBuf>,
    last_tool_stdout: &mut Option<String>,
) -> Result<()> {
    // Cache successful tool results
    if let Some(cache) = ctx.tool_result_cache() {
        if let Ok(mut cache_guard) = cache.write().await {
            cache_guard.cache_result(name, args_val, output);
        }
    }

    // Record trajectory
    ctx.traj().log_tool_call(
        working_history_len_estimate(),
        name,
        &serde_json::to_value(output).unwrap_or(serde_json::json!({})),
        true,
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
    let output_rendered = render_tool_call_summary_with_status(
        name,
        args_val,
        output,
        modified_files,
        command_success,
        vt_config,
        token_budget,
    )?;

    ctx.renderer().render_tool_output(&output_rendered);
    Ok(())
}

async fn handle_mcp_tool_failure<C: ToolOutputContext>(
    ctx: &mut C,
    tool_name: &str,
    args_val: &serde_json::Value,
    error: &str,
    error_kind: &crate::agent::runloop::unified::tool_pipeline::ToolErrorKind,
) -> Result<()> {
    let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
        "mcp".to_string(),
        tool_name.to_string(),
        Some(args_val.to_string()),
    );

    let error_json = serde_json::json!({ "error": error });
    mcp_event.failure(Some(error.to_string()));

    if let Ok(rendered) = render_tool_output(
        &format!("mcp_{}", tool_name),
        args_val,
        &error_json,
        &[],
    ) {
        mcp_event.output = Some(rendered);
    }

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
    error_kind: &crate::agent::runloop::unified::tool_pipeline::ToolErrorKind,
    vt_config: Option<&VTCodeConfig>,
    token_budget: &TokenBudgetManager,
    any_write_effect: &mut bool,
    turn_modified_files: &mut Vec<PathBuf>,
    last_tool_stdout: &mut Option<String>,
) -> Result<()> {
    // Record trajectory
    let error_json = serde_json::json!({ "error": error });
    ctx.traj().log_tool_call(
        working_history_len_estimate(),
        name,
        &error_json,
        false,
        false,
    );

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
    let output_rendered = render_tool_call_summary_with_status(
        name,
        args_val,
        &error_json,
        modified_files,
        false,
        vt_config,
        token_budget,
    )?;

    ctx.renderer().render_tool_output(&output_rendered);
    Ok(())
}

async fn handle_tool_blocked<C: ToolOutputContext>(
    ctx: &mut C,
    name: &str,
    args_val: &serde_json::Value,
    reason: &str,
    vt_config: Option<&VTCodeConfig>,
    token_budget: &TokenBudgetManager,
) -> Result<()> {
    // Record trajectory
    let blocked_message = format!("Tool '{}' was blocked: {}", name, reason);
    let blocked_json = serde_json::json!({ "blocked": blocked_message });
    
    ctx.traj().log_tool_call(
        working_history_len_estimate(),
        name,
        &blocked_json,
        false,
        false,
    );

    // Render blocked output
    let output_rendered = render_tool_call_summary_with_status(
        name,
        args_val,
        &blocked_json,
        &[],
        false,
        vt_config,
        token_budget,
    )?;

    ctx.renderer().render_tool_output(&output_rendered);
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