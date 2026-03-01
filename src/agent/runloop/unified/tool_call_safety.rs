//! Tool call safety validation and safeguards.
//!
//! This adapter keeps the runloop-facing API stable while delegating safety
//! checks to `vtcode_core::tools::SafetyGateway` for single-source consistency.

use anyhow::anyhow;
use serde_json::{Map, Value};
use std::collections::HashSet;
use thiserror::Error;
use vtcode_core::tools::{
    RiskLevel, SafetyDecision, SafetyError as GatewaySafetyError, SafetyGateway,
    SafetyGatewayConfig, ToolInvocationId, UnifiedExecutionContext, WorkspaceTrust,
};

/// Safety violation errors
#[derive(Debug, Error)]
pub enum SafetyError {
    #[error("Per-turn tool limit reached (max: {max}). Wait or adjust config.")]
    TurnLimitReached { max: usize },
    #[error("Session tool limit reached (max: {max}). End turn or reduce tool calls.")]
    SessionLimitReached { max: usize },
    #[error("Rate limit exceeded: {current} calls/{window} (max: {max})")]
    RateLimitExceeded {
        current: usize,
        max: usize,
        window: &'static str,
    },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Safety validation rules for tool calls
pub struct ToolCallSafetyValidator {
    /// Destructive tools that require explicit confirmation
    destructive_tools: HashSet<&'static str>,
    /// Total tool limit per session
    max_per_session: usize,
    /// Call rate limit (max calls per second)
    rate_limit_per_second: usize,
    /// Optional per-minute cap to prevent bursts that dodge the per-second window
    rate_limit_per_minute: Option<usize>,
    /// Shared safety gateway for canonical checks
    safety_gateway: SafetyGateway,
    /// Validator-scoped execution context
    gateway_ctx: UnifiedExecutionContext,
}

impl ToolCallSafetyValidator {
    pub fn new() -> Self {
        let mut destructive = HashSet::new();
        destructive.insert("delete_file");
        destructive.insert("edit_file");
        destructive.insert("write_file");
        destructive.insert("shell");
        destructive.insert("apply_patch");

        let rate_limit_per_second = std::env::var("VTCODE_TOOL_RATE_LIMIT_PER_SECOND")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(5);

        let rate_limit_per_minute = std::env::var("VTCODE_TOOL_CALLS_PER_MIN")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0);

        let max_per_turn = 10;
        let max_per_session = 100;
        let gateway_config = SafetyGatewayConfig {
            max_per_turn,
            max_per_session,
            rate_limit_per_second,
            rate_limit_per_minute,
            plan_mode_active: false,
            workspace_trust: WorkspaceTrust::Trusted,
            approval_risk_threshold: RiskLevel::Medium,
            // The runloop enforces adaptive per-tool throttling separately.
            // Keep SafetyGateway focused on turn/session budgets by default.
            enforce_rate_limits: false,
        };

        Self {
            destructive_tools: destructive,
            max_per_session,
            rate_limit_per_second,
            rate_limit_per_minute,
            safety_gateway: SafetyGateway::with_config(gateway_config),
            gateway_ctx: UnifiedExecutionContext::new("runloop-safety-validator"),
        }
    }

    /// Reset per-turn counters; call at the start of a turn
    pub async fn start_turn(&mut self) {
        self.safety_gateway.start_turn().await;
    }

    /// Override per-turn and session limits based on runtime config
    pub fn set_limits(&mut self, max_per_turn: usize, max_per_session: usize) {
        self.max_per_session = max_per_session;
        self.safety_gateway
            .set_limits(max_per_turn, max_per_session);
    }

    /// Increase the session tool limit
    pub fn increase_session_limit(&mut self, increment: usize) {
        self.max_per_session = self.max_per_session.saturating_add(increment);
        self.safety_gateway.increase_session_limit(increment);
        tracing::info!("Session tool limit increased to {}", self.max_per_session);
    }

    #[allow(dead_code)]
    pub fn rate_limit_per_second(&self) -> usize {
        self.rate_limit_per_second
    }

    #[allow(dead_code)]
    pub fn set_rate_limit_per_second(&mut self, limit: usize) {
        if limit > 0 {
            self.rate_limit_per_second = limit;
            self.safety_gateway
                .set_rate_limits(self.rate_limit_per_second, self.rate_limit_per_minute);
        }
    }

    #[allow(dead_code)]
    pub fn set_rate_limit_enforcement(&mut self, enabled: bool) {
        self.safety_gateway.set_rate_limit_enforcement(enabled);
    }

    #[allow(dead_code)]
    pub fn set_rate_limit_per_minute(&mut self, limit: Option<usize>) {
        self.rate_limit_per_minute = limit.filter(|v| *v > 0);
        self.safety_gateway
            .set_rate_limits(self.rate_limit_per_second, self.rate_limit_per_minute);
    }

