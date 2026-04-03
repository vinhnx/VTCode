//! Tool call safety validation and safeguards.
//!
//! This adapter keeps the runloop-facing API stable while delegating safety
//! checks to `vtcode_core::tools::SafetyGateway` for single-source consistency.

use anyhow::anyhow;
#[cfg(test)]
use serde_json::Map;
use serde_json::Value;
#[cfg(test)]
use std::sync::Mutex;
use thiserror::Error;
use vtcode_core::tools::{
    RiskLevel, SafetyContext, SafetyDecision, SafetyError as GatewaySafetyError, SafetyGateway,
    SafetyGatewayConfig, ToolInvocationId, WorkspaceTrust,
};

/// Safety violation errors
#[derive(Debug, Error)]
pub(crate) enum SafetyError {
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
pub(crate) struct ToolCallSafetyValidator {
    /// Shared safety gateway for canonical checks
    safety_gateway: SafetyGateway,
    /// Validator-scoped execution context
    gateway_ctx: SafetyContext,
    #[cfg(test)]
    test_rate_limits: Mutex<TestRateLimits>,
}

#[cfg(test)]
struct TestRateLimits {
    per_second: usize,
    per_minute: Option<usize>,
}

impl ToolCallSafetyValidator {
    pub(crate) fn new() -> Self {
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
            safety_gateway: SafetyGateway::with_config(gateway_config),
            gateway_ctx: SafetyContext::new("runloop-safety-validator"),
            #[cfg(test)]
            test_rate_limits: Mutex::new(TestRateLimits {
                per_second: rate_limit_per_second,
                per_minute: rate_limit_per_minute,
            }),
        }
    }

    /// Reset per-turn counters; call at the start of a turn
    pub(crate) fn start_turn(&self) {
        self.safety_gateway.start_turn();
    }

    /// Override per-turn and session limits based on runtime config
    pub(crate) fn set_limits(&self, max_per_turn: usize, max_per_session: usize) {
        self.safety_gateway
            .set_limits(max_per_turn, max_per_session);
    }

    /// Increase the session tool limit
    pub(crate) fn increase_session_limit(&self, increment: usize) {
        self.safety_gateway.increase_session_limit(increment);
    }

    #[cfg(test)]
    pub fn set_rate_limit_per_second(&self, limit: usize) {
        if limit > 0 {
            let mut test_rate_limits = self
                .test_rate_limits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            test_rate_limits.per_second = limit;
            self.safety_gateway
                .set_rate_limits(test_rate_limits.per_second, test_rate_limits.per_minute);
        }
    }

    #[cfg(test)]
    pub fn set_rate_limit_enforcement(&self, enabled: bool) {
        self.safety_gateway.set_rate_limit_enforcement(enabled);
    }

    /// Get the current session limit
    pub(crate) fn get_session_limit(&self) -> usize {
        self.safety_gateway.max_per_session()
    }

    /// Validate a tool call before execution
    pub(crate) async fn validate_call(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> std::result::Result<(), SafetyError> {
        self.validate_call_with_invocation_id(tool_name, args, ToolInvocationId::new())
            .await
    }

    /// Validate a tool call with an explicit invocation id for log correlation.
    pub(crate) async fn validate_call_with_invocation_id(
        &self,
        tool_name: &str,
        args: &Value,
        invocation_id: ToolInvocationId,
    ) -> std::result::Result<(), SafetyError> {
        let result = self
            .safety_gateway
            .check_and_record_with_id(&self.gateway_ctx, tool_name, args, Some(invocation_id))
            .await;

        match result.decision {
            SafetyDecision::Allow | SafetyDecision::NeedsApproval(_) => Ok(()),
            SafetyDecision::Deny(reason) => Err(map_gateway_violation(result.violation, &reason)),
        }
    }

    /// Check if tool is destructive
    #[cfg(test)]
    pub fn is_destructive(&self, tool_name: &str) -> bool {
        let normalized = tool_name.trim().to_ascii_lowercase();
        vtcode_core::tools::tool_intent::classify_tool_intent(
            normalized.as_str(),
            &Value::Object(Map::new()),
        )
        .destructive
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
        let validator = ToolCallSafetyValidator::new();
        validator.set_rate_limit_per_second(2);
        validator.set_rate_limit_enforcement(true);
        validator.start_turn();

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
    async fn test_validation_allows_safe_and_destructive_tools() {
        let validator = ToolCallSafetyValidator::new();
        validator.start_turn();

        assert!(
            validator
                .validate_call("read_file", &json!({}))
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate_call("delete_file", &json!({}))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_turn_and_session_limits() {
        let validator = ToolCallSafetyValidator::new();
        validator.set_limits(2, 3);

        validator.start_turn();
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

        validator.start_turn();
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
