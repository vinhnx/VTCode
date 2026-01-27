//! Golden Path Orchestrator - Consolidated Tool Execution Entry Point
//!
//! This module consolidates multiple execution paths into a single canonical entry point:
//! - `ToolRegistry.execute_tool` → `execute_golden_path`
//! - `ToolOrchestrator.run` → `execute_golden_path`
//! - `OptimizedToolRegistry.execute_tool_optimized` → `execute_golden_path`
//!
//! All tool execution now flows through these functions, enabling:
//! - Single source of truth for policy enforcement
//! - Consistent `ToolInvocationId` correlation across the pipeline
//! - Unified error handling via `UnifiedToolError`
//! - Trust level enforcement at the executor level

use std::time::{Duration, Instant};

use serde_json::Value;
use tracing::{debug, warn};

use crate::tools::invocation::{InvocationBuilder, ToolInvocation, ToolInvocationId};
use crate::tools::registry::{RiskLevel, ToolRegistry, WorkspaceTrust};
use crate::tools::safety_gateway::{SafetyDecision, SafetyGateway, SafetyGatewayConfig};
use crate::tools::unified_error::{UnifiedErrorKind, UnifiedToolError};
use crate::tools::unified_executor::{ApprovalState, ToolExecutionContext, TrustLevel};

/// Execution result with full correlation metadata
#[derive(Debug, Clone)]
pub struct GoldenPathResult {
    /// Unique invocation ID for correlation
    pub invocation_id: ToolInvocationId,
    /// Tool output value
    pub value: Value,
    /// Execution duration
    pub duration: Duration,
    /// Whether result was served from cache
    pub was_cached: bool,
    /// Approval state at execution time
    pub approval_state: ApprovalState,
    /// Trust level used for execution
    pub trust_level: TrustLevel,
}

/// Configuration for golden path execution
#[derive(Debug, Clone)]
pub struct GoldenPathConfig {
    /// Default trust level for new executions
    pub default_trust_level: TrustLevel,
    /// Maximum concurrent executions
    pub max_concurrency: usize,
    /// Whether to enforce plan mode globally
    pub plan_mode_enforced: bool,
    /// Maximum tool calls per turn
    pub max_per_turn: Option<usize>,
    /// Maximum tool calls per session
    pub max_per_session: Option<usize>,
    /// Default execution timeout
    pub default_timeout: Option<Duration>,
}

impl Default for GoldenPathConfig {
    fn default() -> Self {
        Self {
            default_trust_level: TrustLevel::Standard,
            max_concurrency: 8,
            plan_mode_enforced: false,
            max_per_turn: Some(100),
            max_per_session: Some(1000),
            default_timeout: Some(Duration::from_secs(300)),
        }
    }
}

