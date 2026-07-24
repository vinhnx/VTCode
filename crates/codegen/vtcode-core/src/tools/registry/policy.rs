use rustc_hash::FxHashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::config::constants::tools;
use crate::config::mcp::McpAllowListConfig;
use crate::tool_policy::{ToolExecutionDecision, ToolPolicy, ToolPolicyManager};
use crate::tools::handlers::SessionToolsConfig;
use crate::tools::names::canonical_tool_name;
use crate::tools::tool_intent::file_operation_action_is;

const ERR_POLICY_MANAGER_NOT_INITIALIZED: &str = "Tool policy manager not initialized";

#[cfg(test)]
#[derive(Clone, Default)]
pub(super) struct PolicyCatalogueTestPause {
    reached: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}

#[cfg(test)]
impl PolicyCatalogueTestPause {
    async fn pause(&self) {
        self.reached.notify_one();
        self.resume.notified().await;
    }

    pub(super) async fn wait_until_reached(&self) {
        self.reached.notified().await;
    }

    pub(super) fn resume(&self) {
        self.resume.notify_one();
    }
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct PolicyCatalogueTestHooks {
    before_enable_lifecycle: parking_lot::Mutex<Option<PolicyCatalogueTestPause>>,
    before_disable_lifecycle: parking_lot::Mutex<Option<PolicyCatalogueTestPause>>,
    after_enable_snapshot: parking_lot::Mutex<Option<PolicyCatalogueTestPause>>,
    after_refresh_snapshot: parking_lot::Mutex<Option<PolicyCatalogueTestPause>>,
}

#[cfg(test)]
impl PolicyCatalogueTestHooks {
    pub(super) fn install_before_enable_lifecycle(&self, pause: PolicyCatalogueTestPause) {
        *self.before_enable_lifecycle.lock() = Some(pause);
    }

    pub(super) fn install_before_disable_lifecycle(&self, pause: PolicyCatalogueTestPause) {
        *self.before_disable_lifecycle.lock() = Some(pause);
    }

    pub(super) fn install_after_enable_snapshot(&self, pause: PolicyCatalogueTestPause) {
        *self.after_enable_snapshot.lock() = Some(pause);
    }

    pub(super) fn install_after_refresh_snapshot(&self, pause: PolicyCatalogueTestPause) {
        *self.after_refresh_snapshot.lock() = Some(pause);
    }

    pub(super) async fn pause_before_enable_lifecycle(&self) {
        let pause = self.before_enable_lifecycle.lock().take();
        if let Some(pause) = pause {
            pause.pause().await;
        }
    }

    pub(super) async fn pause_before_disable_lifecycle(&self) {
        let pause = self.before_disable_lifecycle.lock().take();
        if let Some(pause) = pause {
            pause.pause().await;
        }
    }

    pub(super) async fn pause_after_enable_snapshot(&self) {
        let pause = self.after_enable_snapshot.lock().take();
        if let Some(pause) = pause {
            pause.pause().await;
        }
    }

