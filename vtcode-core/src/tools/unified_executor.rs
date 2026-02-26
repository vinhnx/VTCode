//! Golden Path Enforcement: Unified Tool Executor Interface
//!
//! This module provides a single entry point for all tool execution paths,
//! consolidating the multiple execution patterns in the codebase:
//! - `ToolRegistry.execute_tool`
//! - `ToolOrchestrator.run`
//! - `OptimizedToolRegistry.execute_tool_optimized`
//!
//! All tool execution flows through `UnifiedToolExecutor`, enabling:
//! - Consistent approval/policy enforcement
//! - Unified error handling via `UnifiedToolError`
//! - Centralized telemetry and audit logging
//! - Trust level enforcement across all execution paths

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::SandboxPolicy;
use crate::tools::registry::ToolRegistry;
use crate::tools::traits::ToolExecutor;
use crate::tools::unified_error::{DebugContext, UnifiedErrorKind, UnifiedToolError};

/// Trust level for tool execution context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrustLevel {
    /// Untrusted context (e.g., user input, external MCP)
    Untrusted,
    /// Standard trust (default for most operations)
    #[default]
    Standard,
    /// Elevated trust (e.g., internal subagent, pre-approved patterns)
    Elevated,
    /// Full trust (e.g., system tools, fully autonomous mode)
    Full,
}

impl TrustLevel {
    /// Whether this trust level allows bypassing approval prompts
    #[inline]
    pub const fn can_bypass_approval(&self) -> bool {
        matches!(self, TrustLevel::Elevated | TrustLevel::Full)
    }

    /// Whether this trust level allows mutating operations
    #[inline]
    pub const fn can_mutate(&self) -> bool {
        !matches!(self, TrustLevel::Untrusted)
    }
}

/// Approval state for tool execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalState {
    /// Not yet evaluated
    Pending,
    /// Pre-approved (cached or pattern-matched)
    PreApproved { reason: String },
    /// Requires user confirmation
    NeedsApproval,
    /// User approved this invocation
    Approved,
    /// User denied this invocation
    Denied { reason: String },
    /// Policy blocked (never prompt)
    Blocked { reason: String },
}

impl ApprovalState {
    /// Whether execution can proceed
    #[inline]
    pub fn can_proceed(&self) -> bool {
        matches!(
            self,
            ApprovalState::PreApproved { .. } | ApprovalState::Approved
        )
    }

    /// Whether this state is terminal (no further evaluation needed)
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ApprovalState::Approved | ApprovalState::Denied { .. } | ApprovalState::Blocked { .. }
        )
    }
}

/// Policy configuration for tool execution
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// Base policy from tool definition
    pub base_policy: ToolPolicy,
    /// Sandbox policy for command execution
    pub sandbox_policy: Option<SandboxPolicy>,
    /// Allow patterns that auto-approve
    pub allow_patterns: Vec<String>,
    /// Deny patterns that block execution
    pub deny_patterns: Vec<String>,
    /// Whether to enforce plan-mode (read-only)
    pub plan_mode_enforced: bool,
    /// Maximum execution timeout
    pub timeout: Option<Duration>,
    /// Custom policy overrides
    pub overrides: HashMap<String, Value>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            base_policy: ToolPolicy::Prompt,
            sandbox_policy: None,
            allow_patterns: Vec::new(),
            deny_patterns: Vec::new(),
            plan_mode_enforced: false,
            timeout: None,
            overrides: HashMap::new(),
        }
    }
}

/// Execution context for unified tool execution
///
/// Contains all metadata needed for policy evaluation, audit logging,
/// and error context propagation.
#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    /// Trust level for this execution
    pub trust_level: TrustLevel,
    /// Current approval state
    pub approval_state: ApprovalState,
    /// Policy configuration
    pub policy_config: PolicyConfig,
    /// Unique invocation ID for correlation
    pub invocation_id: String,
    /// Session ID for grouping related calls
    pub session_id: String,
    /// Parent invocation ID (for subagent chains)
    pub parent_invocation_id: Option<String>,
    /// Turn number in conversation
    pub turn_number: Option<u32>,
    /// Attempt number (for retries)
    pub attempt: u32,
    /// Timestamp when context was created
    pub created_at: Instant,
    /// Additional metadata for telemetry
    pub metadata: HashMap<String, String>,
}

