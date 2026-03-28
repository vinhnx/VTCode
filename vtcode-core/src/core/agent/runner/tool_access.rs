use super::AgentRunner;
use crate::config::constants::tools;
use crate::core::agent::harness_kernel::PreparedToolCall;
use crate::core::agent::session::AgentSessionState;
use crate::retry::RetryPolicy;
use crate::tools::registry::{ToolErrorType, ToolExecutionError};
use crate::tools::{command_args, tool_intent};
use crate::utils::error_messages::ERR_TOOL_DENIED;
use anyhow::{Result, anyhow};
use serde_json::Value;
use std::borrow::Cow;
use tokio::time::Duration;
use tracing::{error, info, trace, warn};

fn tool_retry_delay(error: &ToolExecutionError) -> Option<Duration> {
    error.retry_after().or_else(|| error.retry_delay())
}

fn circuit_open_error(tool_name: &str, retry_after: Option<Duration>) -> ToolExecutionError {
    let mut error = ToolExecutionError::new(
        tool_name.to_string(),
        ToolErrorType::ExecutionError,
        format!(
            "Tool '{}' is temporarily unavailable after repeated transient failures",
            tool_name
        ),
    );
    error.category = vtcode_commons::ErrorCategory::CircuitOpen;
    error.retryable = true;
    error.is_recoverable = true;
    error.retry_after_ms = retry_after.map(|delay| delay.as_millis() as u64);
    error.circuit_breaker_impact = true;
    error.recovery_suggestions = vec![Cow::Borrowed(
        "Wait briefly, then retry or choose a different tool.",
    )];
    error
}

impl AgentRunner {
    #[inline]
    fn canonical_exec_request_name(tool_name: &str) -> &str {
        tool_intent::canonical_unified_exec_tool_name(tool_name).unwrap_or(tool_name)
    }

    pub(super) async fn resolve_executable_tool_name(&self, tool_name: &str) -> Option<String> {
        let requested_name = Self::canonical_exec_request_name(tool_name);
        let canonical_name = self
            .tool_registry
            .resolve_public_tool_name(requested_name)
            .await
            .ok()?;

        self.is_tool_exposed(&canonical_name)
            .await
            .then_some(canonical_name)
    }

