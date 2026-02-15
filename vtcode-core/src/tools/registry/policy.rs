use rustc_hash::FxHashSet;
use std::path::Path;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::config::mcp::McpAllowListConfig;
use crate::tool_policy::{ToolExecutionDecision, ToolPolicy, ToolPolicyManager};
use crate::tools::names::canonical_tool_name;

use super::ToolPermissionDecision;
use super::risk_scorer::{RiskLevel, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust};

const ALWAYS_PROMPT_IN_SAFE_MODE: &[&str] = &[
    tools::RUN_PTY_CMD,
    tools::SEND_PTY_INPUT,
    tools::CREATE_PTY_SESSION,
    tools::WRITE_FILE,
    tools::EDIT_FILE,
    tools::CREATE_FILE,
    tools::DELETE_FILE,
    tools::APPLY_PATCH,
    "exec",
    "shell",
    "unified_exec",
    "exec_pty_cmd",
];

#[derive(Clone, Default)]
pub(super) struct ToolPolicyGateway {
    tool_policy: Option<ToolPolicyManager>,
    preapproved_tools: FxHashSet<String>,
    full_auto_allowlist: Option<FxHashSet<String>>,
    enforce_safe_mode_prompts: bool,
}

impl ToolPolicyGateway {
    pub async fn new(workspace_root: &Path) -> Self {
        let tool_policy = match ToolPolicyManager::new_with_workspace(workspace_root).await {
            Ok(manager) => Some(manager),
            Err(err) => {
                tracing::warn!(%err, "Failed to initialize tool policy manager");
                None
            }
        };

        Self {
            tool_policy,
            preapproved_tools: FxHashSet::default(),
            full_auto_allowlist: None,
            enforce_safe_mode_prompts: false,
        }
    }

    pub fn with_policy_manager(manager: ToolPolicyManager) -> Self {
        Self {
            tool_policy: Some(manager),
            preapproved_tools: FxHashSet::default(),
            full_auto_allowlist: None,
            enforce_safe_mode_prompts: false,
        }
    }

    pub fn set_enforce_safe_mode_prompts(&mut self, enabled: bool) {
        self.enforce_safe_mode_prompts = enabled;
    }

    fn requires_safe_mode_prompt(&self, tool_name: &str) -> bool {
        if !self.enforce_safe_mode_prompts {
            return false;
        }
        let canonical = canonical_tool_name(tool_name);
        ALWAYS_PROMPT_IN_SAFE_MODE.contains(&canonical.as_ref())
    }

    pub fn has_policy_manager(&self) -> bool {
        self.tool_policy.is_some()
    }

    pub async fn sync_available_tools(&mut self, mut available: Vec<String>, mcp_keys: &[String]) {
        available.extend(mcp_keys.iter().cloned());
        available.sort();
        available.dedup();

        if let Some(ref mut policy) = self.tool_policy
            && let Err(err) = policy.update_available_tools(available).await
        {
            tracing::warn!(%err, "Failed to update tool policies");
        }
    }

    pub fn apply_policy_constraints(&self, name: &str, args: &Value) -> Result<Value> {
        let mut args = args.clone();
        let canonical = canonical_tool_name(name);
        let normalized = canonical.as_ref();

        if let Some(constraints) = self
            .tool_policy
            .as_ref()
            .and_then(|tp| tp.get_constraints(normalized))
            .cloned()
        {
            let obj = args
                .as_object_mut()
                .ok_or_else(|| anyhow!("Error: tool arguments must be an object"))?;

            if let Some(fmt) = constraints.default_response_format {
                obj.entry("response_format").or_insert(json!(fmt));
            }

            if let Some(allowed) = constraints.allowed_modes
                && let Some(mode) = obj.get("mode").and_then(|v| v.as_str())
                && !allowed.iter().any(|m| m == mode)
            {
                return Err(anyhow!(
                    "Mode '{}' not allowed by policy for '{}'. Allowed: {}",
                    mode,
                    normalized,
                    allowed.join(", ")
                ));
            }

            match normalized {
                n if n == tools::LIST_FILES => {
                    if let Some(cap) = constraints.max_items_per_call {
                        let requested = obj
                            .get("max_items")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cap as u64) as usize;
                        if requested > cap {
                            obj.insert("max_items".to_string(), json!(cap));
                            obj.insert(
                                "_policy_note".to_string(),
                                json!(format!("Capped max_items to {} by policy", cap)),
                            );
                        }
                    }
                }
                n if n == tools::GREP_FILE => {
                    if let Some(cap) = constraints.max_results_per_call {
                        let requested = obj
                            .get("max_results")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cap as u64) as usize;
                        if requested > cap {
                            obj.insert("max_results".to_string(), json!(cap));
                            obj.insert(
                                "_policy_note".to_string(),
                                json!(format!("Capped max_results to {} by policy", cap)),
                            );
                        }
                    }
                }
                n if n == tools::READ_FILE => {
                    if let Some(cap) = constraints.max_bytes_per_read {
                        let requested = obj
                            .get("max_bytes")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cap as u64) as usize;
                        if requested > cap {
                            obj.insert("max_bytes".to_string(), json!(cap));
                            obj.insert(
                                "_policy_note".to_string(),
                                json!(format!("Capped max_bytes to {} by policy", cap)),
                            );
                        }
                    }
                }

