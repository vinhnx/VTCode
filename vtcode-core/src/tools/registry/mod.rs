//! Tool registry and function declarations

mod astgrep;
mod builtins;
mod cache;
mod declarations;
mod error;
mod executors;
mod inventory;
mod legacy;
mod policy;
mod pty;
mod registration;
mod utils;

pub use declarations::{
    build_function_declarations, build_function_declarations_for_level,
    build_function_declarations_with_mode,
};
pub use error::{ToolErrorType, ToolExecutionError, classify_error};
pub use registration::{ToolExecutorFn, ToolHandler, ToolRegistration};

use builtins::register_builtin_tools;
use inventory::ToolInventory;
use policy::ToolPolicyGateway;
use pty::PtySessionManager;
use utils::normalize_tool_output;

use crate::config::PtyConfig;
use crate::config::ToolsConfig;
#[cfg(test)]
use crate::config::constants::tools;
use crate::sandbox::SandboxProfile;
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::tools::ast_grep::AstGrepEngine;
use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::names::{canonical_tool_name, tool_aliases};
use crate::tools::pty::PtyManager;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, warn};

use super::plan::PlanManager;
use crate::mcp_client::{McpClient, McpToolExecutor, McpToolInfo};

#[cfg(test)]
use super::traits::Tool;
#[cfg(test)]
use crate::config::types::CapabilityLevel;

#[derive(Clone)]
pub struct ToolRegistry {
    inventory: ToolInventory,
    policy_gateway: ToolPolicyGateway,
    pty_sessions: PtySessionManager,
    mcp_client: Option<Arc<McpClient>>,
    mcp_tool_index: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionDecision {
    Allow,
    Deny,
    Prompt,
}

impl ToolRegistry {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::build(workspace_root, PtyConfig::default(), true)
    }

    pub fn new_with_config(workspace_root: PathBuf, pty_config: PtyConfig) -> Self {
        Self::build(workspace_root, pty_config, true)
    }

    pub fn new_with_features(workspace_root: PathBuf, todo_planning_enabled: bool) -> Self {
        Self::build(workspace_root, PtyConfig::default(), todo_planning_enabled)
    }

    pub fn new_with_config_and_features(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
    ) -> Self {
        Self::build(workspace_root, pty_config, todo_planning_enabled)
    }

    pub fn new_with_custom_policy(
        workspace_root: PathBuf,
        policy_manager: ToolPolicyManager,
    ) -> Self {
        Self::build_with_policy(
            workspace_root,
            PtyConfig::default(),
            true,
            Some(policy_manager),
        )
    }

    pub fn new_with_custom_policy_and_config(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
        policy_manager: ToolPolicyManager,
    ) -> Self {
        Self::build_with_policy(
            workspace_root,
            pty_config,
            todo_planning_enabled,
            Some(policy_manager),
        )
    }

    fn build(workspace_root: PathBuf, pty_config: PtyConfig, todo_planning_enabled: bool) -> Self {
        Self::build_with_policy(workspace_root, pty_config, todo_planning_enabled, None)
    }

    fn build_with_policy(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
        policy_manager: Option<ToolPolicyManager>,
    ) -> Self {
        let mut inventory = ToolInventory::new(workspace_root.clone());
        register_builtin_tools(&mut inventory, todo_planning_enabled);

        let pty_sessions = PtySessionManager::new(workspace_root.clone(), pty_config);
        inventory.set_pty_manager(pty_sessions.manager().clone());
        let policy_gateway = policy_manager
            .map(ToolPolicyGateway::with_policy_manager)
            .unwrap_or_else(|| ToolPolicyGateway::new(&workspace_root));

        let mut registry = Self {
            inventory,
            policy_gateway,
            pty_sessions,
            mcp_client: None,
            mcp_tool_index: HashMap::new(),
        };

        registry.sync_policy_catalog();
        registry
    }

    fn sync_policy_catalog(&mut self) {
        let mut available = self.inventory.available_tools();
        let mut alias_entries = Vec::new();
        for tool in &available {
            for alias in tool_aliases(tool) {
                alias_entries.push(alias.to_string());
            }
        }
        available.extend(alias_entries);
        let mcp_keys = self.mcp_policy_keys();
        self.policy_gateway
            .sync_available_tools(available, &mcp_keys);
    }

    pub fn register_tool(&mut self, registration: ToolRegistration) -> Result<()> {
        self.inventory.register_tool(registration)?;
        self.sync_policy_catalog();
        Ok(())
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.inventory.available_tools()
    }