    #[allow(dead_code)]
    pub(super) fn validate_and_normalize_tool_name(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<String> {
        let requested_name = Self::canonical_exec_request_name(tool_name);

        self.tool_registry
            .preflight_validate_call(requested_name, args)
            .map(|outcome| outcome.normalized_tool_name)
    }

    pub(super) fn admit_tool_call(
        &self,
        tool_name: &str,
        args: &Value,
        session_state: &mut AgentSessionState,
    ) -> Result<PreparedToolCall> {
        let normalized_args = self.normalize_tool_args(tool_name, args, session_state);
        self.tool_registry.admit_public_tool_call(
            Self::canonical_exec_request_name(tool_name),
            &normalized_args,
        )
    }

    /// Check if a tool is allowed for this agent
    pub(super) async fn is_tool_allowed(&self, tool_name: &str) -> bool {
        let policy = self.tool_registry.get_tool_policy(tool_name).await;
        matches!(
            policy,
            crate::tool_policy::ToolPolicy::Allow | crate::tool_policy::ToolPolicy::Prompt
        )
    }

    /// Check if a tool is exposed to the active runtime after feature gating.
    pub(super) async fn is_tool_exposed(&self, tool_name: &str) -> bool {
        if !self.tool_registry.is_allowed_in_full_auto(tool_name).await {
            return false;
        }

        self.features()
            .allows_tool_name(tool_name, self.tool_registry.is_plan_mode(), false)
            && self.is_tool_allowed(tool_name).await
    }

    /// Validate if a tool name is safe, registered, and allowed by policy
    #[inline]
    pub(super) async fn is_valid_tool(&self, tool_name: &str) -> bool {
        self.resolve_executable_tool_name(tool_name).await.is_some()
    }

    /// Execute a tool by name with given arguments.
    /// This is the public API that includes validation; for internal use after
    /// validation, prefer `execute_tool_internal`.
    #[allow(dead_code)]
    pub(super) async fn execute_tool(&self, tool_name: &str, args: &Value) -> Result<Value> {
        let prepared = self
            .tool_registry
            .admit_public_tool_call(Self::canonical_exec_request_name(tool_name), args)?;
        let canonical_name = prepared.canonical_name.clone();

        // Fail fast if tool is denied or missing to avoid tight retry loops
        if self
            .resolve_executable_tool_name(&canonical_name)
            .await
            .is_none()
        {
            return Err(anyhow!("{}: {}", ERR_TOOL_DENIED, canonical_name));
        }
        self.tool_registry
            .execute_prepared_public_tool_ref_with_exec_mode(&prepared, false)
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub(super) async fn execute_prepared_tool_internal(
        &self,
        prepared: &PreparedToolCall,
    ) -> std::result::Result<Value, ToolExecutionError> {
        let resolved_tool_name = prepared.canonical_name.as_str();
        let args = &prepared.effective_args;
        let shell_command = if tool_intent::is_command_run_tool_call(resolved_tool_name, args)
            || (resolved_tool_name == tools::UNIFIED_EXEC
                && tool_intent::unified_exec_action(args).is_none())
        {
            command_args::command_text(args).ok().flatten()
        } else {
            None
        };

        // Enforce per-agent shell policies for shell-executed commands.
        if let Some(cmd_text) = shell_command {
            let cfg = self.config();

            let agent_prefix = format!(
                "VTCODE_{}_COMMANDS_",
                self.agent_type.to_string().to_uppercase()
            );

            let mut deny_regex_patterns: Vec<String> = cfg.commands.deny_regex.clone();
            if let Ok(extra) = std::env::var(format!("{}DENY_REGEX", agent_prefix)) {
                deny_regex_patterns.extend(extra.split(',').filter_map(|entry| {
                    let trimmed = entry.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }));
            }

            let mut deny_glob_patterns: Vec<String> = cfg.commands.deny_glob.clone();
            if let Ok(extra) = std::env::var(format!("{}DENY_GLOB", agent_prefix)) {
                deny_glob_patterns.extend(extra.split(',').filter_map(|entry| {
                    let trimmed = entry.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }));
            }

            self.tool_registry
                .check_shell_policy(&cmd_text, &deny_regex_patterns, &deny_glob_patterns)
                .map_err(|err| {
                    ToolExecutionError::policy_violation(
                        resolved_tool_name.to_string(),
                        format!("tool denied by policy: {err}"),
                    )
                    .with_surface("agent_runner")
                })?;

            info!(target = "policy", agent = ?self.agent_type, tool = resolved_tool_name, cmd = %cmd_text, "shell_policy_checked");
        }

        // Canonical retry policy: 3 attempts (1 initial + 2 retries), exponential
        // backoff from 200ms to 2s.  The `RetryPolicy` uses `ErrorCategory` to
        // decide retryability — covering Timeout, Network, RateLimit, and
        // ServiceUnavailable while failing fast on non-retryable errors like
        // Authentication, PolicyViolation, and InvalidParameters.
        let policy = RetryPolicy::new(3, Duration::from_millis(200), Duration::from_secs(2), 2.0);

        // Clone the registry once and reuse across retries (avoids cloning on each attempt)
        let registry = self.tool_registry.clone();
        let shared_breaker = self.tool_registry.shared_circuit_breaker();

        // Execute tool with policy-driven adaptive retry
        let mut last_error: Option<ToolExecutionError> = None;
        for attempt in 0..policy.max_attempts {
            if let Some(breaker) = &shared_breaker
                && !breaker.allow_request_for_tool(resolved_tool_name)
            {
                let structured = policy.apply_to_tool_execution_error(
                    circuit_open_error(
                        resolved_tool_name,
                        breaker.remaining_backoff(resolved_tool_name),
                    )
                    .with_attempt(attempt + 1)
                    .with_surface("agent_runner"),
                    attempt,
                    Some(resolved_tool_name),
                );
                let delay = tool_retry_delay(&structured);
                trace!(
                    tool = %resolved_tool_name,
                    attempt = attempt + 1,
                    category = ?structured.category,
                    retryable = structured.retryable,
                    "tool execution blocked before dispatch"
                );
                if let Some(delay) = delay {
                    warn!(
                        tool = %resolved_tool_name,
                        attempt = attempt + 1,
                        retry_in_ms = delay.as_millis(),
                        "tool execution blocked by transient service protection"
                    );
                    last_error = Some(structured);
                    tokio::time::sleep(delay).await;
                    continue;
                }
                error!(
                    tool = %resolved_tool_name,
                    attempt = attempt + 1,
                    category = ?structured.category,
                    "tool execution failed without retry"
                );
                return Err(structured);
            }

            match registry
                .execute_prepared_public_tool_ref_with_exec_mode(prepared, false)
                .await
            {
                Ok(result) => {
                    if let Some(structured_error) = ToolExecutionError::from_tool_output(&result) {
                        let structured = policy.apply_to_tool_execution_error(
                            structured_error
                                .with_tool_call_context(resolved_tool_name, args)
                                .with_attempt(attempt + 1)
                                .with_surface("agent_runner"),
                            attempt,
                            Some(resolved_tool_name),
                        );
                        let delay = tool_retry_delay(&structured);
                        trace!(
                            tool = %resolved_tool_name,
                            attempt = attempt + 1,
                            category = ?structured.category,
                            retryable = structured.retryable,
                            partial_state_possible = structured.partial_state_possible,
                            "tool execution returned structured failure"
                        );
                        if let Some(delay) = delay {
                            warn!(
                                tool = %resolved_tool_name,
                                attempt = attempt + 1,
                                retry_in_ms = delay.as_millis(),
                                category = ?structured.category,
                                "transient tool failure; retrying"
                            );
                            last_error = Some(structured);
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        error!(
                            tool = %resolved_tool_name,
                            attempt = attempt + 1,
                            category = ?structured.category,
                            "tool execution failed without retry"
                        );
                        return Err(structured);
                    }

                    if attempt > 0 {
                        info!(
                            tool = %resolved_tool_name,
                            attempt = attempt + 1,
                            "tool execution succeeded after retry"
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let structured = policy.apply_to_tool_execution_error(
                        ToolExecutionError::from_anyhow(
                            resolved_tool_name,
                            &e,
                            attempt,
                            false,
                            false,
                            Some("agent_runner"),
                        )
                        .with_tool_call_context(resolved_tool_name, args)
                        .with_attempt(attempt + 1),
                        attempt,
                        Some(resolved_tool_name),
                    );
                    let delay = tool_retry_delay(&structured);
                    trace!(
                        tool = %resolved_tool_name,
                        attempt = attempt + 1,
                        category = ?structured.category,
                        retryable = structured.retryable,
                        partial_state_possible = structured.partial_state_possible,
                        "tool execution attempt failed"
                    );
                    last_error = Some(structured.clone());
                    if let Some(delay) = delay {
                        warn!(
                            tool = %resolved_tool_name,
                            attempt = attempt + 1,
                            retry_in_ms = delay.as_millis(),
                            category = ?structured.category,
                            "transient tool failure; retrying"
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    error!(
                        tool = %resolved_tool_name,
                        attempt = attempt + 1,
                        category = ?structured.category,
                        "tool execution failed without retry"
                    );
                    return Err(structured);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ToolExecutionError::new(
                resolved_tool_name.to_string(),
                ToolErrorType::ExecutionError,
                format!(
                    "Tool '{}' exhausted {} attempt(s) without a result",
                    resolved_tool_name, policy.max_attempts
                ),
            )
            .with_tool_call_context(resolved_tool_name, args)
            .with_surface("agent_runner")
        }))
    }

    /// Internal tool execution, skipping validation.
    /// Use when `is_valid_tool` has already been called by the caller.
    #[allow(dead_code)]
    pub(super) async fn execute_tool_internal(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> std::result::Result<Value, ToolExecutionError> {
        let prepared = self
            .tool_registry
            .admit_public_tool_call(Self::canonical_exec_request_name(tool_name), args)
            .map_err(|error| {
                ToolExecutionError::from_anyhow(
                    Self::canonical_exec_request_name(tool_name),
                    &error,
                    0,
                    false,
                    false,
                    Some("agent_runner"),
                )
                .with_tool_call_context(tool_name, args)
            })?;
        self.execute_prepared_tool_internal(&prepared).await
    }
}

#[cfg(test)]
mod tests {
    use crate::retry::RetryPolicy;
    use std::time::Duration;
    use vtcode_commons::ErrorCategory;

    /// Verify that the canonical retry policy correctly identifies transient
    /// errors as retryable via the `ErrorCategory` classifier.
    #[test]
    fn policy_retries_transient_errors() {
        let policy = RetryPolicy::new(3, Duration::from_millis(200), Duration::from_secs(2), 2.0);

        let network_err = anyhow::anyhow!("network connection dropped");
        let decision = policy.decision_for_anyhow(&network_err, 0, Some("test_tool"));
        assert!(decision.retryable, "network errors should be retryable");
        assert_eq!(decision.category, ErrorCategory::Network);

        let timeout_err = anyhow::anyhow!("operation timed out");
        let decision = policy.decision_for_anyhow(&timeout_err, 0, Some("test_tool"));
        assert!(decision.retryable, "timeout errors should be retryable");
        assert_eq!(decision.category, ErrorCategory::Timeout);

        let rate_limit_err = anyhow::anyhow!("429 Too Many Requests");
        let decision = policy.decision_for_anyhow(&rate_limit_err, 0, Some("test_tool"));
        assert!(decision.retryable, "rate limit errors should be retryable");
    }

    /// Verify that non-retryable errors fail fast without retry.
    #[test]
    fn policy_does_not_retry_permanent_errors() {
        let policy = RetryPolicy::new(3, Duration::from_millis(200), Duration::from_secs(2), 2.0);

        let policy_err = anyhow::anyhow!("tool denied by policy");
        let decision = policy.decision_for_anyhow(&policy_err, 0, Some("test_tool"));
        assert!(
            !decision.retryable,
            "policy violations should not be retryable"
        );

        let auth_err = anyhow::anyhow!("invalid api key");
        let decision = policy.decision_for_anyhow(&auth_err, 0, Some("test_tool"));
        assert!(
            !decision.retryable,
            "authentication errors should not be retryable"
        );

        let param_err = anyhow::anyhow!("invalid arguments: missing required field");
        let decision = policy.decision_for_anyhow(&param_err, 0, Some("test_tool"));
        assert!(
            !decision.retryable,
            "invalid parameter errors should not be retryable"
        );
    }

    /// Verify that retryable decisions include backoff delays.
    #[test]
    fn policy_provides_backoff_delays() {
        let policy = RetryPolicy::new(3, Duration::from_millis(200), Duration::from_secs(2), 2.0);

        let err = anyhow::anyhow!("network connection dropped");

        let d0 = policy.decision_for_anyhow(&err, 0, Some("test_tool"));
        let d1 = policy.decision_for_anyhow(&err, 1, Some("test_tool"));

        assert!(d0.delay.is_some(), "first retry should have a delay");
        assert!(d1.delay.is_some(), "second retry should have a delay");
        assert!(
            d1.delay.unwrap_or_default() >= d0.delay.unwrap_or_default(),
            "backoff should increase"
        );
    }
}
