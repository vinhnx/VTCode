use super::AgentRunner;
use crate::config::constants::tools;
use crate::retry::RetryPolicy;
use crate::tools::{command_args, tool_intent};
use crate::utils::error_messages::ERR_TOOL_DENIED;
use anyhow::{Result, anyhow};
use serde_json::Value;
use tokio::time::Duration;
use tracing::{info, trace, warn};

impl AgentRunner {
    #[inline]
    fn canonical_exec_request_name(tool_name: &str) -> &str {
        tool_intent::canonical_unified_exec_tool_name(tool_name).unwrap_or(tool_name)
    }

    async fn resolve_executable_tool_name(&self, tool_name: &str) -> Option<String> {
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
        let canonical_name = self.validate_and_normalize_tool_name(tool_name, args)?;

        // Fail fast if tool is denied or missing to avoid tight retry loops
        if self
            .resolve_executable_tool_name(&canonical_name)
            .await
            .is_none()
        {
            return Err(anyhow!("{}: {}", ERR_TOOL_DENIED, canonical_name));
        }
        self.execute_tool_internal(&canonical_name, args).await
    }

    /// Internal tool execution, skipping validation.
    /// Use when `is_valid_tool` has already been called by the caller.
    pub(super) async fn execute_tool_internal(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<Value> {
        let resolved_tool_name = Self::canonical_exec_request_name(tool_name);
        let shell_command = if tool_intent::is_command_run_tool_call(tool_name, args)
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
                .map_err(|err| anyhow!("tool denied by policy: {err}"))?;

            info!(target = "policy", agent = ?self.agent_type, tool = resolved_tool_name, cmd = %cmd_text, "shell_policy_checked");
        }

        // Check shared circuit breaker before executing — fail fast when the
        // tool's circuit is open to avoid wasting resources on doomed requests.
        if let Some(breaker) = self.tool_registry.shared_circuit_breaker()
            && !breaker.allow_request_for_tool(resolved_tool_name)
        {
            let remaining = breaker
                .remaining_backoff(resolved_tool_name)
                .map(|d| format!(" (retry in {}s)", d.as_secs()))
                .unwrap_or_default();
            warn!(
                tool = %resolved_tool_name,
                "tool execution blocked by circuit breaker"
            );
            return Err(anyhow!(
                "Tool '{}' temporarily disabled by circuit breaker{}",
                resolved_tool_name,
                remaining
            ));
        }

        // Canonical retry policy: 3 attempts (1 initial + 2 retries), exponential
        // backoff from 200ms to 2s.  The `RetryPolicy` uses `ErrorCategory` to
        // decide retryability — covering Timeout, Network, RateLimit, and
        // ServiceUnavailable while failing fast on non-retryable errors like
        // Authentication, PolicyViolation, and InvalidParameters.
        let policy = RetryPolicy::new(3, Duration::from_millis(200), Duration::from_secs(2), 2.0);

        // Clone the registry once and reuse across retries (avoids cloning on each attempt)
        let registry = self.tool_registry.clone();

        // Execute tool with policy-driven adaptive retry
        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..policy.max_attempts {
            match registry
                .execute_public_tool_ref(resolved_tool_name, args)
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let decision =
                        policy.decision_for_anyhow(&e, attempt, Some(resolved_tool_name));
                    trace!(
                        tool = %resolved_tool_name,
                        attempt = attempt + 1,
                        category = ?decision.category,
                        retryable = decision.retryable,
                        "tool execution attempt failed"
                    );
                    last_error = Some(e);
                    if decision.retryable {
                        if let Some(delay) = decision.delay {
                            tokio::time::sleep(delay).await;
                        }
                        continue;
                    }
                    break;
                }
            }
        }

        let category = last_error
            .as_ref()
            .map(vtcode_commons::classify_anyhow_error)
            .unwrap_or(vtcode_commons::ErrorCategory::ExecutionError);
        Err(anyhow!(
            "[{}] Tool '{}' failed after {} attempt(s): {}",
            category.user_label(),
            resolved_tool_name,
            policy.max_attempts,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        ))
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
