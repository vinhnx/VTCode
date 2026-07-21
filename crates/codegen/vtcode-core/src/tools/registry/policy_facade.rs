//! Tool policy evaluation helpers attached to ToolRegistry.

use anyhow::Result;
use hashbrown::HashSet;
use indexmap::IndexMap;

use super::{ToolPermissionDecision, ToolRegistry};
use crate::config::ToolsConfig;
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::tools::mcp::{is_legacy_mcp_tool_name, legacy_mcp_tool_name, parse_canonical_mcp_tool_name};
use crate::tools::names::canonical_tool_name;

fn more_restrictive_policy(left: ToolPolicy, right: ToolPolicy) -> ToolPolicy {
    match (left, right) {
        (ToolPolicy::Deny, _) | (_, ToolPolicy::Deny) => ToolPolicy::Deny,
        (ToolPolicy::Prompt, _) | (_, ToolPolicy::Prompt) => ToolPolicy::Prompt,
        _ => ToolPolicy::Allow,
    }
}

impl ToolRegistry {
    pub(super) async fn visible_policy_names(
        &self,
        session_tools_config: crate::tools::handlers::SessionToolsConfig,
    ) -> Vec<String> {
        self.model_tools(session_tools_config)
            .await
            .iter()
            .map(|tool| self.resolve_runtime_policy_name(tool.function_name()))
            .collect()
    }

    fn resolve_runtime_policy_name(&self, name: &str) -> String {
        if is_legacy_mcp_tool_name(name) || parse_canonical_mcp_tool_name(name).is_some() {
            return name.to_string();
        }

        if let Ok(resolved) = self.resolve_public_tool(name) {
            return resolved.registration_name().to_string();
        }

        canonical_tool_name(name).to_owned()
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
                .map(|existing| more_restrictive_policy(existing, policy.clone()))
                .unwrap_or(policy.clone());
            policies.insert(canonical, merged);
        }

