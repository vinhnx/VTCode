use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::config::mcp::McpAllowListConfig;
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::tools::names::canonical_tool_name;

use super::ToolPermissionDecision;
use super::risk_scorer::{RiskLevel, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust};

#[derive(Clone, Default)]
pub(super) struct ToolPolicyGateway {
    tool_policy: Option<ToolPolicyManager>,
    preapproved_tools: HashSet<String>,
    full_auto_allowlist: Option<HashSet<String>>,
}

impl ToolPolicyGateway {
    pub async fn new(workspace_root: &PathBuf) -> Self {
        let tool_policy = match ToolPolicyManager::new_with_workspace(workspace_root).await {
            Ok(manager) => Some(manager),
            Err(err) => {
                eprintln!("Warning: Failed to initialize tool policy manager: {}", err);
                None
            }
        };

        Self {
            tool_policy,
            preapproved_tools: HashSet::new(),
            full_auto_allowlist: None,
        }
    }

    pub fn with_policy_manager(manager: ToolPolicyManager) -> Self {
        Self {
            tool_policy: Some(manager),
            preapproved_tools: HashSet::new(),
            full_auto_allowlist: None,
        }
    }

    pub async fn sync_available_tools(&mut self, mut available: Vec<String>, mcp_keys: &[String]) {
        available.extend(mcp_keys.iter().cloned());
        available.sort();
        available.dedup();

        if let Some(ref mut policy) = self.tool_policy {
            if let Err(err) = policy.update_available_tools(available).await {
                eprintln!("Warning: Failed to update tool policies: {}", err);
            }
        }
    }

    pub fn apply_policy_constraints(&self, name: &str, mut args: Value) -> Result<Value> {
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
                return Err(anyhow!(format!(
                    "Mode '{}' not allowed by policy for '{}'. Allowed: {}",
                    mode,
                    normalized,
                    allowed.join(", ")
                )));
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

    pub fn policy_manager(&self) -> Result<&ToolPolicyManager> {
        self.tool_policy
            .as_ref()
            .ok_or_else(|| anyhow!("Tool policy manager not available"))
    }

    pub fn set_policy_manager(&mut self, manager: ToolPolicyManager) {
        self.tool_policy = Some(manager);
    }

    pub async fn set_tool_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        let canonical = canonical_tool_name(tool_name);
        self.tool_policy
            .as_mut()
            .expect("Tool policy manager not initialized")
            .set_policy(canonical.as_ref(), policy)
            .await
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
            eprintln!("Tool policy manager not available");
        }
    }

    pub fn enable_full_auto_mode(&mut self, allowed_tools: &[String], available_tools: &[String]) {
        let mut normalized: HashSet<String> = HashSet::new();
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
            .map(|allowlist| allowlist.contains(canonical.as_ref()))
            .unwrap_or(true)
    }

    pub fn has_full_auto_allowlist(&self) -> bool {
        self.full_auto_allowlist.is_some()
    }

    pub async fn evaluate_tool_policy(&mut self, name: &str) -> Result<ToolPermissionDecision> {
        let canonical = canonical_tool_name(name);
        let normalized = canonical.as_ref();

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
                    if Self::should_auto_approve_by_risk(normalized) {
                        policy_manager
                            .set_policy(normalized, ToolPolicy::Allow)
                            .await?;
                        self.preapproved_tools.insert(normalized.to_string());
                        Ok(ToolPermissionDecision::Allow)
                    } else if ToolPolicyManager::is_auto_allow_tool(normalized) {
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
        let was_preapproved = self.preapproved_tools.remove(canonical.as_ref());
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

    pub async fn should_execute_tool(&mut self, name: &str) -> Result<bool> {
        let canonical = canonical_tool_name(name);
        if let Some(policy_manager) = self.tool_policy.as_mut() {
            policy_manager.should_execute_tool(canonical.as_ref()).await
        } else {
            Ok(true)
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