    #[allow(dead_code)]
    pub fn rate_limit_per_minute(&self) -> Option<usize> {
        self.rate_limit_per_minute
    }

    /// Get the current session limit
    pub fn get_session_limit(&self) -> usize {
        self.max_per_session
    }

    /// Validate a tool call before execution
    pub async fn validate_call(
        &mut self,
        tool_name: &str,
        args: &Value,
    ) -> std::result::Result<CallValidation, SafetyError> {
        self.validate_call_with_invocation_id(tool_name, args, ToolInvocationId::new())
            .await
    }

    /// Validate a tool call with an explicit invocation id for log correlation.
    pub async fn validate_call_with_invocation_id(
        &mut self,
        tool_name: &str,
        args: &Value,
        invocation_id: ToolInvocationId,
    ) -> std::result::Result<CallValidation, SafetyError> {
        let intent = vtcode_core::tools::tool_intent::classify_tool_intent(tool_name, args);

        let result = self
            .safety_gateway
            .check_and_record_with_id(&self.gateway_ctx, tool_name, args, Some(invocation_id))
            .await;

        match result.decision {
            SafetyDecision::Allow | SafetyDecision::NeedsApproval(_) => Ok(CallValidation {
                is_destructive: intent.destructive,
                requires_confirmation: intent.destructive,
                execution_allowed: true,
            }),
            SafetyDecision::Deny(reason) => Err(map_gateway_violation(result.violation, &reason)),
        }
    }

    /// Check if tool is destructive
    #[allow(dead_code)]
    pub fn is_destructive(&self, tool_name: &str) -> bool {
        self.destructive_tools.contains(tool_name)
            || vtcode_core::tools::tool_intent::classify_tool_intent(
                tool_name,
                &Value::Object(Map::new()),
            )
            .destructive
    }

    /// Get list of destructive tools
    #[allow(dead_code)]
    pub fn destructive_tools(&self) -> Vec<&'static str> {
        self.destructive_tools.iter().copied().collect()
    }

    /// Reset rate limit tracking
    #[allow(dead_code)]
    pub async fn reset_rate_limit(&mut self) {
        self.start_turn().await;
    }
}

fn map_gateway_violation(violation: Option<GatewaySafetyError>, reason: &str) -> SafetyError {
    match violation {
        Some(GatewaySafetyError::TurnLimitReached { max }) => SafetyError::TurnLimitReached { max },
        Some(GatewaySafetyError::SessionLimitReached { max }) => {
            SafetyError::SessionLimitReached { max }
        }
        Some(GatewaySafetyError::RateLimitExceeded {
            current,
            max,
            window,
        }) => SafetyError::RateLimitExceeded {
            current,
            max,
            window,
        },
        Some(err) => SafetyError::Other(anyhow!(err.to_string())),
        None => SafetyError::Other(anyhow!(reason.to_string())),
    }
}

impl Default for ToolCallSafetyValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of tool call validation
#[derive(Debug, Clone)]
pub struct CallValidation {
    /// Whether tool is destructive
    #[allow(dead_code)]
    pub is_destructive: bool,
    /// Whether confirmation is required
    #[allow(dead_code)]
    pub requires_confirmation: bool,
    /// Whether execution is allowed
    #[allow(dead_code)]
    pub execution_allowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_destructive_tool_detection() {
        let validator = ToolCallSafetyValidator::new();
        assert!(validator.is_destructive("delete_file"));
        assert!(validator.is_destructive("edit_file"));
        assert!(!validator.is_destructive("read_file"));
        assert!(!validator.is_destructive("grep_file"));
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let mut validator = ToolCallSafetyValidator::new();
        validator.set_rate_limit_per_second(2);
        validator.set_rate_limit_enforcement(true);
        validator.start_turn().await;

        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_ok()
        );
        assert!(matches!(
            validator.validate_call("read_file", &json!({})).await,
            Err(SafetyError::RateLimitExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn test_validation_structure() {
        let mut validator = ToolCallSafetyValidator::new();
        validator.start_turn().await;

        let validation = validator
            .validate_call("read_file", &json!({}))
            .await
            .unwrap();
        assert!(!validation.is_destructive);
        assert!(!validation.requires_confirmation);
        assert!(validation.execution_allowed);

        let validation = validator
            .validate_call("delete_file", &json!({}))
            .await
            .unwrap();
        assert!(validation.is_destructive);
        assert!(validation.requires_confirmation);
        assert!(validation.execution_allowed);
    }

    #[tokio::test]
    async fn test_turn_and_session_limits() {
        let mut validator = ToolCallSafetyValidator::new();
        validator.set_limits(2, 3);

        validator.start_turn().await;
        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_err()
        );

        validator.start_turn().await;
        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_err()
        );
    }
}
