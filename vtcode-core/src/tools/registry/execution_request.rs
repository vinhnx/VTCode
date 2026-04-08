use serde_json::Value;
use std::time::Duration;

use super::ToolExecutionError;

/// Controls how unified exec calls should settle before returning to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecSettlementMode {
    /// Return on the first yield boundary and require explicit follow-up poll/continue calls.
    #[default]
    Manual,
    /// Keep polling pipe-backed sessions until they reach a terminal state.
    SettleNonInteractive,
}

impl ExecSettlementMode {
    #[must_use]
    pub fn settle_noninteractive(self) -> bool {
        matches!(self, Self::SettleNonInteractive)
    }
}

/// Runtime execution policy snapshot for a single tool call.
#[derive(Debug, Clone)]
pub struct ExecutionPolicySnapshot {
    /// Maximum retry attempts after the initial attempt.
    pub max_retries: usize,
    /// Base exponential backoff delay.
    pub retry_base_delay: Duration,
    /// Maximum retry backoff delay.
    pub retry_max_delay: Duration,
    /// Retry backoff multiplier.
    pub retry_multiplier: f64,
    /// Retry jitter ratio in [0.0, 1.0].
    pub retry_jitter: f64,
    /// When true, caller already ran preflight validation.
    pub prevalidated: bool,
    /// Optional invocation id for cross-surface correlation.
    pub invocation_id: Option<String>,
    /// How unified_exec should settle before returning.
    pub exec_settlement_mode: ExecSettlementMode,
    /// Whether caller already completed safety gateway admission.
    pub safety_prevalidated: bool,
}

impl Default for ExecutionPolicySnapshot {
    fn default() -> Self {
        Self {
            max_retries: 0,
            retry_base_delay: Duration::from_millis(200),
            retry_max_delay: Duration::from_secs(2),
            retry_multiplier: 2.0,
            retry_jitter: 0.0,
            prevalidated: false,
            invocation_id: None,
            exec_settlement_mode: ExecSettlementMode::Manual,
            safety_prevalidated: false,
        }
    }
}

impl ExecutionPolicySnapshot {
    #[must_use]
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    #[must_use]
    pub fn with_prevalidated(mut self, prevalidated: bool) -> Self {
        self.prevalidated = prevalidated;
        self
    }

    #[must_use]
    pub fn with_invocation_id(mut self, invocation_id: Option<String>) -> Self {
        self.invocation_id = invocation_id;
        self
    }

    #[must_use]
    pub fn with_exec_settlement_mode(mut self, mode: ExecSettlementMode) -> Self {
        self.exec_settlement_mode = mode;
        self
    }

    #[must_use]
    pub fn with_safety_prevalidated(mut self, safety_prevalidated: bool) -> Self {
        self.safety_prevalidated = safety_prevalidated;
        self
    }
}

/// Canonical tool execution request routed through ToolRegistry kernel.
#[derive(Debug, Clone)]
pub struct ToolExecutionRequest {
    pub tool_name: String,
    pub args: Value,
    pub policy: ExecutionPolicySnapshot,
}

impl ToolExecutionRequest {
    #[must_use]
    pub fn new(tool_name: impl Into<String>, args: Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            args,
            policy: ExecutionPolicySnapshot::default(),
        }
    }

    #[must_use]
    pub fn with_policy(mut self, policy: ExecutionPolicySnapshot) -> Self {
        self.policy = policy;
        self
    }
}

/// Canonical execution result for shared runtime adapters.
#[derive(Debug, Clone)]
pub struct ToolExecutionOutcome {
    pub tool_name: String,
    pub attempts: u32,
    pub output: Option<Value>,
    pub error: Option<ToolExecutionError>,
}

impl ToolExecutionOutcome {
    #[must_use]
    pub fn success(tool_name: impl Into<String>, attempts: u32, output: Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            attempts,
            output: Some(output),
            error: None,
        }
    }

    #[must_use]
    pub fn failure(tool_name: impl Into<String>, attempts: u32, error: ToolExecutionError) -> Self {
        Self {
            tool_name: tool_name.into(),
            attempts,
            output: None,
            error: Some(error),
        }
    }

    #[must_use]
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}
