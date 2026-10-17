//! Safety admission and retry lifecycle for structured tool requests.
//!
//! Public entrypoints remain in the execution facade. Each attempt checks safety
//! before dispatch and applies the same retry policy to safety, structured-output,
//! and dispatch failures.

use serde_json::Value;
use std::time::{Duration, Instant};
use vtcode_commons::ErrorCategory;

use crate::retry::RetryPolicyCoreExt;
use crate::tools::invocation::ToolInvocationId;
use crate::tools::safety_gateway::{SafetyContext, SafetyDecision, SafetyError as GatewaySafetyError};
use crate::tools::tool_intent;

use super::execution_kernel;
use super::{ToolErrorType, ToolExecutionError, ToolExecutionOutcome, ToolExecutionRequest, ToolRegistry};

fn requests_unsandboxed_shell_permissions(tool_name: &str, args: &Value) -> bool {
    if !tool_intent::is_command_run_tool_call(tool_name, args) {
        return false;
    }

    matches!(
        args.get("sandbox_permissions").and_then(Value::as_str),
        Some(value) if value.eq_ignore_ascii_case("require_escalated") || value.eq_ignore_ascii_case("bypass_sandbox")
    )
}

impl ToolRegistry {
    fn safety_denial_error(
        &self,
        tool_name: &str,
        reason: &str,
        violation: Option<GatewaySafetyError>,
        retry_after: Option<Duration>,
    ) -> ToolExecutionError {
        let mut error = ToolExecutionError::policy_violation(
            tool_name.to_string(),
            format!("Safety gateway denied execution: {reason}"),
        );

        match violation {
            Some(GatewaySafetyError::RateLimitExceeded { .. }) => {
                error.error_type = ToolErrorType::NetworkError;
                error.category = ErrorCategory::RateLimit;
                error.retryable = true;
                error.is_recoverable = true;
            }
            Some(GatewaySafetyError::TurnLimitReached { .. })
            | Some(GatewaySafetyError::SessionLimitReached { .. }) => {
                error.error_type = ToolErrorType::ExecutionError;
                error.category = ErrorCategory::ResourceExhausted;
                error.retryable = false;
                error.is_recoverable = false;
            }
            Some(GatewaySafetyError::PlanningPolicyViolation(_)) => {
                error.error_type = ToolErrorType::PolicyViolation;
                error.category = ErrorCategory::PlanningPolicyViolation;
                error.retryable = false;
                error.is_recoverable = true;
            }
            Some(GatewaySafetyError::CommandPolicyDenied(_))
            | Some(GatewaySafetyError::DotfileProtectionViolation(_))
            | None => {}
        }

        if let Some(delay) = retry_after {
            error.retry_after_ms = Some(delay.as_millis() as u64);
        }
        error.circuit_breaker_impact = error.category.should_trip_circuit_breaker();
        error.recovery_suggestions = error.category.recovery_suggestions();
        error
    }

    async fn check_safety_for_request(
        &self,
        tool_name: &str,
        args: &Value,
        invocation_id: Option<String>,
    ) -> Option<ToolExecutionError> {
        let context = SafetyContext::new(self.harness_context_snapshot().session_id);
        let invocation_id = invocation_id
            .and_then(|id| ToolInvocationId::parse(&id).ok())
            .unwrap_or_default();
        let safety_result = self
            .safety_gateway
            .check_and_record_with_id(&context, tool_name, args, Some(invocation_id))
            .await;

        match safety_result.decision {
            SafetyDecision::Allow | SafetyDecision::NeedsApproval(_) => None,
            SafetyDecision::Deny(reason) => Some(
                self.safety_denial_error(tool_name, &reason, safety_result.violation, safety_result.retry_after)
                    .with_surface("tool_registry"),
            ),
        }
    }