    fn mcp_policy_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        for (provider, tools) in &self.mcp_tool_index {
            for tool in tools {
                keys.push(format!("mcp::{}::{}", provider, tool));
            }
        }
        keys
    }

    fn find_mcp_provider(&self, tool_name: &str) -> Option<String> {
        for (provider, tools) in &self.mcp_tool_index {
            if tools.iter().any(|candidate| candidate == tool_name) {
                return Some(provider.clone());
            }
        }
        None
    }

    pub fn enable_full_auto_mode(&mut self, allowed_tools: &[String]) {
        let available = self.available_tools();
        self.policy_gateway
            .enable_full_auto_mode(allowed_tools, &available);
    }

    pub fn disable_full_auto_mode(&mut self) {
        self.policy_gateway.disable_full_auto_mode();
    }

    pub fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.policy_gateway.current_full_auto_allowlist()
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.inventory.has_tool(name)
    }

    pub fn with_ast_grep(mut self, engine: Arc<AstGrepEngine>) -> Self {
        self.inventory.set_ast_grep_engine(engine);
        self.sync_policy_catalog();
        self
    }

    pub fn workspace_root(&self) -> &PathBuf {
        self.inventory.workspace_root()
    }

    pub fn ast_grep_engine(&self) -> Option<&Arc<AstGrepEngine>> {
        self.inventory.ast_grep_engine()
    }

    pub fn file_ops_tool(&self) -> &FileOpsTool {
        self.inventory.file_ops_tool()
    }

    pub fn grep_file_manager(&self) -> Arc<GrepSearchManager> {
        self.inventory.grep_file_manager()
    }

    pub fn pty_manager(&self) -> &PtyManager {
        self.pty_sessions.manager()
    }

    pub fn pty_config(&self) -> &PtyConfig {
        self.pty_sessions.config()
    }

    pub fn can_start_pty_session(&self) -> bool {
        self.pty_sessions.can_start_session()
    }

    pub fn start_pty_session(&self) -> Result<()> {
        self.pty_sessions.start_session()
    }

    pub fn end_pty_session(&self) {
        self.pty_sessions.end_session();
    }

    pub fn active_pty_sessions(&self) -> usize {
        self.pty_sessions.active_sessions()
    }

    pub fn plan_manager(&self) -> PlanManager {
        self.inventory.plan_manager()
    }

    pub fn current_plan(&self) -> crate::tools::TaskPlan {
        self.inventory.plan_manager().snapshot()
    }

    pub fn policy_manager_mut(&mut self) -> Result<&mut ToolPolicyManager> {
        self.policy_gateway.policy_manager_mut()
    }

    pub fn policy_manager(&self) -> Result<&ToolPolicyManager> {
        self.policy_gateway.policy_manager()
    }

    pub fn set_policy_manager(&mut self, manager: ToolPolicyManager) {
        self.policy_gateway.set_policy_manager(manager);
        self.sync_policy_catalog();
    }

    pub fn set_tool_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        self.policy_gateway.set_tool_policy(tool_name, policy)
    }

    pub fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.policy_gateway.get_tool_policy(tool_name)
    }

    pub fn reset_tool_policies(&mut self) -> Result<()> {
        self.policy_gateway.reset_tool_policies()
    }

    pub fn set_bash_sandbox(&mut self, profile: Option<SandboxProfile>) {
        self.inventory.set_bash_sandbox(profile.clone());
        self.pty_sessions.manager().set_sandbox_profile(profile);
    }

    pub fn allow_all_tools(&mut self) -> Result<()> {
        self.policy_gateway.allow_all_tools()
    }

    pub fn deny_all_tools(&mut self) -> Result<()> {
        self.policy_gateway.deny_all_tools()
    }

    pub fn print_policy_status(&self) {
        self.policy_gateway.print_policy_status();
    }

    pub async fn initialize_async(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn apply_config_policies(&mut self, tools_config: &ToolsConfig) -> Result<()> {
        if let Ok(policy_manager) = self.policy_manager_mut() {
            policy_manager.apply_tools_config(tools_config)?;
        }

        Ok(())
    }

    pub async fn execute_tool(&mut self, name: &str, args: Value) -> Result<Value> {
        let canonical_name = canonical_tool_name(name);
        let tool_name = canonical_name.as_ref();
        let display_name = if tool_name == name {
            name.to_string()
        } else {
            format!("{} (alias for {})", name, tool_name)
        };

        if self.policy_gateway.has_full_auto_allowlist()
            && !self.policy_gateway.is_allowed_in_full_auto(tool_name)
        {
            let error = ToolExecutionError::new(
                tool_name.to_string(),
                ToolErrorType::PolicyViolation,
                format!(
                    "Tool '{}' is not permitted while full-auto mode is active",
                    display_name
                ),
            );
            return Ok(error.to_json_value());
        }

        let skip_policy_prompt = self.policy_gateway.take_preapproved(tool_name);

        if !skip_policy_prompt && !self.policy_gateway.should_execute_tool(tool_name)? {
            let error = ToolExecutionError::new(
                tool_name.to_string(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{}' execution denied by policy", display_name),
            );
            return Ok(error.to_json_value());
        }

        let args = match self
            .policy_gateway
            .apply_policy_constraints(tool_name, args)
        {
            Ok(args) => args,
            Err(err) => {
                let error = ToolExecutionError::with_original_error(
                    tool_name.to_string(),
                    ToolErrorType::InvalidParameters,
                    "Failed to apply policy constraints".to_string(),
                    err.to_string(),
                );
                return Ok(error.to_json_value());
            }
        };

        let registration = match self.inventory.registration_for(tool_name) {
            Some(registration) => registration,
            None => {
                // If not found in standard registry, check if it's an MCP tool
                if let Some(mcp_client) = &self.mcp_client {
                    // Check if it's an MCP tool (prefixed with "mcp_")
                    if name.starts_with("mcp_") {
                        let actual_tool_name = &name[4..]; // Remove "mcp_" prefix
                        match mcp_client.has_mcp_tool(actual_tool_name).await {
                            Ok(true) => {
                                debug!(
                                    "MCP tool '{}' found, executing via MCP client",
                                    actual_tool_name
                                );
                                return self.execute_mcp_tool(actual_tool_name, args).await;
                            }
                            Ok(false) => {
                                if let Some(resolved_name) =
                                    self.resolve_mcp_tool_alias(actual_tool_name).await
                                {
                                    if resolved_name != actual_tool_name {
                                        debug!(
                                            "Resolved MCP tool alias '{}' to '{}'",
                                            actual_tool_name, resolved_name
                                        );
                                        return self.execute_mcp_tool(&resolved_name, args).await;
                                    }
                                }

                                // MCP client doesn't have this tool either
                                let error = ToolExecutionError::new(
                                    tool_name.to_string(),
                                    ToolErrorType::ToolNotFound,
                                    format!("Unknown MCP tool: {}", actual_tool_name),
                                );
                                return Ok(error.to_json_value());
                            }
                            Err(e) => {
                                warn!(
                                    "Error checking MCP tool availability for '{}': {}",
                                    actual_tool_name, e
                                );
                                let error = ToolExecutionError::with_original_error(
                                    tool_name.to_string(),
                                    ToolErrorType::ExecutionError,
                                    format!(
                                        "Failed to verify MCP tool '{}' due to provider errors",
                                        actual_tool_name
                                    ),
                                    e.to_string(),
                                );
                                return Ok(error.to_json_value());
                            }
                        }
                    } else {
                        // Check if MCP client has a tool with this exact name
                        match mcp_client.has_mcp_tool(tool_name).await {
                            Ok(true) => {
                                debug!(
                                    "Tool '{}' not found in registry, delegating to MCP client",
                                    tool_name
                                );
                                return self.execute_mcp_tool(tool_name, args).await;
                            }
                            Ok(false) => {
                                // MCP client doesn't have this tool either
                                let error = ToolExecutionError::new(
                                    tool_name.to_string(),
                                    ToolErrorType::ToolNotFound,
                                    format!("Unknown tool: {}", display_name),
                                );
                                return Ok(error.to_json_value());
                            }
                            Err(e) => {
                                warn!(
                                    "Error checking MCP tool availability for '{}': {}",
                                    tool_name, e
                                );
                                let error = ToolExecutionError::with_original_error(
                                    tool_name.to_string(),
                                    ToolErrorType::ExecutionError,
                                    format!(
                                        "Failed to verify MCP tool '{}' due to provider errors",
                                        tool_name
                                    ),
                                    e.to_string(),
                                );
                                return Ok(error.to_json_value());
                            }
                        }
                    }
                } else {
                    // No MCP client available
                    let error = ToolExecutionError::new(
                        tool_name.to_string(),
                        ToolErrorType::ToolNotFound,
                        format!("Unknown tool: {}", display_name),
                    );
                    return Ok(error.to_json_value());
                }
            }
        };

        let uses_pty = registration.uses_pty();
        if uses_pty && let Err(err) = self.start_pty_session() {
            let error = ToolExecutionError::with_original_error(
                tool_name.to_string(),
                ToolErrorType::ExecutionError,
                "Failed to start PTY session".to_string(),
                err.to_string(),
            );
            return Ok(error.to_json_value());
        }

        let handler = registration.handler();
        let result = match handler {
            ToolHandler::RegistryFn(executor) => executor(self, args).await,
            ToolHandler::TraitObject(tool) => tool.execute(args).await,
        };

        if uses_pty {
            self.end_pty_session();
        }

        match result {
            Ok(value) => Ok(normalize_tool_output(value)),
            Err(err) => {
                let error_type = classify_error(&err);
                let error = ToolExecutionError::with_original_error(
                    tool_name.to_string(),
                    error_type,
                    format!("Tool execution failed: {}", err),
                    err.to_string(),
                );
                Ok(error.to_json_value())
            }
        }
    }

    /// Set the MCP client for this registry
    pub fn with_mcp_client(mut self, mcp_client: Arc<McpClient>) -> Self {
        self.mcp_client = Some(mcp_client);
        self
    }

    /// Attach an MCP client without consuming the registry
    pub fn set_mcp_client(&mut self, mcp_client: Arc<McpClient>) {
        self.mcp_client = Some(mcp_client);
        self.mcp_tool_index.clear();
    }

    /// Get the MCP client if available
    pub fn mcp_client(&self) -> Option<&Arc<McpClient>> {
        self.mcp_client.as_ref()
    }

    /// List all MCP tools
    pub async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        if let Some(mcp_client) = &self.mcp_client {
            mcp_client.list_mcp_tools().await
        } else {
            Ok(Vec::new())
        }
    }

    /// Check if an MCP tool exists
    pub async fn has_mcp_tool(&self, tool_name: &str) -> bool {
        if let Some(mcp_client) = &self.mcp_client {
            match mcp_client.has_mcp_tool(tool_name).await {
                Ok(true) => true,
                Ok(false) => false,
                Err(_) => {
                    // Log error but return false to continue operation
                    false
                }
            }
        } else {
            false
        }
    }

    /// Execute an MCP tool
    pub async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        if let Some(mcp_client) = &self.mcp_client {
            mcp_client.execute_mcp_tool(tool_name, args).await
        } else {
            Err(anyhow::anyhow!("MCP client not available"))
        }
    }

    async fn resolve_mcp_tool_alias(&self, tool_name: &str) -> Option<String> {
        let Some(mcp_client) = &self.mcp_client else {
            return None;
        };

        let normalized = normalize_mcp_tool_identifier(tool_name);
        if normalized.is_empty() {
            return None;
        }

        let tools = match mcp_client.list_mcp_tools().await {
            Ok(list) => list,
            Err(err) => {
                warn!(
                    "Failed to list MCP tools while resolving alias '{}': {}",
                    tool_name, err
                );
                return None;
            }
        };

        for tool in tools {
            if normalize_mcp_tool_identifier(&tool.name) == normalized {
                return Some(tool.name);
            }
        }

        None
    }

    /// Refresh MCP tools (reconnect to providers and update tool lists)
    pub async fn refresh_mcp_tools(&mut self) -> Result<()> {
        if let Some(mcp_client) = &self.mcp_client {
            debug!(
                "Refreshing MCP tools for {} providers",
                mcp_client.get_status().provider_count
            );

            let tools = mcp_client.list_mcp_tools().await?;
            let mut provider_map: HashMap<String, Vec<String>> = HashMap::new();

            for tool in tools {
                provider_map
                    .entry(tool.provider.clone())
                    .or_default()
                    .push(tool.name.clone());
            }

            for tools in provider_map.values_mut() {
                tools.sort();
                tools.dedup();
            }

            self.mcp_tool_index = provider_map;

            if let Some(allowlist) = self.policy_gateway.update_mcp_tools(&self.mcp_tool_index)? {
                mcp_client.update_allowlist(allowlist);
            }

            self.sync_policy_catalog();
            Ok(())
        } else {
            debug!("No MCP client configured, nothing to refresh");
            Ok(())
        }
    }
}

