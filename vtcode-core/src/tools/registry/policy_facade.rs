//! Tool policy evaluation helpers attached to ToolRegistry.

use anyhow::Result;
use hashbrown::HashSet;
use indexmap::IndexMap;

use crate::config::ToolsConfig;
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::tools::mcp::{
    is_legacy_mcp_tool_name, legacy_mcp_tool_name, parse_canonical_mcp_tool_name,
};
use crate::tools::names::canonical_tool_name;
use crate::ui::is_tui_mode;

use super::{ToolPermissionDecision, ToolRegistry};

fn more_restrictive_policy(
    left: vtcode_config::ToolPolicy,
    right: vtcode_config::ToolPolicy,
) -> vtcode_config::ToolPolicy {
    match (left, right) {
        (vtcode_config::ToolPolicy::Deny, _) | (_, vtcode_config::ToolPolicy::Deny) => {
            vtcode_config::ToolPolicy::Deny
        }
        (vtcode_config::ToolPolicy::Prompt, _) | (_, vtcode_config::ToolPolicy::Prompt) => {
            vtcode_config::ToolPolicy::Prompt
        }
        _ => vtcode_config::ToolPolicy::Allow,
    }
}

impl ToolRegistry {
    fn resolve_runtime_policy_name(&self, name: &str) -> String {
        if is_legacy_mcp_tool_name(name) || parse_canonical_mcp_tool_name(name).is_some() {
            return name.to_string();
        }

        if let Ok(resolved) = self.resolve_public_tool(name) {
            return resolved.registration_name().to_string();
        }

        match name {
            "list_dir" | "list_directory" => {
                crate::config::constants::tools::UNIFIED_SEARCH.to_string()
            }
            _ => canonical_tool_name(name).into_owned(),
        }
    }

    fn normalize_tools_config_policies(&self, tools_config: &ToolsConfig) -> ToolsConfig {
        let mut normalized = tools_config.clone();
        let mut explicit_canonical_names: HashSet<String> = HashSet::default();

        for name in tools_config.policies.keys() {
            let canonical = self.resolve_runtime_policy_name(name);
            if canonical == *name {
                explicit_canonical_names.insert(canonical);
            }
        }

        let mut policies = IndexMap::new();
        for (name, policy) in &tools_config.policies {
            let canonical = self.resolve_runtime_policy_name(name);
            if canonical != *name && explicit_canonical_names.contains(&canonical) {
                continue;
            }
            let merged = policies
                .get(&canonical)
                .cloned()
                .map(|existing| more_restrictive_policy(existing, *policy))
                .unwrap_or(*policy);
            policies.insert(canonical, merged);
        }

        normalized.policies = policies;
        normalized
    }

    pub async fn enable_full_auto_mode(&self, allowed_tools: &[String]) {
        let normalized_allowed_tools: Vec<String> = allowed_tools
            .iter()
            .map(|tool| self.resolve_runtime_policy_name(tool))
            .collect();
        let available = self.available_tools().await;
        self.policy_gateway
            .write()
            .await
            .enable_full_auto_mode(&normalized_allowed_tools, &available);
    }

    pub async fn disable_full_auto_mode(&self) {
        self.policy_gateway.write().await.disable_full_auto_mode();
    }

    pub async fn set_enforce_safe_mode_prompts(&self, enabled: bool) {
        self.policy_gateway
            .write()
            .await
            .set_enforce_safe_mode_prompts(enabled);
    }

