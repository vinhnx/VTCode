use super::AgentRunner;
use crate::config::constants::tools;
use crate::tools::registry::{ToolErrorType, classify_error};
use crate::utils::error_messages::ERR_TOOL_DENIED;
use anyhow::{Result, anyhow};
use serde_json::Value;
use tokio::time::Duration;
use tracing::info;

impl AgentRunner {
    /// Check if a tool is allowed for this agent
    pub(super) async fn is_tool_allowed(&self, tool_name: &str) -> bool {
        let policy = self.tool_registry.get_tool_policy(tool_name).await;
        matches!(
            policy,
            crate::tool_policy::ToolPolicy::Allow | crate::tool_policy::ToolPolicy::Prompt
        )
    }

    /// Validate if a tool name is safe, registered, and allowed by policy
    #[inline]
    pub(super) async fn is_valid_tool(&self, tool_name: &str) -> bool {
        // Normalize legacy alias for shell commands
        let canonical = if tool_name == "shell" {
            tools::RUN_PTY_CMD
        } else {
            tool_name
        };

        // Ensure the tool exists in the registry (including MCP tools)
        if !self.tool_registry.has_tool(canonical).await {
            return false;
        }

        // Enforce policy gate: Allow and Prompt are executable, Deny blocks
        self.is_tool_allowed(canonical).await
    }

    /// Execute a tool by name with given arguments.
    /// This is the public API that includes validation; for internal use after
    /// validation, prefer `execute_tool_internal`.
    #[allow(dead_code)]
    pub(super) async fn execute_tool(&self, tool_name: &str, args: &Value) -> Result<Value> {
        // Fail fast if tool is denied or missing to avoid tight retry loops
        if !self.is_valid_tool(tool_name).await {
            return Err(anyhow!("{}: {}", ERR_TOOL_DENIED, tool_name));
        }
        self.execute_tool_internal(tool_name, args).await
    }

    /// Internal tool execution, skipping validation.
    /// Use when `is_valid_tool` has already been called by the caller.
    pub(super) async fn execute_tool_internal(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<Value> {
        let extract_command_text = |args: &Value| {
            if let Some(cmd_val) = args.get("command") {
                if let Some(arr) = cmd_val.as_array() {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    cmd_val.as_str().unwrap_or("").to_owned()
                }
            } else {
                String::new()
            }
        };

        let shell_command = match tool_name {
            n if n == tools::RUN_PTY_CMD || n == "shell" => Some(extract_command_text(args)),
            n if n == tools::UNIFIED_EXEC => {
                let action = args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("command").map(|_| "run"));
                if action == Some("run") {
                    Some(extract_command_text(args))
                } else {
                    None
                }
            }
            _ => None,
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

            info!(target = "policy", agent = ?self.agent_type, tool = tool_name, cmd = %cmd_text, "shell_policy_checked");
        }

        // Use pre-computed retry delays to avoid repeated Duration construction
        const RETRY_DELAYS_MS: [u64; 3] = [200, 400, 800];

        // Clone the registry once and reuse across retries (avoids cloning on each attempt)
        let registry = self.tool_registry.clone();

        // Execute tool with adaptive retry
        let mut last_error: Option<anyhow::Error> = None;
        for (attempt, delay_ms) in RETRY_DELAYS_MS.iter().enumerate() {
            match registry.execute_tool_ref(tool_name, args).await {
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
            tool_name,
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