impl ToolRegistry {
    /// Prompt for permission before starting long-running tool executions to avoid spinner conflicts
    pub fn preflight_tool_permission(&mut self, name: &str) -> Result<bool> {
        match self.evaluate_tool_policy(name)? {
            ToolPermissionDecision::Allow => Ok(true),
            ToolPermissionDecision::Deny => Ok(false),
            ToolPermissionDecision::Prompt => Ok(true),
        }
    }

    pub fn evaluate_tool_policy(&mut self, name: &str) -> Result<ToolPermissionDecision> {
        if let Some(tool_name) = name.strip_prefix("mcp_") {
            return self.evaluate_mcp_tool_policy(name, tool_name);
        }

        self.policy_gateway.evaluate_tool_policy(name)
    }

    fn evaluate_mcp_tool_policy(
        &mut self,
        full_name: &str,
        tool_name: &str,
    ) -> Result<ToolPermissionDecision> {
        let provider = match self.find_mcp_provider(tool_name) {
            Some(provider) => provider,
            None => {
                // Unknown provider for this tool; default to prompt for safety
                return Ok(ToolPermissionDecision::Prompt);
            }
        };

        if self.policy_gateway.has_full_auto_allowlist()
            && !self.policy_gateway.is_allowed_in_full_auto(full_name)
        {
            return Ok(ToolPermissionDecision::Deny);
        }

        if let Ok(policy_manager) = self.policy_manager_mut() {
            match policy_manager.get_mcp_tool_policy(&provider, tool_name) {
                ToolPolicy::Allow => {
                    self.policy_gateway.preapprove(full_name);
                    Ok(ToolPermissionDecision::Allow)
                }
                ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
                ToolPolicy::Prompt => {
                    if self.policy_gateway.has_full_auto_allowlist() {
                        self.policy_gateway.preapprove(full_name);
                        Ok(ToolPermissionDecision::Allow)
                    } else {
                        Ok(ToolPermissionDecision::Prompt)
                    }
                }
            }
        } else {
            self.policy_gateway.preapprove(full_name);
            Ok(ToolPermissionDecision::Allow)
        }
    }