    pub(super) async fn pause_after_refresh_snapshot(&self) {
        let pause = self.after_refresh_snapshot.lock().take();
        if let Some(pause) = pause {
            pause.pause().await;
        }
    }
}

use super::ToolPermissionDecision;
use super::risk_scorer::{RiskLevel, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust};

#[derive(Default)]
pub(super) struct ToolPolicyGateway {
    tool_policy: Arc<Mutex<Option<ToolPolicyManager>>>,
    preapproved_tools: Arc<Mutex<FxHashSet<String>>>,
    full_auto_allowlist: Arc<Mutex<Option<FxHashSet<String>>>>,
    full_auto_catalogue_config: Arc<Mutex<Option<SessionToolsConfig>>>,
    full_auto_catalogue_lifecycle: Arc<Mutex<()>>,
    #[cfg(test)]
    full_auto_catalogue_test_hooks: Arc<PolicyCatalogueTestHooks>,
    enforce_safe_mode_prompts: AtomicBool,
}

impl Clone for ToolPolicyGateway {
    fn clone(&self) -> Self {
        Self {
            tool_policy: Arc::clone(&self.tool_policy),
            preapproved_tools: Arc::clone(&self.preapproved_tools),
            full_auto_allowlist: self.full_auto_allowlist.clone(),
            full_auto_catalogue_config: self.full_auto_catalogue_config.clone(),
            full_auto_catalogue_lifecycle: Arc::clone(&self.full_auto_catalogue_lifecycle),
            #[cfg(test)]
            full_auto_catalogue_test_hooks: Arc::clone(&self.full_auto_catalogue_test_hooks),
            enforce_safe_mode_prompts: AtomicBool::new(self.enforce_safe_mode_prompts.load(Ordering::Relaxed)),
        }
    }
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
            tool_policy: Arc::new(Mutex::new(tool_policy)),
            preapproved_tools: Arc::new(Mutex::new(FxHashSet::default())),
            full_auto_allowlist: Arc::new(Mutex::new(None)),
            full_auto_catalogue_config: Arc::new(Mutex::new(None)),
            full_auto_catalogue_lifecycle: Arc::new(Mutex::new(())),
            #[cfg(test)]
            full_auto_catalogue_test_hooks: Arc::new(PolicyCatalogueTestHooks::default()),
            enforce_safe_mode_prompts: AtomicBool::new(false),
        }
    }

    pub fn with_policy_manager(manager: ToolPolicyManager) -> Self {
        Self {
            tool_policy: Arc::new(Mutex::new(Some(manager))),
            preapproved_tools: Arc::new(Mutex::new(FxHashSet::default())),
            full_auto_allowlist: Arc::new(Mutex::new(None)),
            full_auto_catalogue_config: Arc::new(Mutex::new(None)),
            full_auto_catalogue_lifecycle: Arc::new(Mutex::new(())),
            #[cfg(test)]
            full_auto_catalogue_test_hooks: Arc::new(PolicyCatalogueTestHooks::default()),
            enforce_safe_mode_prompts: AtomicBool::new(false),
        }
    }

    pub fn set_enforce_safe_mode_prompts(&self, enabled: bool) {
        self.enforce_safe_mode_prompts.store(enabled, Ordering::Relaxed);
    }

    fn requires_safe_mode_prompt(&self, safe_mode_prompt: bool) -> bool {
        self.enforce_safe_mode_prompts.load(Ordering::Relaxed) && safe_mode_prompt
    }

    pub async fn has_policy_manager(&self) -> bool {
        self.tool_policy.lock().await.is_some()
    }

    pub async fn sync_available_tools(&self, mut available: Vec<String>, mcp_keys: &[String]) {
        available.extend(mcp_keys.iter().cloned());
        available.sort();
        available.dedup();

        if let Some(ref mut policy) = *self.tool_policy.lock().await
            && let Err(err) = policy.update_available_tools(available).await
        {
            tracing::warn!(%err, "Failed to update tool policies");
        }
    }

    pub async fn apply_policy_constraints(&self, name: &str, args: &Value) -> Result<Value> {
        let mut args = args.clone();
        let canonical = canonical_tool_name(name);
        let normalized = canonical;
        let file_operation_read = normalized == tools::UNIFIED_FILE && file_operation_action_is(&args, "read");

        if let Some(constraints) = self
            .tool_policy
            .lock()
            .await
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
                n if n == tools::CODE_SEARCH => {
                    let valid_cap = constraints.max_results_per_call.filter(|cap| (1..=100).contains(cap));
                    match (obj.get("max_results"), valid_cap) {
                        (None, cap) => {
                            obj.insert("max_results".to_string(), json!(cap.map_or(20, |cap| 20.min(cap))));
                        }
                        (Some(value), Some(cap)) if value.as_u64().is_some_and(|value| value > cap as u64) => {
                            obj.insert("max_results".to_string(), json!(cap));
                        }
                        (Some(_), _) => {}
                    }
                }
                n if n == tools::READ_FILE || file_operation_read => {
                    if let Some(cap) = constraints.max_bytes_per_read {
                        let requested = obj.get("max_bytes").and_then(|v| v.as_u64()).unwrap_or(cap as u64) as usize;
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

    pub async fn policy_manager(&self) -> Option<ToolPolicyManager> {
        self.tool_policy.lock().await.clone()
    }

    pub async fn set_policy_manager(&self, manager: ToolPolicyManager) {
        *self.tool_policy.lock().await = Some(manager);
    }

    pub async fn set_tool_policy(&self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        let canonical = canonical_tool_name(tool_name);
        if let Some(ref mut manager) = *self.tool_policy.lock().await {
            manager.set_policy(canonical, policy).await
        } else {
            Err(anyhow!(ERR_POLICY_MANAGER_NOT_INITIALIZED))
        }
    }

    pub async fn add_approval_cache_key(&self, approval_key: &str) -> Result<()> {
        if let Some(ref mut manager) = *self.tool_policy.lock().await {
            manager.add_approval_cache_key_with_segments(approval_key).await
        } else {
            Err(anyhow!(ERR_POLICY_MANAGER_NOT_INITIALIZED))
        }
    }

    pub async fn add_approval_cache_prefix(&self, prefix_entry: &str) -> Result<()> {
        if let Some(ref mut manager) = *self.tool_policy.lock().await {
            manager.add_approval_cache_prefix(prefix_entry).await
        } else {
            Err(anyhow!(ERR_POLICY_MANAGER_NOT_INITIALIZED))
        }
    }

    pub async fn has_approval_cache_key(&self, approval_key: &str) -> bool {
        self.tool_policy
            .lock()
            .await
            .as_ref()
            .is_some_and(|manager| manager.has_approval_cache_key(approval_key))
    }

    pub async fn matching_shell_approval_prefix(
        &self,
        command_words: &[String],
        scope_signature: &str,
    ) -> Option<String> {
        self.tool_policy
            .lock()
            .await
            .as_ref()
            .and_then(|manager| manager.matching_shell_approval_prefix(command_words, scope_signature))
    }

    pub async fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        let canonical = canonical_tool_name(tool_name);
        self.tool_policy
            .lock()
            .await
            .as_ref()
            .map(|tp| tp.get_policy(canonical))
            .unwrap_or(ToolPolicy::Allow)
    }

    pub async fn print_policy_status(&self) {
        if let Some(tp) = self.tool_policy.lock().await.as_ref() {
            tp.print_status();
        } else {
            tracing::warn!("{}", ERR_POLICY_MANAGER_NOT_INITIALIZED);
        }
    }

    pub async fn enable_full_auto_permission(
        &self,
        allowed_tools: &[String],
        available_tools: &[String],
        session_tools_config: SessionToolsConfig,
    ) {
        let mut normalized: FxHashSet<String> = FxHashSet::default();
        let wildcard = allowed_tools.iter().any(|tool| tool.trim() == tools::WILDCARD_ALL);
        if wildcard {
            for tool in available_tools {
                let canonical = canonical_tool_name(tool);
                normalized.insert(canonical.to_owned());
            }
        } else {
            for tool in allowed_tools {
                let trimmed = tool.trim();
                if !trimmed.is_empty() {
                    let canonical = canonical_tool_name(trimmed);
                    normalized.insert(canonical.to_owned());
                }
            }
        }

        *self.full_auto_allowlist.lock().await = Some(normalized);
        *self.full_auto_catalogue_config.lock().await = wildcard.then_some(session_tools_config);
    }

    pub async fn disable_full_auto_permission(&self) {
        *self.full_auto_allowlist.lock().await = None;
        *self.full_auto_catalogue_config.lock().await = None;
    }

    pub async fn full_auto_catalogue_config(&self) -> Option<SessionToolsConfig> {
        self.full_auto_catalogue_config.lock().await.clone()
    }

    pub fn full_auto_catalogue_lifecycle(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.full_auto_catalogue_lifecycle)
    }

    pub(crate) fn tool_policy_arc(&self) -> Arc<Mutex<Option<ToolPolicyManager>>> {
        Arc::clone(&self.tool_policy)
    }

    #[cfg(test)]
    pub(super) fn full_auto_catalogue_test_hooks(&self) -> Arc<PolicyCatalogueTestHooks> {
        Arc::clone(&self.full_auto_catalogue_test_hooks)
    }

    pub async fn refresh_full_auto_catalogue(
        &self,
        session_tools_config: &SessionToolsConfig,
        available_tools: &[String],
    ) {
        if self.full_auto_catalogue_config.lock().await.as_ref() != Some(session_tools_config) {
            return;
        }

        *self.full_auto_allowlist.lock().await = Some(
            available_tools
                .iter()
                .map(|tool| canonical_tool_name(tool).to_owned())
                .collect(),
        );
    }

    pub async fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.full_auto_allowlist.lock().await.as_ref().map(|set| {
            let mut items: Vec<String> = set.iter().cloned().collect();
            items.sort();
            items
        })
    }

    pub async fn is_allowed_in_full_auto(&self, name: &str) -> bool {
        let canonical = canonical_tool_name(name);
        self.full_auto_allowlist
            .lock()
            .await
            .as_ref()
            .map(|allowlist| allowlist.contains(canonical))
            .unwrap_or(true)
    }

    pub async fn is_denied_in_full_auto(&self, name: &str) -> bool {
        let allowlist = self.full_auto_allowlist.lock().await;
        if let Some(allowlist) = &*allowlist {
            let canonical = canonical_tool_name(name);
            !allowlist.contains(canonical)
        } else {
            false
        }
    }

    pub async fn evaluate_tool_policy(
        &self,
        name: &str,
        safe_mode_prompt: bool,
        default_permission: ToolPolicy,
    ) -> Result<ToolPermissionDecision> {
        let canonical = canonical_tool_name(name);
        let normalized = canonical;

        if self.requires_safe_mode_prompt(safe_mode_prompt) {
            tracing::debug!("Tool '{}' requires prompt in safe mode (tools_policy)", normalized);
            return Ok(ToolPermissionDecision::Prompt);
        }

        let allowlist = self.full_auto_allowlist.lock().await;
        let has_allowlist = allowlist.is_some();
        if let Some(allowlist) = &*allowlist
            && !allowlist.contains(normalized)
        {
            return Ok(ToolPermissionDecision::Deny);
        }
        drop(allowlist);

        if has_allowlist {
            if let Some(policy_manager) = self.tool_policy.lock().await.as_mut() {
                match policy_manager.get_policy(normalized) {
                    ToolPolicy::Deny => return Ok(ToolPermissionDecision::Deny),
                    ToolPolicy::Allow => {
                        self.preapproved_tools.lock().await.insert(normalized.to_string());
                        return Ok(ToolPermissionDecision::Allow);
                    }
                    ToolPolicy::Prompt => {
                        return Ok(ToolPermissionDecision::Prompt);
                    }
                }
            }

            self.preapproved_tools.lock().await.insert(normalized.to_string());
            return Ok(ToolPermissionDecision::Allow);
        }

        if let Some(policy_manager) = self.tool_policy.lock().await.as_mut() {
            match policy_manager.get_policy(normalized) {
                ToolPolicy::Allow => {
                    self.preapproved_tools.lock().await.insert(normalized.to_string());
                    Ok(ToolPermissionDecision::Allow)
                }
                ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
                ToolPolicy::Prompt => {
                    if Self::should_auto_approve_by_risk(normalized)
                        || ToolPolicyManager::is_auto_allow_tool(normalized)
                    {
                        policy_manager.set_policy(normalized, ToolPolicy::Allow).await?;
                        self.preapproved_tools.lock().await.insert(normalized.to_string());
                        Ok(ToolPermissionDecision::Allow)
                    } else {
                        Ok(ToolPermissionDecision::Prompt)
                    }
                }
            }
        } else {
            Ok(match default_permission {
                ToolPolicy::Allow => {
                    self.preapproved_tools.lock().await.insert(normalized.to_string());
                    ToolPermissionDecision::Allow
                }
                ToolPolicy::Deny => ToolPermissionDecision::Deny,
                ToolPolicy::Prompt => ToolPermissionDecision::Prompt,
            })
        }
    }

    /// Determine if a tool should be auto-approved based on risk level
    /// Low-risk read-only tools are auto-approved to reduce approval friction
    fn should_auto_approve_by_risk(tool_name: &str) -> bool {
        let mut ctx = ToolRiskContext::new(tool_name.to_string(), ToolSource::Internal, WorkspaceTrust::Trusted);
        // Reflect outbound network access in the score so network tools are not
        // silently auto-approved. Without this, the trusted-workspace risk
        // reduction can drop a network tool below the low-risk threshold and
        // bypass human-in-the-loop approval.
        if ToolRiskScorer::is_network_tool(tool_name) {
            ctx = ctx.accesses_network();
        }
        let risk = ToolRiskScorer::calculate_risk(&ctx);
        // Auto-approve only low-risk tools
        matches!(risk, RiskLevel::Low)
    }

    pub async fn take_preapproved(&self, name: &str) -> bool {
        let canonical = canonical_tool_name(name);
        let mut preapproved = self.preapproved_tools.lock().await;
        let was_preapproved = preapproved.remove(canonical);
        tracing::trace!(
            "take_preapproved: tool='{}', canonical='{}', was_preapproved={}, remaining={:?}",
            name,
            canonical,
            was_preapproved,
            *preapproved
        );
        was_preapproved
    }

    pub async fn preapprove(&self, name: &str) {
        let canonical = canonical_tool_name(name);
        let canonical_owned = canonical.to_owned();
        let mut preapproved = self.preapproved_tools.lock().await;
        preapproved.insert(canonical_owned.clone());
        tracing::trace!(
            "preapprove: tool='{}', canonical='{}', preapproved_tools={:?}",
            name,
            canonical_owned,
            *preapproved
        );
    }

    pub async fn should_execute_tool(&self, name: &str) -> Result<ToolExecutionDecision> {
        let canonical = canonical_tool_name(name);
        if let Some(policy_manager) = self.tool_policy.lock().await.as_mut() {
            policy_manager.should_execute_tool(canonical).await
        } else {
            Ok(ToolExecutionDecision::Allowed)
        }
    }

    pub async fn update_mcp_tools(
        &self,
        mcp_tool_index: &hashbrown::HashMap<String, Vec<String>>,
    ) -> Result<Option<McpAllowListConfig>> {
        if let Some(policy_manager) = self.tool_policy.lock().await.as_mut() {
            policy_manager.update_mcp_tools(mcp_tool_index).await?;
            return Ok(Some(policy_manager.mcp_allowlist().clone()));
        }
        Ok(None)
    }

    pub async fn persist_mcp_tool_policy(&self, provider: &str, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        if let Some(manager) = self.tool_policy.lock().await.as_mut() {
            manager.set_mcp_tool_policy(provider, tool_name, policy).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_policy::{ToolConstraints, ToolPolicyConfig};
    use indexmap::IndexMap;
    use serde_json::json;

    async fn gateway_with_constraints(tool_name: &str, constraints: ToolConstraints) -> ToolPolicyGateway {
        let temp = tempfile::tempdir().expect("temp workspace");
        let config_path = temp.path().join("tool-policy.json");
        let config = ToolPolicyConfig {
            constraints: IndexMap::from([(tool_name.to_string(), constraints)]),
            ..ToolPolicyConfig::default()
        };
        std::fs::write(&config_path, serde_json::to_vec_pretty(&config).expect("policy config json"))
            .expect("policy config file");
        let manager = ToolPolicyManager::new_with_config_path(config_path)
            .await
            .expect("policy manager");
        ToolPolicyGateway::with_policy_manager(manager)
    }

    #[test]
    fn network_tools_are_not_auto_approved_by_risk() {
        // Web fetches must require HITL approval and never be auto-promoted to
        // Allow by the low-risk auto-approval heuristic.
        assert!(!ToolPolicyGateway::should_auto_approve_by_risk(tools::WEB_FETCH));
        // The read-only code search tool stays auto-approved for low friction.
        assert!(ToolPolicyGateway::should_auto_approve_by_risk(tools::CODE_SEARCH));
    }

    #[test]
    fn mcp_connect_and_disconnect_are_not_auto_approved_by_risk() {
        // `mcp` itself is a low-risk discovery tool (Allow by default), but its
        // connect/disconnect actions open/tear down a network connection and
        // must stay HITL-gated via the action-qualified policy keys, never
        // auto-promoted to Allow by the low-risk auto-approval heuristic.
        assert!(!ToolPolicyGateway::should_auto_approve_by_risk("mcp:connect"));
        assert!(!ToolPolicyGateway::should_auto_approve_by_risk("mcp:disconnect"));
        // The bare mcp tool (discovery actions) stays auto-approved for low
        // friction.
        assert!(ToolPolicyGateway::should_auto_approve_by_risk(tools::MCP));
    }

    #[tokio::test]
    async fn apply_policy_constraints_caps_code_search_results() {
        let gateway = gateway_with_constraints(
            tools::CODE_SEARCH,
            ToolConstraints {
                max_results_per_call: Some(15),
                ..ToolConstraints::default()
            },
        )
        .await;

        let constrained = gateway
            .apply_policy_constraints(
                tools::CODE_SEARCH,
                &json!({
                    "query": "ToolRegistry",
                    "path": ".",
                    "max_results": 50
                }),
            )
            .await
            .expect("constrained args");

        assert_eq!(constrained["max_results"], json!(15));
        assert!(constrained.get("_policy_note").is_none());
    }

    #[tokio::test]
    async fn omitted_code_search_limit_uses_public_default_bounded_by_valid_policy_cap() {
        for (configured_cap, expected_limit) in [(15, 15), (50, 20), (0, 20), (101, 20)] {
            let gateway = gateway_with_constraints(
                tools::CODE_SEARCH,
                ToolConstraints {
                    max_results_per_call: Some(configured_cap),
                    ..ToolConstraints::default()
                },
            )
            .await;

            let constrained = gateway
                .apply_policy_constraints(tools::CODE_SEARCH, &json!({"query": "ToolRegistry", "path": "."}))
                .await
                .expect("valid omitted max_results must remain valid under policy");

            assert_eq!(constrained["max_results"], json!(expected_limit), "configured cap {configured_cap}");
        }
    }

    #[tokio::test]
    async fn explicit_code_search_limit_retains_valid_cap_clamping() {
        let gateway = gateway_with_constraints(
            tools::CODE_SEARCH,
            ToolConstraints {
                max_results_per_call: Some(50),
                ..ToolConstraints::default()
            },
        )
        .await;
        let constrained = gateway
            .apply_policy_constraints(tools::CODE_SEARCH, &json!({"query": "ToolRegistry", "max_results": 80}))
            .await
            .expect("constrained args");
        assert_eq!(constrained["max_results"], json!(50));
    }
}