                _ => {}
            }
        }

        Ok(args)
    }

    pub fn policy_manager_mut(&mut self) -> Result<&mut ToolPolicyManager> {
        self.tool_policy
            .as_mut()
            .ok_or_else(|| anyhow!("Tool policy manager not available"))
    }

    pub fn set_policy_manager(&mut self, manager: ToolPolicyManager) {
        self.tool_policy = Some(manager);
    }

    pub async fn set_tool_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        let canonical = canonical_tool_name(tool_name);
        if let Some(ref mut manager) = self.tool_policy {
            manager.set_policy(canonical.as_ref(), policy).await
        } else {
            Err(anyhow::anyhow!("Tool policy manager not initialized"))
        }
    }

    pub fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        let canonical = canonical_tool_name(tool_name);
        self.tool_policy
            .as_ref()
            .map(|tp| tp.get_policy(canonical.as_ref()))
            .unwrap_or(ToolPolicy::Allow)
    }

    pub async fn reset_tool_policies(&mut self) -> Result<()> {
        if let Some(tp) = self.tool_policy.as_mut() {
            tp.reset_all_to_prompt().await
        } else {
            Err(anyhow!("Tool policy manager not available"))
        }
    }

    pub async fn allow_all_tools(&mut self) -> Result<()> {
        if let Some(tp) = self.tool_policy.as_mut() {
            tp.allow_all_tools().await
        } else {
            Err(anyhow!("Tool policy manager not available"))
        }
    }

    pub async fn deny_all_tools(&mut self) -> Result<()> {
        if let Some(tp) = self.tool_policy.as_mut() {
            tp.deny_all_tools().await
        } else {
            Err(anyhow!("Tool policy manager not available"))
        }
    }

    pub fn print_policy_status(&self) {
        if let Some(tp) = self.tool_policy.as_ref() {
            tp.print_status();
        } else {
            tracing::warn!("Tool policy manager not available");
        }
    }

    pub fn enable_full_auto_mode(&mut self, allowed_tools: &[String], available_tools: &[String]) {
        let mut normalized: FxHashSet<String> = FxHashSet::default();
        if allowed_tools
            .iter()
            .any(|tool| tool.trim() == tools::WILDCARD_ALL)
        {
            for tool in available_tools {
                let canonical = canonical_tool_name(tool);
                normalized.insert(canonical.into_owned());
            }
        } else {
            for tool in allowed_tools {
                let trimmed = tool.trim();
                if !trimmed.is_empty() {
                    let canonical = canonical_tool_name(trimmed);
                    normalized.insert(canonical.into_owned());
                }
            }
        }

        self.full_auto_allowlist = Some(normalized);
    }

    pub fn disable_full_auto_mode(&mut self) {
        self.full_auto_allowlist = None;
    }

    pub fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.full_auto_allowlist.as_ref().map(|set| {
            let mut items: Vec<String> = set.iter().cloned().collect();
            items.sort();
            items
        })
    }

    pub fn is_allowed_in_full_auto(&self, name: &str) -> bool {
        let canonical = canonical_tool_name(name);
        self.full_auto_allowlist
            .as_ref()
            .map(|allowlist| allowlist.contains(&*canonical))
            .unwrap_or(true)
    }

    pub fn has_full_auto_allowlist(&self) -> bool {
        self.full_auto_allowlist.is_some()
    }

    pub async fn evaluate_tool_policy(&mut self, name: &str) -> Result<ToolPermissionDecision> {
        let canonical = canonical_tool_name(name);
        let normalized = canonical.as_ref();

        // In safe mode (tools_policy), high-risk tools always require a prompt
        // regardless of persisted policy
        if self.requires_safe_mode_prompt(normalized) {
            tracing::debug!(
                "Tool '{}' requires prompt in safe mode (tools_policy)",
                normalized
            );
            return Ok(ToolPermissionDecision::Prompt);
        }

        if let Some(allowlist) = self.full_auto_allowlist.as_ref() {
            if !allowlist.contains(normalized) {
                return Ok(ToolPermissionDecision::Deny);
            }

            if let Some(policy_manager) = self.tool_policy.as_mut() {
                match policy_manager.get_policy(normalized) {
                    ToolPolicy::Deny => return Ok(ToolPermissionDecision::Deny),
                    ToolPolicy::Allow => {
                        self.preapproved_tools.insert(normalized.to_string());
                        return Ok(ToolPermissionDecision::Allow);
                    }
                    ToolPolicy::Prompt => {
                        // Always prompt for explicit "prompt" policy, even in full-auto mode
                        // This ensures human-in-the-loop approval for sensitive operations
                        return Ok(ToolPermissionDecision::Prompt);
                    }
                }
            }

            self.preapproved_tools.insert(normalized.to_string());
            return Ok(ToolPermissionDecision::Allow);
        }

        if let Some(policy_manager) = self.tool_policy.as_mut() {
            match policy_manager.get_policy(normalized) {
                ToolPolicy::Allow => {
                    self.preapproved_tools.insert(normalized.to_string());
                    Ok(ToolPermissionDecision::Allow)
                }
                ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
                ToolPolicy::Prompt => {
                    // Check if low-risk by using risk scorer
                    if Self::should_auto_approve_by_risk(normalized)
                        || ToolPolicyManager::is_auto_allow_tool(normalized)
                    {
                        policy_manager
                            .set_policy(normalized, ToolPolicy::Allow)
                            .await?;
                        self.preapproved_tools.insert(normalized.to_string());
                        Ok(ToolPermissionDecision::Allow)
                    } else {
                        Ok(ToolPermissionDecision::Prompt)
                    }
                }
            }
        } else {
            self.preapproved_tools.insert(normalized.to_string());
            Ok(ToolPermissionDecision::Allow)
        }
    }

    /// Determine if a tool should be auto-approved based on risk level
    /// Low-risk read-only tools are auto-approved to reduce approval friction
    fn should_auto_approve_by_risk(tool_name: &str) -> bool {
        let ctx = ToolRiskContext::new(
            tool_name.to_string(),
            ToolSource::Internal,
            WorkspaceTrust::Trusted,
        );
        let risk = ToolRiskScorer::calculate_risk(&ctx);
        // Auto-approve only low-risk tools
        matches!(risk, RiskLevel::Low)
    }

    pub fn take_preapproved(&mut self, name: &str) -> bool {
        let canonical = canonical_tool_name(name);
        let was_preapproved = self.preapproved_tools.remove(&*canonical);
        tracing::debug!(
            "take_preapproved: tool='{}', canonical='{}', was_preapproved={}, remaining={:?}",
            name,
            canonical,
            was_preapproved,
            self.preapproved_tools
        );
        was_preapproved
    }

    pub fn preapprove(&mut self, name: &str) {
        let canonical = canonical_tool_name(name);
        let canonical_owned = canonical.into_owned();
        self.preapproved_tools.insert(canonical_owned.clone());
        tracing::debug!(
            "preapprove: tool='{}', canonical='{}', preapproved_tools={:?}",
            name,
            canonical_owned,
            self.preapproved_tools
        );
    }

    pub async fn should_execute_tool(&mut self, name: &str) -> Result<ToolExecutionDecision> {
        let canonical = canonical_tool_name(name);
        if let Some(policy_manager) = self.tool_policy.as_mut() {
            policy_manager.should_execute_tool(canonical.as_ref()).await
        } else {
            Ok(ToolExecutionDecision::Allowed)
        }
    }

    pub async fn update_mcp_tools(
        &mut self,
        mcp_tool_index: &std::collections::HashMap<String, Vec<String>>,
    ) -> Result<Option<McpAllowListConfig>> {
        if let Some(policy_manager) = self.tool_policy.as_mut() {
            policy_manager.update_mcp_tools(mcp_tool_index).await?;
            return Ok(Some(policy_manager.mcp_allowlist().clone()));
        }
        Ok(None)
    }

    pub async fn persist_mcp_tool_policy(
        &mut self,
        provider: &str,
        tool_name: &str,
        policy: ToolPolicy,
    ) -> Result<()> {
        if let Some(manager) = self.tool_policy.as_mut() {
            manager
                .set_mcp_tool_policy(provider, tool_name, policy)
                .await?;
        }
        Ok(())
    }
}