    pub fn mark_tool_preapproved(&mut self, name: &str) {
        self.policy_gateway.preapprove(name);
    }

    pub fn persist_mcp_tool_policy(&mut self, name: &str, policy: ToolPolicy) -> Result<()> {
        if !name.starts_with("mcp_") {
            return Ok(());
        }

        let Some(tool_name) = name.strip_prefix("mcp_") else {
            return Ok(());
        };

        let Some(provider) = self.find_mcp_provider(tool_name) else {
            return Ok(());
        };

        self.policy_gateway
            .persist_mcp_tool_policy(&provider, tool_name, policy)
    }
}

fn normalize_mcp_tool_identifier(value: &str) -> String {
    let mut normalized = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::TempDir;

    const CUSTOM_TOOL_NAME: &str = "custom_test_tool";

    struct CustomEchoTool;

    #[async_trait]
    impl Tool for CustomEchoTool {
        async fn execute(&self, args: Value) -> Result<Value> {
            Ok(json!({
                "success": true,
                "args": args,
            }))
        }

        fn name(&self) -> &'static str {
            CUSTOM_TOOL_NAME
        }

        fn description(&self) -> &'static str {
            "Custom echo tool for testing"
        }
    }

    #[tokio::test]
    async fn registers_builtin_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf());
        let available = registry.available_tools();

        assert!(available.contains(&tools::READ_FILE.to_string()));
        assert!(available.contains(&tools::RUN_TERMINAL_CMD.to_string()));
        assert!(available.contains(&tools::CURL.to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn allows_registering_custom_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf());

        registry.register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))?;

        registry.sync_policy_available_tools();

        registry.allow_all_tools().ok();

        let available = registry.available_tools();
        assert!(available.contains(&CUSTOM_TOOL_NAME.to_string()));

        let response = registry
            .execute_tool(CUSTOM_TOOL_NAME, json!({"input": "value"}))
            .await?;
        assert!(response["success"].as_bool().unwrap_or(false));
        Ok(())
    }

    #[tokio::test]
    async fn full_auto_allowlist_enforced() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf());

        registry.enable_full_auto_mode(&vec![tools::READ_FILE.to_string()]);

        assert!(registry.preflight_tool_permission(tools::READ_FILE)?);
        assert!(!registry.preflight_tool_permission(tools::RUN_TERMINAL_CMD)?);

        Ok(())
    }

    #[test]
    fn normalizes_mcp_tool_identifiers() {
        assert_eq!(
            normalize_mcp_tool_identifier("sequential-thinking"),
            "sequentialthinking"
        );
        assert_eq!(
            normalize_mcp_tool_identifier("Context7.Lookup"),
            "context7lookup"
        );
        assert_eq!(normalize_mcp_tool_identifier("alpha_beta"), "alphabeta");
    }
}