    pub(super) async fn execute_tool_request_internal(&self, request: ToolExecutionRequest) -> ToolExecutionOutcome {
        let execution_started_at = Instant::now();
        let tool_name = &request.tool_name;
        let policy = request.policy.clone();

        if requests_unsandboxed_shell_permissions(tool_name, &request.args) {
            let message = format!(
                "sandbox_permissions in `{tool_name}` requires an enforced operator approval decision before unsandboxed execution"
            );
            let error = ToolExecutionError::new(tool_name.clone(), ToolErrorType::PolicyViolation, message)
                .with_tool_call_context(tool_name, &request.args)
                .with_surface("tool_registry");
            return ToolExecutionOutcome::failure(tool_name.clone(), 1, error)
                .with_execution_metadata(execution_started_at.elapsed(), None);
        }

        let mut retry_policy = crate::retry::RetryPolicy::from_retries(
            policy.max_retries as u32,
            policy.retry_base_delay,
            policy.retry_max_delay,
            policy.retry_multiplier,
        );
        retry_policy.jitter = policy.retry_jitter.clamp(0.0, 1.0);

        let max_attempts = retry_policy.max_attempts.max(1);
        let mut attempt_index: u32 = 0;
        let mut last_error: Option<ToolExecutionError> = None;

        while attempt_index < max_attempts {
            if !policy.safety_prevalidated
                && let Some(safety_error) = self
                    .check_safety_for_request(tool_name, &request.args, policy.invocation_id.clone())
                    .await
            {
                let decorated = safety_error
                    .with_tool_call_context(tool_name, &request.args)
                    .with_attempt(attempt_index + 1)
                    .with_surface("tool_registry");
                if let Some(terminal) = Self::classify_and_step(
                    decorated,
                    &retry_policy,
                    tool_name,
                    &mut attempt_index,
                    max_attempts,
                    &mut last_error,
                )
                .await
                {
                    let category = Some(terminal.category);
                    return ToolExecutionOutcome::failure(tool_name, attempt_index + 1, terminal)
                        .with_execution_metadata(execution_started_at.elapsed(), category);
                }
                continue;
            }

            let result = self
                .execute_public_tool_ref_dispatch(
                    tool_name,
                    &request.args,
                    policy.prevalidated,
                    execution_kernel::DispatchMode::Harness,
                    policy.exec_settlement_mode,
                )
                .await;

            match result {
                Ok(output) => {
                    if let Some(structured_error) = ToolExecutionError::from_tool_output(&output) {
                        let decorated = structured_error
                            .with_tool_call_context(tool_name, &request.args)
                            .with_attempt(attempt_index + 1)
                            .with_surface("tool_registry");
                        if let Some(terminal) = Self::classify_and_step(
                            decorated,
                            &retry_policy,
                            tool_name,
                            &mut attempt_index,
                            max_attempts,
                            &mut last_error,
                        )
                        .await
                        {
                            let category = Some(terminal.category);
                            return ToolExecutionOutcome::failure(tool_name, attempt_index + 1, terminal)
                                .with_execution_metadata(execution_started_at.elapsed(), category);
                        }
                        continue;
                    }

                    let recovered_category = last_error.as_ref().map(|error| error.category);
                    return ToolExecutionOutcome::success(tool_name, attempt_index + 1, output)
                        .with_execution_metadata(execution_started_at.elapsed(), recovered_category);
                }
                Err(error) => {
                    let mut base = ToolExecutionError::from_anyhow(
                        tool_name,
                        &error,
                        attempt_index,
                        false,
                        false,
                        Some("tool_registry"),
                    );
                    let lower_message = base.message.to_ascii_lowercase();
                    let lower_original = base.original_error.as_deref().unwrap_or_default().to_ascii_lowercase();
                    if lower_message.contains("circuit breaker") || lower_original.contains("circuit breaker") {
                        base.category = ErrorCategory::CircuitOpen;
                        base.retryable = true;
                        base.is_recoverable = true;
                        if base.retry_delay_ms.is_none() {
                            base.retry_delay_ms = Some(policy.retry_base_delay.as_millis() as u64);
                        }
                    }

                    if let Some(terminal) = Self::classify_and_step(
                        base,
                        &retry_policy,
                        tool_name,
                        &mut attempt_index,
                        max_attempts,
                        &mut last_error,
                    )
                    .await
                    {
                        let category = Some(terminal.category);
                        return ToolExecutionOutcome::failure(tool_name, attempt_index + 1, terminal)
                            .with_execution_metadata(execution_started_at.elapsed(), category);
                    }
                    continue;
                }
            }
        }

        let outcome = ToolExecutionOutcome::failure(
            tool_name,
            max_attempts,
            last_error.unwrap_or_else(|| {
                ToolExecutionError::new(
                    tool_name,
                    ToolErrorType::ExecutionError,
                    format!("Tool '{}' failed after {} attempts with no structured error", tool_name, max_attempts),
                )
                .with_surface("tool_registry")
            }),
        );
        let category = outcome.last_error_category;
        outcome.with_execution_metadata(execution_started_at.elapsed(), category)
    }

    /// Apply the retry policy to a `ToolExecutionError` and either schedule
    /// the next attempt (sleep + bump index, return `None`) or report a
    /// terminal failure (return `Some(structured)` for the caller to surface).
    ///
    /// Consolidates the three identical retry/sleep/continue blocks that
    /// previously lived inline in `execute_tool_request_internal`.
    async fn classify_and_step(
        decorated: ToolExecutionError,
        retry_policy: &crate::retry::RetryPolicy,
        tool_name: &str,
        attempt_index: &mut u32,
        max_attempts: u32,
        last_error: &mut Option<ToolExecutionError>,
    ) -> Option<ToolExecutionError> {
        let structured = retry_policy.apply_to_tool_execution_error(decorated, *attempt_index, Some(tool_name));
        let retry_delay = structured.retry_after().or_else(|| structured.retry_delay());
        if structured.retryable
            && *attempt_index + 1 < max_attempts
            && let Some(delay) = retry_delay
        {
            *last_error = Some(structured);
            tokio::time::sleep(delay).await;
            *attempt_index = attempt_index.saturating_add(1);
            return None;
        }
        Some(structured)
    }
}

#[cfg(test)]
mod tests;
