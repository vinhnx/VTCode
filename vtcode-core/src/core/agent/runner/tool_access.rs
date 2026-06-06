use super::AgentRunner;
use crate::config::constants::tools;
use crate::core::agent::harness_kernel::PreparedToolCall;
use crate::core::agent::session::AgentSessionState;
use crate::tools::file_ops::restore_exact_text_content;
use crate::tools::registry::{ExecutionPolicySnapshot, ToolExecutionError};
use crate::tools::{command_args, tool_intent};
use anyhow::Result;
use serde_json::Value;
use tracing::info;

fn restore_exact_file_read_output(mut output: Value) -> Value {
    let Some(obj) = output.as_object_mut() else {
        return output;
    };

    let is_text_file_read = obj
        .get("content_kind")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "text")
        && obj.get("path").and_then(Value::as_str).is_some();
    if !is_text_file_read {
        return output;
    }

    let Some(content) = obj.get("content").and_then(Value::as_str) else {
        return output;
    };
    let Some(size_bytes) = obj
        .get("metadata")
        .and_then(|metadata| metadata.get("data"))
        .and_then(|data| data.get("size_bytes"))
        .and_then(Value::as_u64)
    else {
        return output;
    };

    if let Some(exact_content) = restore_exact_text_content(content, size_bytes) {
        obj.insert("content".to_string(), Value::String(exact_content));
    }

    output
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
            .ok()?;

        self.is_tool_exposed(&canonical_name)
            .await
            .then_some(canonical_name)
    }

    pub(super) fn admit_tool_call(
        &self,
        tool_name: &str,
        args: Value,
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

            let deny_regex_patterns = crate::utils::merge_env_patterns(
                &cfg.commands.deny_regex,
                &format!("{}{}", agent_prefix, "DENY_REGEX"),
            );
            let deny_glob_patterns = crate::utils::merge_env_patterns(
                &cfg.commands.deny_glob,
                &format!("{}{}", agent_prefix, "DENY_GLOB"),
            );

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

        let mut policy = ExecutionPolicySnapshot::default()
            .with_prevalidated(prepared.already_preflighted)
            .with_max_retries(self.config().agent.harness.max_tool_retries as usize);
        policy.retry_jitter = 0.15;

        let outcome = self
            .tool_registry
            .execute_prepared_public_tool_request(prepared, policy)
            .await;
        match (outcome.output, outcome.error) {
            (Some(output), None) => Ok(restore_exact_file_read_output(output)),
            (_, Some(error)) => Err(error.with_surface("agent_runner")),
            _ => Err(ToolExecutionError::policy_violation(
                resolved_tool_name.to_string(),
                "tool execution failed without output or error",
            )
            .with_surface("agent_runner")),
        }
    }

    /// Internal tool execution, skipping validation.
    /// Use when `is_valid_tool` has already been called by the caller.
    #[cfg_attr(not(test), expect(dead_code))]
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
