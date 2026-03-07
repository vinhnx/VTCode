use super::AgentRunner;
use crate::config::constants::tools;
use crate::tools::registry::{ToolErrorType, classify_error};
use crate::tools::{command_args, tool_intent};
use crate::utils::error_messages::ERR_TOOL_DENIED;
use anyhow::{Result, anyhow};
use serde_json::Value;
use tokio::time::Duration;
use tracing::info;

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

            self.tool_registry.check_shell_policy(
                &cmd_text,
                &deny_regex_patterns,
                &deny_glob_patterns,
            )?;

            info!(target = "policy", agent = ?self.agent_type, tool = resolved_tool_name, cmd = %cmd_text, "shell_policy_checked");
        }

        // Use pre-computed retry delays to avoid repeated Duration construction
        const RETRY_DELAYS_MS: [u64; 3] = [200, 400, 800];

        // Clone the registry once and reuse across retries (avoids cloning on each attempt)
        let registry = self.tool_registry.clone();

        // Execute tool with adaptive retry
        let mut last_error: Option<anyhow::Error> = None;
        for (attempt, delay_ms) in RETRY_DELAYS_MS.iter().enumerate() {
            match registry
                .execute_public_tool_ref(resolved_tool_name, args)
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let should_retry = should_retry_tool_error(&e);
                    last_error = Some(e);
                    if should_retry && attempt < RETRY_DELAYS_MS.len().saturating_sub(1) {
                        tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
                        continue;
                    }
                    break;
                }
            }
        }
        Err(anyhow!(
            "Tool '{}' failed after retries: {}",
            resolved_tool_name,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        ))
    }
}

fn should_retry_tool_error(error: &anyhow::Error) -> bool {
    matches!(
        classify_error(error),
        ToolErrorType::Timeout | ToolErrorType::NetworkError
    )
}

#[cfg(test)]
mod tests {
    use super::should_retry_tool_error;
    use anyhow::anyhow;

    #[test]
    fn retries_only_for_transient_error_types() {
        assert!(should_retry_tool_error(&anyhow!(
            "network connection dropped"
        )));
        assert!(should_retry_tool_error(&anyhow!("operation timed out")));
    }

    #[test]
    fn does_not_retry_policy_or_validation_failures() {
        assert!(!should_retry_tool_error(&anyhow!("tool denied by policy")));
        assert!(!should_retry_tool_error(&anyhow!(
            "invalid arguments: missing required field"
        )));
    }
}