        normalized.policies = policies;
        normalized
    }

    pub async fn enable_full_auto_permission(&self, allowed_tools: &[String]) {
        self.enable_full_auto_permission_for_session(
            allowed_tools,
            crate::tools::handlers::SessionToolsConfig::full_public(
                crate::tools::handlers::SessionSurface::Interactive,
                crate::config::types::CapabilityLevel::CodeSearch,
                crate::config::ToolDocumentationMode::Full,
                crate::tools::handlers::ToolModelCapabilities::default(),
            ),
        )
        .await;
    }

    /// Enable full-auto mode against the tools visible in a specific session.
    pub async fn enable_full_auto_permission_for_session(
        &self,
        allowed_tools: &[String],
        session_tools_config: crate::tools::handlers::SessionToolsConfig,
    ) {
        #[cfg(test)]
        let test_hooks = {
            let policy_gateway = self.policy_gateway.clone();
            policy_gateway.full_auto_catalogue_test_hooks()
        };
        #[cfg(test)]
        test_hooks.pause_before_enable_lifecycle().await;
        let lifecycle = {
            let policy_gateway = self.policy_gateway.clone();
            policy_gateway.full_auto_catalogue_lifecycle()
        };
        let _lifecycle_guard = lifecycle.lock().await;
        let normalized_allowed_tools: Vec<String> = allowed_tools
            .iter()
            .map(|tool| self.resolve_runtime_policy_name(tool))
            .collect();
        let visible_policy_names = self.visible_policy_names(session_tools_config.clone()).await;
        #[cfg(test)]
        test_hooks.pause_after_enable_snapshot().await;
        self.policy_gateway
            .enable_full_auto_permission(&normalized_allowed_tools, &visible_policy_names, session_tools_config)
            .await;
    }

    pub async fn disable_full_auto_permission(&self) {
        #[cfg(test)]
        let test_hooks = {
            let policy_gateway = self.policy_gateway.clone();
            policy_gateway.full_auto_catalogue_test_hooks()
        };
        #[cfg(test)]
        test_hooks.pause_before_disable_lifecycle().await;
        let lifecycle = {
            let policy_gateway = self.policy_gateway.clone();
            policy_gateway.full_auto_catalogue_lifecycle()
        };
        let _lifecycle_guard = lifecycle.lock().await;
        self.policy_gateway.disable_full_auto_permission().await;
    }

    pub async fn set_enforce_safe_mode_prompts(&self, enabled: bool) {
        self.policy_gateway.set_enforce_safe_mode_prompts(enabled);
    }

    pub async fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.policy_gateway.current_full_auto_allowlist().await
    }

    pub async fn is_allowed_in_full_auto(&self, tool_name: &str) -> bool {
        self.policy_gateway
            .is_allowed_in_full_auto(&self.resolve_runtime_policy_name(tool_name))
            .await
    }

    pub async fn is_denied_in_full_auto(&self, tool_name: &str) -> bool {
        self.policy_gateway
            .is_denied_in_full_auto(&self.resolve_runtime_policy_name(tool_name))
            .await
    }

    pub async fn set_policy_manager(&self, manager: ToolPolicyManager) {
        self.policy_gateway.set_policy_manager(manager).await;
        self.sync_policy_catalog().await;
    }

    pub async fn set_tool_policy(&self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        let normalized_name = self.resolve_runtime_policy_name(tool_name);
        self.policy_gateway.set_tool_policy(&normalized_name, policy).await
    }

    pub async fn persist_approval_cache_key(&self, approval_key: &str) -> Result<()> {
        self.policy_gateway.add_approval_cache_key(approval_key).await
    }

    pub async fn persist_approval_cache_prefix(&self, prefix_entry: &str) -> Result<()> {
        self.policy_gateway.add_approval_cache_prefix(prefix_entry).await
    }

    pub async fn has_persisted_approval(&self, approval_key: &str) -> bool {
        self.policy_gateway.has_approval_cache_key(approval_key).await
    }

    pub async fn find_persisted_shell_approval_prefix(
        &self,
        command_words: &[String],
        scope_signature: &str,
    ) -> Option<String> {
        self.policy_gateway
            .matching_shell_approval_prefix(command_words, scope_signature)
            .await
    }

    pub async fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.policy_gateway
            .get_tool_policy(&self.resolve_runtime_policy_name(tool_name))
            .await
    }

    pub async fn reset_tool_policies(&self) -> Result<()> {
        let manager = self.policy_gateway.policy_manager().await;
        let mut manager = manager.ok_or_else(|| anyhow::anyhow!("Tool policy manager not available"))?;
        manager.reset_all_to_prompt().await?;
        self.policy_gateway.set_policy_manager(manager).await;
        Ok(())
    }

    pub async fn allow_all_tools(&self) -> Result<()> {
        let manager = self.policy_gateway.policy_manager().await;
        let mut manager = manager.ok_or_else(|| anyhow::anyhow!("Tool policy manager not available"))?;
        let all_tools: Vec<String> = self
            .inventory
            .registrations_snapshot()
            .into_iter()
            .map(|registration| registration.name().to_string())
            .collect();
        manager.allow_all_tools_for_tools(&all_tools).await?;
        self.policy_gateway.set_policy_manager(manager).await;
        Ok(())
    }

    pub async fn deny_all_tools(&self) -> Result<()> {
        let manager = self.policy_gateway.policy_manager().await;
        let mut manager = manager.ok_or_else(|| anyhow::anyhow!("Tool policy manager not available"))?;
        manager.deny_all_tools().await?;
        self.policy_gateway.set_policy_manager(manager).await;
        Ok(())
    }

    pub async fn print_policy_status(&self) {
        self.policy_gateway.print_policy_status().await;
    }

    pub async fn apply_config_policies(&self, tools_config: &ToolsConfig) -> Result<()> {
        let normalized_tools_config = self.normalize_tools_config_policies(tools_config);
        {
            let mut active_tool_profile = self
                .active_tool_profile
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *active_tool_profile = normalized_tools_config.profile;
        }
        *self.cached_available_tools.write() = None;
        self.sync_policy_catalog().await;

        let manager = {
            let guard = self.policy_gateway.clone();
            guard.policy_manager().await
        };
        let mut manager = manager.ok_or_else(|| anyhow::anyhow!("Tool policy manager not available"))?;
        manager.apply_tools_config(&normalized_tools_config).await?;
        self.policy_gateway.set_policy_manager(manager).await;

        let detect_window = super::DEFAULT_LOOP_DETECT_WINDOW
            .max(normalized_tools_config.max_repeated_tool_calls.saturating_mul(2))
            .max(1);
        self.execution_history
            .set_loop_detection_limits(detect_window, normalized_tools_config.max_repeated_tool_calls);
        self.execution_history
            .set_rate_limit_per_minute(crate::tools::rate_limit_config::tool_calls_per_minute_from_env());

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
            && let Some((_, tool_name)) = parse_canonical_mcp_tool_name(resolution.registration_name())
        {
            return self.evaluate_mcp_tool_policy(resolution.registration_name(), tool_name).await;
        }

        let (default_permission, safe_mode_prompt) = self
            .inventory
            .get_registration(&resolved_name)
            .map(|registration| {
                (
                    registration.metadata().default_permission().unwrap_or(ToolPolicy::Prompt),
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

        let has_policy_manager = {
            let gateway = self.policy_gateway.clone();
            gateway.has_policy_manager().await
        };

        if !has_policy_manager {
            return Ok(match default_permission {
                ToolPolicy::Allow => ToolPermissionDecision::Allow,
                ToolPolicy::Deny => ToolPermissionDecision::Deny,
                ToolPolicy::Prompt => ToolPermissionDecision::Prompt,
            });
        }

        self.policy_gateway
            .evaluate_tool_policy(&resolved_name, safe_mode_prompt, default_permission)
            .await
    }

    async fn evaluate_mcp_tool_policy(&self, full_name: &str, tool_name: &str) -> Result<ToolPermissionDecision> {
        let provider = match self.find_mcp_provider(tool_name).await {
            Some(provider) => provider,
            None => {
                // Unknown provider for this tool; default to prompt for safety
                return Ok(ToolPermissionDecision::Prompt);
            }
        };

        {
            let gateway = self.policy_gateway.clone();
            if gateway.is_denied_in_full_auto(full_name).await {
                return Ok(ToolPermissionDecision::Deny);
            }
        }

        let mcp_decision = {
            let gateway = self.policy_gateway.clone();
            let policy_arc = gateway.tool_policy_arc();
            drop(gateway);
            let policy_guard = policy_arc.lock().await;
            policy_guard
                .as_ref()
                .map(|m| m.get_mcp_tool_policy(&provider, tool_name))
                .unwrap_or(ToolPolicy::Prompt)
        };
        match mcp_decision {
            ToolPolicy::Allow => {
                self.policy_gateway.preapprove(full_name).await;
                Ok(ToolPermissionDecision::Allow)
            }
            ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
            ToolPolicy::Prompt => Ok(ToolPermissionDecision::Prompt),
        }
    }

    /// Mark a tool as pre-approved for a single execution after the permission
    /// flow already granted it.
    pub async fn mark_tool_preapproved(&self, name: &str) {
        let normalized_name = self.resolve_runtime_policy_name(name);
        self.policy_gateway.preapprove(&normalized_name).await;
        tracing::trace!(tool = %normalized_name, "Preapproved tool after explicit approval");
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
            let Some((provider, tool_name)) = parse_canonical_mcp_tool_name(resolution.registration_name()) else {
                return Ok(());
            };
            (provider.to_string(), tool_name.to_string())
        } else {
            return Ok(());
        };

        self.policy_gateway.persist_mcp_tool_policy(&provider, &tool_name, policy).await
    }
}