    pub async fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.policy_gateway
            .read()
            .await
            .current_full_auto_allowlist()
    }

    pub async fn is_allowed_in_full_auto(&self, tool_name: &str) -> bool {
        self.policy_gateway
            .read()
            .await
            .is_allowed_in_full_auto(&self.resolve_runtime_policy_name(tool_name))
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
        gateway
            .set_tool_policy(&self.resolve_runtime_policy_name(tool_name), policy)
            .await
    }

    pub async fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.policy_gateway
            .read()
            .await
            .get_tool_policy(&self.resolve_runtime_policy_name(tool_name))
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
        let normalized_tools_config = self.normalize_tools_config_policies(tools_config);
        let mut policy_gateway = self.policy_gateway.write().await;
        if let Ok(policy_manager) = policy_gateway.policy_manager_mut() {
            policy_manager
                .apply_tools_config(&normalized_tools_config)
                .await?;
        }

        let detect_window = super::DEFAULT_LOOP_DETECT_WINDOW
            .max(
                normalized_tools_config
                    .max_repeated_tool_calls
                    .saturating_mul(2),
            )
            .max(1);
        self.execution_history.set_loop_detection_limits(
            detect_window,
            normalized_tools_config.max_repeated_tool_calls,
        );
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
        if let Some(tool_name) = legacy_mcp_tool_name(name) {
            return self.evaluate_mcp_tool_policy(name, tool_name).await;
        }

        if let Some((_, tool_name)) = parse_canonical_mcp_tool_name(name) {
            return self.evaluate_mcp_tool_policy(name, tool_name).await;
        }

        let resolved_name = self.resolve_runtime_policy_name(name);
        let resolved_public_tool = self.resolve_public_tool(name).ok();

        if let Some(resolution) = &resolved_public_tool
            && let Some((_, tool_name)) =
                parse_canonical_mcp_tool_name(resolution.registration_name())
        {
            return self
                .evaluate_mcp_tool_policy(resolution.registration_name(), tool_name)
                .await;
        }

        let (default_permission, safe_mode_prompt) = self
            .inventory
            .get_registration(&resolved_name)
            .map(|registration| {
                (
                    registration
                        .metadata()
                        .default_permission()
                        .unwrap_or(ToolPolicy::Prompt),
                    registration
                        .metadata()
                        .behavior()
                        .map(|behavior| behavior.safe_mode_prompt)
                        .unwrap_or(false),
                )
            })
            .or_else(|| {
                resolved_public_tool
                    .as_ref()
                    .map(|resolution| (resolution.default_permission().clone(), false))
            })
            .unwrap_or((ToolPolicy::Prompt, false));

        {
            let gateway = self.policy_gateway.read().await;
            if !gateway.has_policy_manager() {
                return Ok(match default_permission {
                    ToolPolicy::Allow => ToolPermissionDecision::Allow,
                    ToolPolicy::Deny => ToolPermissionDecision::Deny,
                    ToolPolicy::Prompt => ToolPermissionDecision::Prompt,
                });
            }
        }

        self.policy_gateway
            .write()
            .await
            .evaluate_tool_policy(&resolved_name, safe_mode_prompt, default_permission)
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
        let normalized_name = self.resolve_runtime_policy_name(name);
        let mut gateway = self.policy_gateway.write().await;
        // Allow all when TUI mode is active (approval already captured by modal)
        if is_tui_mode() {
            gateway.preapprove(&normalized_name);
            tracing::debug!(tool = %normalized_name, "Preapproved tool in TUI mode");
            return;
        }

        // Legacy CLI allowlist of tools that can be preapproved
        const PREAPPROVABLE_TOOLS: &[&str] = &["debug_agent", "analyze_agent"];

        if PREAPPROVABLE_TOOLS.contains(&normalized_name.as_str()) {
            gateway.preapprove(&normalized_name);
        } else {
            tracing::warn!(
                tool = %normalized_name,
                "Attempted to preapprove non-whitelisted tool. Use permission pipeline instead."
            );
        }
    }

    pub async fn persist_mcp_tool_policy(&self, name: &str, policy: ToolPolicy) -> Result<()> {
        let (provider, tool_name) = if is_legacy_mcp_tool_name(name) {
            let Some(tool_name) = legacy_mcp_tool_name(name) else {
                return Ok(());
            };
            let Some(provider) = self.find_mcp_provider(tool_name).await else {
                return Ok(());
            };
            (provider, tool_name.to_string())
        } else if let Some((provider, tool_name)) = parse_canonical_mcp_tool_name(name) {
            (provider.to_string(), tool_name.to_string())
        } else if let Ok(resolution) = self.resolve_public_tool(name) {
            let Some((provider, tool_name)) =
                parse_canonical_mcp_tool_name(resolution.registration_name())
            else {
                return Ok(());
            };
            (provider.to_string(), tool_name.to_string())
        } else {
            return Ok(());
        };

        self.policy_gateway
            .write()
            .await
            .persist_mcp_tool_policy(&provider, &tool_name, policy)
            .await
    }
}
