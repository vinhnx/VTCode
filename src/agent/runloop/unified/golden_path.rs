//! Golden Path Integration Module
//!
//! Bridges the runloop's tool execution to the unified tool execution components:
//! - `UnifiedToolExecutor` - Single entry point for tool execution
//! - `ToolInvocationId` - Unique correlation ID for tracking
//! - `SafetyGateway` - Centralized safety decisions
//! - `ParallelToolBatch` - Batch execution with concurrency control
//!
//! This module provides adapter functions that allow incremental adoption:
//! existing runloop code continues working while new code can use the golden path.
//!
//! All functions are `pub` for use by other modules during migration.

#![allow(dead_code)]

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use super::run_loop_context::RunLoopContext;
use vtcode_core::tools::unified_error::UnifiedToolError;
use vtcode_core::tools::{
    ExecutionContextBuilder, InvocationBuilder, ParallelToolBatch, SafetyDecision, SafetyGateway,
    SafetyGatewayConfig, ToolInvocationId, ToolRegistryAdapter, TrustLevel,
    UnifiedExecutionContext, UnifiedExecutionResult, UnifiedToolExecutor, UnifiedToolInvocation,
};

/// Create a new tool invocation from runloop context.
///
/// Generates a unique `ToolInvocationId` and populates the invocation
/// with session metadata from the context.
pub fn create_invocation(
    ctx: &RunLoopContext<'_>,
    tool_name: &str,
    args: &Value,
) -> UnifiedToolInvocation {
    let session_id = ctx.harness_state.run_id.0.clone();
    let turn_id = ctx.harness_state.turn_id.0.clone();

    InvocationBuilder::new(tool_name)
        .args(args.clone())
        .session_id(format!("{}:{}", session_id, turn_id))
        .attempt(1)
        .build()
}

/// Create a tool execution context from runloop context.
///
/// Maps runloop state to the unified execution context:
/// - Session ID from harness state
/// - Trust level based on autonomous mode
/// - Turn number from tool call counter
/// - Plan mode enforcement from registry state
pub fn create_execution_context(
    ctx: &RunLoopContext<'_>,
    invocation: &UnifiedToolInvocation,
) -> UnifiedExecutionContext {
    let is_autonomous = ctx.session_stats.is_autonomous_mode();
    let is_plan_mode = ctx.tool_registry.is_plan_mode();

    let trust_level = if is_autonomous {
        TrustLevel::Elevated
    } else {
        TrustLevel::Standard
    };

    let mut builder = ExecutionContextBuilder::new(&invocation.session_id)
        .trust_level(trust_level)
        .turn(ctx.harness_state.tool_calls as u32)
        .metadata("turn_id", &ctx.harness_state.turn_id.0)
        .metadata("run_id", &ctx.harness_state.run_id.0);

    if is_plan_mode {
        builder = builder.plan_mode();
    }

    if let Some(timeout) = execution_timeout_from_context(ctx) {
        builder = builder.timeout(timeout);
    }

    builder.build()
}

/// Create a safety gateway configured from runloop context.
///
/// Configures the gateway with:
/// - Rate limits from harness state
/// - Plan mode from registry
/// - Command policy from config (if available)
pub fn create_safety_gateway(ctx: &RunLoopContext<'_>) -> SafetyGateway {
    let config = SafetyGatewayConfig {
        max_per_turn: ctx.harness_state.max_tool_calls,
        max_per_session: 1000, // Default, can be overridden
        rate_limit_per_second: 5,
        rate_limit_per_minute: None,
        plan_mode_active: ctx.tool_registry.is_plan_mode(),
        workspace_trust: vtcode_core::tools::WorkspaceTrust::Trusted,
        approval_risk_threshold: vtcode_core::tools::RiskLevel::Medium,
    };

    SafetyGateway::with_config(config)
}

/// Check safety for a tool call using the safety gateway.
///
/// Returns a `SafetyDecision` indicating whether the tool can proceed:
/// - `Allow` - Execution permitted
/// - `Deny(reason)` - Execution blocked
/// - `NeedsApproval(justification)` - User confirmation required
pub async fn check_safety(
    ctx: &RunLoopContext<'_>,
    tool_name: &str,
    args: &Value,
) -> SafetyDecision {
    let gateway = create_safety_gateway(ctx);
    let exec_ctx = UnifiedExecutionContext::new(&ctx.harness_state.run_id.0);

    gateway.check_safety(&exec_ctx, tool_name, args).await
}