/// Execute a tool via the golden path
///
/// This is the canonical entry point for all tool execution. It:
/// 1. Creates a `ToolInvocationId` for correlation
/// 2. Checks safety via the `SafetyGateway`
/// 3. Enforces trust level requirements
/// 4. Delegates to the registry for execution
/// 5. Returns a `GoldenPathResult` with full metadata
pub async fn execute_golden_path(
    registry: &ToolRegistry,
    tool_name: &str,
    args: Value,
    ctx: &ToolExecutionContext,
    config: &GoldenPathConfig,
) -> Result<GoldenPathResult, UnifiedToolError> {
    let invocation = create_invocation(tool_name, &args, ctx);
    let start = Instant::now();

    debug!(
        target: "vtcode.golden_path",
        invocation_id = %invocation.id,
        tool = tool_name,
        trust_level = ?ctx.trust_level,
        "golden path execution starting"
    );

    // 1. Safety check
    let safety_gateway = create_safety_gateway(config);
    let safety_decision = safety_gateway.check_safety(ctx, tool_name, &args).await;

    let approval_state = match &safety_decision {
        SafetyDecision::Allow => ApprovalState::PreApproved {
            reason: "Safety check passed".to_string(),
        },
        SafetyDecision::Deny(reason) => {
            return Err(create_error(
                UnifiedErrorKind::PolicyViolation,
                format!("Safety check denied: {}", reason),
                tool_name,
                &invocation.id,
                start.elapsed(),
            ));
        }
        SafetyDecision::NeedsApproval(justification) => {
            if ctx.trust_level.can_bypass_approval() {
                ApprovalState::PreApproved {
                    reason: format!("Elevated trust: {}", justification),
                }
            } else {
                ApprovalState::NeedsApproval
            }
        }
    };

    // 2. Check plan mode enforcement
    if ctx.policy_config.plan_mode_enforced
        && is_mutating_tool(tool_name)
        && !ctx.trust_level.can_mutate()
    {
        return Err(create_error(
            UnifiedErrorKind::PlanModeViolation,
            format!("Tool '{}' is mutating but plan mode is active", tool_name),
            tool_name,
            &invocation.id,
            start.elapsed(),
        ));
    }

    // 3. Execute via registry
    let result = registry.execute_tool_ref(tool_name, &args).await;

    let duration = start.elapsed();

    match result {
        Ok(value) => {
            debug!(
                target: "vtcode.golden_path",
                invocation_id = %invocation.id,
                tool = tool_name,
                duration_ms = duration.as_millis(),
                "golden path execution complete"
            );

            Ok(GoldenPathResult {
                invocation_id: invocation.id,
                value,
                duration,
                was_cached: false,
                approval_state,
                trust_level: ctx.trust_level,
            })
        }
        Err(e) => {
            warn!(
                target: "vtcode.golden_path",
                invocation_id = %invocation.id,
                tool = tool_name,
                error = %e,
                "golden path execution failed"
            );

            Err(create_error(
                UnifiedErrorKind::ExecutionFailed,
                e.to_string(),
                tool_name,
                &invocation.id,
                duration,
            ))
        }
    }
}

/// Execute a tool with default configuration
pub async fn execute_golden_path_simple(
    registry: &ToolRegistry,
    tool_name: &str,
    args: Value,
    session_id: &str,
) -> Result<GoldenPathResult, UnifiedToolError> {
    let ctx = ToolExecutionContext::new(session_id);
    let config = GoldenPathConfig::default();
    execute_golden_path(registry, tool_name, args, &ctx, &config).await
}

/// Execute multiple tools in batch
///
/// Read-only tools execute in parallel, mutating tools execute sequentially.
pub async fn execute_batch_golden_path(
    registry: &ToolRegistry,
    calls: Vec<(&str, Value)>,
    ctx: &ToolExecutionContext,
    config: &GoldenPathConfig,
) -> Vec<Result<GoldenPathResult, UnifiedToolError>> {
    let mut results = Vec::with_capacity(calls.len());

    // Partition into read-only and mutating
    let (read_only, mutating): (Vec<_>, Vec<_>) = calls
        .into_iter()
        .partition(|(name, _)| !is_mutating_tool(name));

    // Execute read-only tools in parallel
    if !read_only.is_empty() {
        let futures: Vec<_> = read_only
            .into_iter()
            .map(|(name, args)| {
                let ctx = ctx.for_retry();
                async move { execute_golden_path(registry, name, args, &ctx, config).await }
            })
            .collect();

        let parallel_results = futures::future::join_all(futures).await;
        results.extend(parallel_results);
    }

    // Execute mutating tools sequentially
    for (name, args) in mutating {
        let ctx = ctx.for_retry();
        let result = execute_golden_path(registry, name, args, &ctx, config).await;
        results.push(result);
    }

    results
}

/// Create a tool invocation with unique ID
fn create_invocation(tool_name: &str, args: &Value, ctx: &ToolExecutionContext) -> ToolInvocation {
    InvocationBuilder::new(tool_name)
        .args(args.clone())
        .session_id(&ctx.session_id)
        .attempt(ctx.attempt)
        .build()
}

/// Create a safety gateway with the given configuration
fn create_safety_gateway(config: &GoldenPathConfig) -> SafetyGateway {
    let gateway_config = SafetyGatewayConfig {
        max_per_turn: config.max_per_turn.unwrap_or(100),
        max_per_session: config.max_per_session.unwrap_or(1000),
        rate_limit_per_second: 5,
        rate_limit_per_minute: None,
        plan_mode_active: config.plan_mode_enforced,
        workspace_trust: WorkspaceTrust::Trusted,
        approval_risk_threshold: RiskLevel::Medium,
    };

    SafetyGateway::with_config(gateway_config)
}