impl ToolExecutionContext {
    /// Create a new execution context with generated IDs
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            trust_level: TrustLevel::default(),
            approval_state: ApprovalState::Pending,
            policy_config: PolicyConfig::default(),
            invocation_id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            parent_invocation_id: None,
            turn_number: None,
            attempt: 1,
            created_at: Instant::now(),
            metadata: HashMap::new(),
        }
    }

    /// Create context for a retry attempt
    pub fn for_retry(&self) -> Self {
        Self {
            trust_level: self.trust_level,
            approval_state: ApprovalState::Pending,
            policy_config: self.policy_config.clone(),
            invocation_id: Uuid::new_v4().to_string(),
            session_id: self.session_id.clone(),
            parent_invocation_id: Some(self.invocation_id.clone()),
            turn_number: self.turn_number,
            attempt: self.attempt + 1,
            created_at: Instant::now(),
            metadata: self.metadata.clone(),
        }
    }

    /// Create context for a subagent call
    pub fn for_subagent(&self, trust_level: TrustLevel) -> Self {
        Self {
            trust_level,
            approval_state: ApprovalState::Pending,
            policy_config: self.policy_config.clone(),
            invocation_id: Uuid::new_v4().to_string(),
            session_id: self.session_id.clone(),
            parent_invocation_id: Some(self.invocation_id.clone()),
            turn_number: self.turn_number,
            attempt: 1,
            created_at: Instant::now(),
            metadata: self.metadata.clone(),
        }
    }

    /// Set trust level
    pub fn with_trust_level(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    /// Set policy config
    pub fn with_policy(mut self, config: PolicyConfig) -> Self {
        self.policy_config = config;
        self
    }

    /// Set turn number
    pub fn with_turn(mut self, turn: u32) -> Self {
        self.turn_number = Some(turn);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Convert to debug context for error reporting
    pub fn to_debug_context(&self, tool_name: &str) -> DebugContext {
        DebugContext {
            tool_name: tool_name.to_string(),
            invocation_id: Some(self.invocation_id.clone()),
            attempt: self.attempt,
            metadata: self
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// Elapsed time since context creation
    pub fn elapsed(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// Result of unified tool execution
#[derive(Debug)]
pub struct UnifiedExecutionResult {
    /// The execution result value
    pub value: Value,
    /// Final approval state after execution
    pub approval_state: ApprovalState,
    /// Execution duration
    pub duration: Duration,
    /// Whether result was cached
    pub was_cached: bool,
    /// Metadata from execution
    pub metadata: HashMap<String, Value>,
}

impl UnifiedExecutionResult {
    /// Create a successful result
    pub fn success(value: Value, ctx: &ToolExecutionContext) -> Self {
        Self {
            value,
            approval_state: ctx.approval_state.clone(),
            duration: ctx.elapsed(),
            was_cached: false,
            metadata: HashMap::new(),
        }
    }

    /// Mark result as cached
    pub fn with_cached(mut self, cached: bool) -> Self {
        self.was_cached = cached;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

/// Unified Tool Executor trait - the golden path for all tool execution
///
/// All tool execution paths should implement this trait to ensure:
/// - Consistent policy enforcement
/// - Unified error handling
/// - Centralized audit logging
/// - Trust level propagation
#[async_trait]
pub trait UnifiedToolExecutor: Send + Sync {
    /// Execute a tool through the unified path
    ///
    /// This is the single entry point for all tool execution. Implementors
    /// must ensure:
    /// 1. Policy evaluation based on context
    /// 2. Approval state management
    /// 3. Error classification via UnifiedToolError
    /// 4. Telemetry/audit logging
    async fn execute(
        &self,
        ctx: ToolExecutionContext,
        name: &str,
        args: Value,
    ) -> Result<UnifiedExecutionResult, UnifiedToolError>;

    /// Check if a tool exists without executing
    async fn has_tool(&self, name: &str) -> bool;

    /// List available tools for the given trust level
    async fn available_tools(&self, trust_level: TrustLevel) -> Vec<String>;

    /// Pre-flight check: evaluate policy without execution
    ///
    /// Returns the approval state that would be used if execute() were called.
    async fn preflight(
        &self,
        ctx: &ToolExecutionContext,
        name: &str,
        args: &Value,
    ) -> Result<ApprovalState, UnifiedToolError>;
}

/// Adapter to expose ToolRegistry through the UnifiedToolExecutor interface
pub struct ToolRegistryAdapter {
    registry: ToolRegistry,
}

impl ToolRegistryAdapter {
    /// Create a new adapter wrapping a ToolRegistry
    pub fn new(registry: ToolRegistry) -> Self {
        Self { registry }
    }

    /// Get reference to the underlying registry
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Get mutable reference to the underlying registry
    pub fn registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.registry
    }

    /// Evaluate approval state based on context and tool
    async fn evaluate_approval(
        &self,
        ctx: &ToolExecutionContext,
        name: &str,
        _args: &Value,
    ) -> ApprovalState {
        // Check if already terminal
        if ctx.approval_state.is_terminal() {
            return ctx.approval_state.clone();
        }

        // Check deny patterns first
        for pattern in &ctx.policy_config.deny_patterns {
            if name.contains(pattern) || pattern == "*" {
                return ApprovalState::Blocked {
                    reason: format!("Matches deny pattern: {}", pattern),
                };
            }
        }

        // Check plan mode enforcement
        if ctx.policy_config.plan_mode_enforced
            && let Ok(is_mutating) = self.is_tool_mutating(name).await
            && is_mutating
        {
            return ApprovalState::Blocked {
                reason: "Plan mode: mutating tools blocked".to_string(),
            };
        }

        // Check trust level bypass
        if ctx.trust_level.can_bypass_approval() {
            return ApprovalState::PreApproved {
                reason: format!("Trust level: {:?}", ctx.trust_level),
            };
        }

        // Check allow patterns
        for pattern in &ctx.policy_config.allow_patterns {
            if name.contains(pattern) || pattern == "*" {
                return ApprovalState::PreApproved {
                    reason: format!("Matches allow pattern: {}", pattern),
                };
            }
        }

        // Check base policy
        match ctx.policy_config.base_policy {
            ToolPolicy::Allow => ApprovalState::PreApproved {
                reason: "Base policy: Allow".to_string(),
            },
            ToolPolicy::Deny => ApprovalState::Blocked {
                reason: "Base policy: Deny".to_string(),
            },
            ToolPolicy::Prompt => ApprovalState::NeedsApproval,
        }
    }

    /// Check if a tool is mutating
    async fn is_tool_mutating(&self, name: &str) -> Result<bool> {
        Ok(!crate::tools::parallel_tool_batch::ParallelToolBatch::is_parallel_safe(name))
    }
}

#[async_trait]
impl UnifiedToolExecutor for ToolRegistryAdapter {
    async fn execute(
        &self,
        mut ctx: ToolExecutionContext,
        name: &str,
        args: Value,
    ) -> Result<UnifiedExecutionResult, UnifiedToolError> {
        let start = Instant::now();

        // Evaluate approval
        ctx.approval_state = self.evaluate_approval(&ctx, name, &args).await;

        // Check if blocked
        if let ApprovalState::Blocked { reason } = &ctx.approval_state {
            return Err(UnifiedToolError::new(
                UnifiedErrorKind::PermissionDenied,
                format!("Tool '{}' blocked: {}", name, reason),
            )
            .with_context(ctx.to_debug_context(name)));
        }

        // Check if needs approval and not approved
        if matches!(ctx.approval_state, ApprovalState::NeedsApproval) {
            return Err(UnifiedToolError::new(
                UnifiedErrorKind::PermissionDenied,
                format!("Tool '{}' requires approval", name),
            )
            .with_context(ctx.to_debug_context(name)));
        }

        // Execute through registry
        let result = self
            .registry
            .execute_tool(name, args.clone())
            .await
            .map_err(|e| {
                let kind = crate::tools::unified_error::classify_error(&e);
                UnifiedToolError::new(kind, e.to_string())
                    .with_context(ctx.to_debug_context(name))
                    .with_source(e)
            })?;

        ctx.approval_state = ApprovalState::Approved;

        Ok(UnifiedExecutionResult {
            value: result,
            approval_state: ctx.approval_state,
            duration: start.elapsed(),
            was_cached: false,
            metadata: HashMap::new(),
        })
    }

    async fn has_tool(&self, name: &str) -> bool {
        ToolExecutor::has_tool(&self.registry, name).await
    }

    async fn available_tools(&self, trust_level: TrustLevel) -> Vec<String> {
        let all_tools = self.registry.available_tools().await;

        // Filter based on trust level
        match trust_level {
            TrustLevel::Untrusted => {
                // Only read-only tools for untrusted context
                let mut filtered = Vec::new();
                for name in all_tools {
                    if let Ok(is_mutating) = self.is_tool_mutating(&name).await
                        && !is_mutating
                    {
                        filtered.push(name);
                    }
                }
                filtered
            }
            _ => all_tools,
        }
    }

    async fn preflight(
        &self,
        ctx: &ToolExecutionContext,
        name: &str,
        args: &Value,
    ) -> Result<ApprovalState, UnifiedToolError> {
        // Check tool exists
        if !self.has_tool(name).await {
            return Err(UnifiedToolError::new(
                UnifiedErrorKind::ToolNotFound,
                format!("Tool '{}' not found", name),
            )
            .with_context(ctx.to_debug_context(name)));
        }

        Ok(self.evaluate_approval(ctx, name, args).await)
    }
}

/// Builder for creating execution contexts with common patterns
pub struct ExecutionContextBuilder {
    session_id: String,
    trust_level: TrustLevel,
    policy_config: PolicyConfig,
    parent_invocation_id: Option<String>,
    turn_number: Option<u32>,
    metadata: HashMap<String, String>,
}

impl ExecutionContextBuilder {
    /// Create a new builder
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            trust_level: TrustLevel::default(),
            policy_config: PolicyConfig::default(),
            parent_invocation_id: None,
            turn_number: None,
            metadata: HashMap::new(),
        }
    }

    /// Set trust level
    pub fn trust_level(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    /// Enable autonomous mode (elevated trust)
    pub fn autonomous(self) -> Self {
        self.trust_level(TrustLevel::Elevated)
    }

    /// Enable plan mode (read-only)
    pub fn plan_mode(mut self) -> Self {
        self.policy_config.plan_mode_enforced = true;
        self
    }

    /// Set timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.policy_config.timeout = Some(timeout);
        self
    }

    /// Add allow pattern
    pub fn allow_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.policy_config.allow_patterns.push(pattern.into());
        self
    }

    /// Set parent invocation for chaining
    pub fn parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_invocation_id = Some(parent_id.into());
        self
    }

    /// Set turn number
    pub fn turn(mut self, turn: u32) -> Self {
        self.turn_number = Some(turn);
        self
    }

    /// Add metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Build the execution context
    pub fn build(self) -> ToolExecutionContext {
        ToolExecutionContext {
            trust_level: self.trust_level,
            approval_state: ApprovalState::Pending,
            policy_config: self.policy_config,
            invocation_id: Uuid::new_v4().to_string(),
            session_id: self.session_id,
            parent_invocation_id: self.parent_invocation_id,
            turn_number: self.turn_number,
            attempt: 1,
            created_at: Instant::now(),
            metadata: self.metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_trust_level_permissions() {
        assert!(!TrustLevel::Untrusted.can_bypass_approval());
        assert!(!TrustLevel::Standard.can_bypass_approval());
        assert!(TrustLevel::Elevated.can_bypass_approval());
        assert!(TrustLevel::Full.can_bypass_approval());

        assert!(!TrustLevel::Untrusted.can_mutate());
        assert!(TrustLevel::Standard.can_mutate());
    }

    #[test]
    fn test_approval_state_transitions() {
        let pending = ApprovalState::Pending;
        assert!(!pending.can_proceed());
        assert!(!pending.is_terminal());

        let approved = ApprovalState::Approved;
        assert!(approved.can_proceed());
        assert!(approved.is_terminal());

        let pre_approved = ApprovalState::PreApproved {
            reason: "test".to_string(),
        };
        assert!(pre_approved.can_proceed());
        assert!(!pre_approved.is_terminal());

        let blocked = ApprovalState::Blocked {
            reason: "test".to_string(),
        };
        assert!(!blocked.can_proceed());
        assert!(blocked.is_terminal());
    }

    #[test]
    fn test_context_builder() {
        let ctx = ExecutionContextBuilder::new("session-123")
            .trust_level(TrustLevel::Elevated)
            .plan_mode()
            .timeout(Duration::from_secs(30))
            .allow_pattern("read_*")
            .turn(5)
            .metadata("agent", "explore")
            .build();

        assert_eq!(ctx.session_id, "session-123");
        assert_eq!(ctx.trust_level, TrustLevel::Elevated);
        assert!(ctx.policy_config.plan_mode_enforced);
        assert_eq!(ctx.policy_config.timeout, Some(Duration::from_secs(30)));
        assert!(
            ctx.policy_config
                .allow_patterns
                .contains(&"read_*".to_string())
        );
        assert_eq!(ctx.turn_number, Some(5));
        assert_eq!(ctx.metadata.get("agent"), Some(&"explore".to_string()));
    }

    #[test]
    fn test_context_for_retry() {
        let ctx = ToolExecutionContext::new("session-1");
        let original_id = ctx.invocation_id.clone();

        let retry_ctx = ctx.for_retry();
        assert_eq!(retry_ctx.attempt, 2);
        assert_eq!(retry_ctx.parent_invocation_id, Some(original_id));
        assert_eq!(retry_ctx.session_id, "session-1");
        assert_ne!(retry_ctx.invocation_id, ctx.invocation_id);
    }

    #[test]
    fn test_context_for_subagent() {
        let ctx = ToolExecutionContext::new("session-1");
        let original_id = ctx.invocation_id.clone();

        let subagent_ctx = ctx.for_subagent(TrustLevel::Elevated);
        assert_eq!(subagent_ctx.trust_level, TrustLevel::Elevated);
        assert_eq!(subagent_ctx.parent_invocation_id, Some(original_id));
        assert_eq!(subagent_ctx.attempt, 1);
    }

    #[test]
    fn test_debug_context_conversion() {
        let ctx = ExecutionContextBuilder::new("session-1")
            .metadata("key", "value")
            .build();

        let debug_ctx = ctx.to_debug_context("test_tool");
        assert_eq!(debug_ctx.tool_name, "test_tool");
        assert_eq!(debug_ctx.invocation_id, Some(ctx.invocation_id.clone()));
        assert_eq!(debug_ctx.attempt, 1);
    }

    #[test]
    fn test_execution_result_builder() {
        let ctx = ToolExecutionContext::new("session-1");
        let result = UnifiedExecutionResult::success(json!({"ok": true}), &ctx)
            .with_cached(true)
            .with_metadata("source", json!("cache"));

        assert!(result.was_cached);
        assert_eq!(result.metadata.get("source"), Some(&json!("cache")));
    }
}
