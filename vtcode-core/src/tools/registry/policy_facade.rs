//! Tool policy evaluation helpers attached to ToolRegistry.

use anyhow::Result;

use crate::config::ToolsConfig;
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::tools::names::canonical_tool_name;

use super::{ToolPermissionDecision, ToolRegistry};

impl ToolRegistry {
    pub async fn enable_full_auto_mode(&self, allowed_tools: &[String]) {
        let available = self.available_tools().await;
        self.policy_gateway
            .write()
            .await
            .enable_full_auto_mode(allowed_tools, &available);
    }

    pub async fn disable_full_auto_mode(&self) {
        self.policy_gateway.write().await.disable_full_auto_mode();
    }

    pub async fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.policy_gateway
            .read()
            .await
            .current_full_auto_allowlist()
    }

    pub async fn set_policy_manager(&self, manager: ToolPolicyManager) {
        self.policy_gateway
            .write()
            .await
            .set_policy_manager(manager);
        self.sync_policy_catalog().await;
    }

    pub async fn set_tool_policy(&self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        let mut gateway = self.policy_gateway.write().await;
        gateway.set_tool_policy(tool_name, policy).await
    }

    pub async fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.policy_gateway.read().await.get_tool_policy(tool_name)
    }

    pub async fn reset_tool_policies(&self) -> Result<()> {
        self.policy_gateway
            .write()
            .await
            .reset_tool_policies()
            .await
    }

    pub async fn allow_all_tools(&self) -> Result<()> {
        self.policy_gateway.write().await.allow_all_tools().await
    }

    pub async fn deny_all_tools(&self) -> Result<()> {
        self.policy_gateway.write().await.deny_all_tools().await
    }

    pub async fn print_policy_status(&self) {
        self.policy_gateway.read().await.print_policy_status();
    }

    pub async fn apply_config_policies(&self, tools_config: &ToolsConfig) -> Result<()> {
        let mut policy_gateway = self.policy_gateway.write().await;
        if let Ok(policy_manager) = policy_gateway.policy_manager_mut() {
            policy_manager.apply_tools_config(tools_config).await?;
        }

        let detect_window = super::DEFAULT_LOOP_DETECT_WINDOW
            .max(tools_config.max_repeated_tool_calls.saturating_mul(2))
            .max(1);
        self.execution_history
            .set_loop_detection_limits(detect_window, tools_config.max_repeated_tool_calls);
        self.execution_history
            .set_rate_limit_per_minute(super::config_helpers::tool_rate_limit_from_env());

        Ok(())
    }

    /// Prompt for permission before starting long-running tool executions to avoid spinner conflicts
    pub async fn preflight_tool_permission(&self, name: &str) -> Result<bool> {
        match self.evaluate_tool_policy(name).await? {
            ToolPermissionDecision::Allow => Ok(true),
            ToolPermissionDecision::Deny => Ok(false),
            ToolPermissionDecision::Prompt => Ok(true),
        }
    }

    pub async fn evaluate_tool_policy(&self, name: &str) -> Result<ToolPermissionDecision> {
        if let Some(tool_name) = name.strip_prefix("mcp_") {
            return self.evaluate_mcp_tool_policy(name, tool_name).await;
        }

        let canonical = canonical_tool_name(name);
        let normalized = canonical.as_ref();

        {
            let gateway = self.policy_gateway.read().await;
            if !gateway.has_policy_manager()
                && let Some(registration) = self.inventory.registration_for(normalized)
                && let Some(permission) = registration.default_permission()
            {
                return Ok(match permission {
                    ToolPolicy::Allow => ToolPermissionDecision::Allow,
                    ToolPolicy::Deny => ToolPermissionDecision::Deny,
                    ToolPolicy::Prompt => ToolPermissionDecision::Prompt,
                });
            }
        }

        self.policy_gateway
            .write()
            .await
            .evaluate_tool_policy(normalized)
            .await
    }

    async fn evaluate_mcp_tool_policy(
        &self,
        full_name: &str,
        tool_name: &str,
    ) -> Result<ToolPermissionDecision> {
        let provider = match self.find_mcp_provider(tool_name).await {
            Some(provider) => provider,
            None => {
                // Unknown provider for this tool; default to prompt for safety
                return Ok(ToolPermissionDecision::Prompt);
            }
        };

        {
            let gateway = self.policy_gateway.read().await;
            // Check full-auto allowlist first (aligned with policy_gateway behavior)
            if gateway.has_full_auto_allowlist() && !gateway.is_allowed_in_full_auto(full_name) {
                return Ok(ToolPermissionDecision::Deny);
            }
        }

        let mut gateway = self.policy_gateway.write().await;
        if let Ok(policy_manager) = gateway.policy_manager_mut() {
            match policy_manager.get_mcp_tool_policy(&provider, tool_name) {
                ToolPolicy::Allow => {
                    gateway.preapprove(full_name);
                    Ok(ToolPermissionDecision::Allow)
                }
                ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
                ToolPolicy::Prompt => {
                    // In full-auto mode with Prompt policy, we still need to check
                    // if this specific tool is in the allowlist
                    if gateway.has_full_auto_allowlist() {
                        Ok(ToolPermissionDecision::Prompt)
                    } else {
                        // In normal mode with Prompt policy, default to prompt for MCP tools
                        // (MCP tools don't have risk metadata for auto-approval like built-in tools)
                        Ok(ToolPermissionDecision::Prompt)
                    }
                }
            }
        } else {
            // Policy manager not available - default to prompt for safety
            // This aligns with MCP tools' default_permission of Prompt
            Ok(ToolPermissionDecision::Prompt)
        }
    }

    /// Mark a tool as pre-approved.
    ///
    /// In TUI mode we already showed the inline approval modal, so we allow preapproval for
    /// any tool to avoid re-prompting in the CLI layer. In CLI mode we keep the legacy
    /// allowlist restriction.
    pub async fn mark_tool_preapproved(&self, name: &str) {
        let mut gateway = self.policy_gateway.write().await;
        // Allow all when TUI mode is active (approval already captured by modal)
        if std::env::var("VTCODE_TUI_MODE").is_ok() {
            gateway.preapprove(name);
            tracing::debug!(tool = %name, "Preapproved tool in TUI mode");
            return;
        }

        // Legacy CLI allowlist of tools that can be preapproved
        const PREAPPROVABLE_TOOLS: &[&str] = &["debug_agent", "analyze_agent"];

        if PREAPPROVABLE_TOOLS.contains(&name) {
            gateway.preapprove(name);
        } else {
            tracing::warn!(
                tool = %name,
                "Attempted to preapprove non-whitelisted tool. Use permission pipeline instead."
            );
        }
    }

    pub async fn persist_mcp_tool_policy(&self, name: &str, policy: ToolPolicy) -> Result<()> {
        if !name.starts_with("mcp_") {
            return Ok(());
        }

        let Some(tool_name) = name.strip_prefix("mcp_") else {
            return Ok(());
        };

        let Some(provider) = self.find_mcp_provider(tool_name).await else {
            return Ok(());
        };

        self.policy_gateway
            .write()
            .await
            .persist_mcp_tool_policy(&provider, tool_name, policy)
            .await
    }
}