/// Check if a tool is mutating (modifies state)
fn is_mutating_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file"
            | "create_file"
            | "apply_patch"
            | "delete_file"
            | "shell_command"
            | "bash"
            | "run_pty_cmd"
            | "write_to_pty"
    )
}

/// Create a unified error with context
fn create_error(
    kind: UnifiedErrorKind,
    message: String,
    tool_name: &str,
    invocation_id: &ToolInvocationId,
    duration: Duration,
) -> UnifiedToolError {
    UnifiedToolError::new(kind, message)
        .with_tool_name(tool_name)
        .with_invocation_id(*invocation_id)
        .with_duration(duration)
}

/// Builder for customized golden path execution
pub struct ExecutionBuilder<'a> {
    registry: &'a ToolRegistry,
    tool_name: String,
    args: Value,
    trust_level: TrustLevel,
    timeout: Option<Duration>,
    plan_mode: bool,
    session_id: String,
}

impl<'a> ExecutionBuilder<'a> {
    /// Create a new execution builder
    pub fn new(registry: &'a ToolRegistry, tool_name: impl Into<String>) -> Self {
        Self {
            registry,
            tool_name: tool_name.into(),
            args: Value::Null,
            trust_level: TrustLevel::Standard,
            timeout: None,
            plan_mode: false,
            session_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Set the tool arguments
    pub fn args(mut self, args: Value) -> Self {
        self.args = args;
        self
    }

    /// Set the trust level
    pub fn trust_level(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    /// Set elevated trust (for internal/autonomous operations)
    pub fn elevated(self) -> Self {
        self.trust_level(TrustLevel::Elevated)
    }

    /// Set full trust (for system operations)
    pub fn full_trust(self) -> Self {
        self.trust_level(TrustLevel::Full)
    }

    /// Set execution timeout
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Enable plan mode (read-only)
    pub fn plan_mode(mut self, enabled: bool) -> Self {
        self.plan_mode = enabled;
        self
    }

    /// Set session ID for correlation
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = id.into();
        self
    }

    /// Execute the tool
    pub async fn execute(self) -> Result<GoldenPathResult, UnifiedToolError> {
        let mut ctx = ToolExecutionContext::new(&self.session_id);
        ctx.trust_level = self.trust_level;
        ctx.policy_config.timeout = self.timeout;
        ctx.policy_config.plan_mode_enforced = self.plan_mode;

        let config = GoldenPathConfig {
            default_trust_level: self.trust_level,
            plan_mode_enforced: self.plan_mode,
            default_timeout: self.timeout,
            ..Default::default()
        };

        execute_golden_path(self.registry, &self.tool_name, self.args, &ctx, &config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_path_config_default() {
        let config = GoldenPathConfig::default();
        assert_eq!(config.default_trust_level, TrustLevel::Standard);
        assert_eq!(config.max_concurrency, 8);
        assert!(!config.plan_mode_enforced);
    }

    #[test]
    fn test_is_mutating_tool() {
        assert!(is_mutating_tool("write_file"));
        assert!(is_mutating_tool("bash"));
        assert!(is_mutating_tool("apply_patch"));
        assert!(!is_mutating_tool("read_file"));
        assert!(!is_mutating_tool("grep_file"));
        assert!(!is_mutating_tool("list_files"));
    }

    #[test]
    fn test_trust_level_can_bypass_approval() {
        assert!(!TrustLevel::Untrusted.can_bypass_approval());
        assert!(!TrustLevel::Standard.can_bypass_approval());
        assert!(TrustLevel::Elevated.can_bypass_approval());
        assert!(TrustLevel::Full.can_bypass_approval());
    }

    #[test]
    fn test_trust_level_can_mutate() {
        assert!(!TrustLevel::Untrusted.can_mutate());
        assert!(TrustLevel::Standard.can_mutate());
        assert!(TrustLevel::Elevated.can_mutate());
        assert!(TrustLevel::Full.can_mutate());
    }
}