/// Execute a tool via the golden path.
///
/// This is the unified entry point that:
/// 1. Creates an invocation with tracking ID
/// 2. Builds execution context from runloop state
/// 3. Delegates to `UnifiedToolExecutor` (via `ToolRegistryAdapter`)
///
/// Existing code can continue using `execute_tool_with_timeout_ref` while
/// new code can migrate to this function for better tracking and safety.
pub async fn execute_via_golden_path(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args: &Value,
) -> Result<Value, UnifiedToolError> {
    let invocation = create_invocation(ctx, name, args);
    let exec_ctx = create_execution_context(ctx, &invocation);

    tracing::debug!(
        target: "vtcode.golden_path",
        invocation_id = %invocation.id,
        tool = name,
        "executing via golden path"
    );

    let registry = ctx.tool_registry.clone();
    let adapter = ToolRegistryAdapter::new(registry);

    let result = adapter.execute(exec_ctx, name, args.clone()).await?;

    tracing::debug!(
        target: "vtcode.golden_path",
        invocation_id = %invocation.id,
        tool = name,
        duration_ms = result.duration.as_millis(),
        cached = result.was_cached,
        "golden path execution complete"
    );

    Ok(result.value)
}

/// Create a parallel tool batch from runloop context.
///
/// The batch can be used to execute multiple read-only tools in parallel,
/// or sequential execution for mutating tools.
pub fn create_parallel_batch(_ctx: &RunLoopContext<'_>) -> ParallelToolBatch {
    let max_concurrency = 8; // Default, can be configured
    ParallelToolBatch::with_concurrency(max_concurrency)
}

/// Add a tool call to a batch with context-derived execution context.
pub fn add_to_batch(
    batch: &mut ParallelToolBatch,
    ctx: &RunLoopContext<'_>,
    tool_name: &str,
    args: Value,
) {
    let invocation = create_invocation(ctx, tool_name, &args);
    let exec_ctx = create_execution_context(ctx, &invocation);
    batch.add_call(tool_name, args, exec_ctx);
}

/// Execute a batch of tools using the golden path.
///
/// Handles parallel/sequential execution based on tool safety:
/// - Read-only tools run in parallel
/// - Mutating tools run sequentially
pub async fn execute_batch_via_golden_path(
    ctx: &mut RunLoopContext<'_>,
    batch: ParallelToolBatch,
) -> Vec<Result<UnifiedExecutionResult, UnifiedToolError>> {
    let registry = ctx.tool_registry.clone();
    let adapter = ToolRegistryAdapter::new(registry);

    batch.execute_batch(&adapter).await
}

/// Get the invocation ID for correlation/logging.
///
/// Creates a new ID if one doesn't exist for the current tool call.
pub fn get_or_create_invocation_id(
    ctx: &RunLoopContext<'_>,
    tool_name: &str,
    args: &Value,
) -> ToolInvocationId {
    let invocation = create_invocation(ctx, tool_name, args);
    invocation.id
}

/// Check if a tool is safe for parallel execution.
///
/// Delegates to `ParallelToolBatch::is_parallel_safe` for consistent behavior.
#[inline]
pub fn is_parallel_safe(tool_name: &str) -> bool {
    ParallelToolBatch::is_parallel_safe(tool_name)
}

fn execution_timeout_from_context(ctx: &RunLoopContext<'_>) -> Option<Duration> {
    let remaining = ctx
        .harness_state
        .max_tool_wall_clock
        .checked_sub(ctx.harness_state.turn_started_at.elapsed())?;

    if remaining.is_zero() {
        None
    } else {
        Some(remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_parallel_safe() {
        assert!(is_parallel_safe("read_file"));
        assert!(is_parallel_safe("list_files"));
        assert!(is_parallel_safe("grep_file"));
        assert!(!is_parallel_safe("write_file"));
        assert!(!is_parallel_safe("delete_file"));
        assert!(!is_parallel_safe("shell"));
    }
}
