//! Tool registry and function declarations

mod astgrep;
mod builtins;
mod cache;
mod declarations;
mod error;
mod executors;
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
use utils::normalize_tool_output;

use crate::config::PtyConfig;
use crate::config::ToolsConfig;
use crate::config::constants::tools;
use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::tools::ast_grep::AstGrepEngine;
use crate::tools::grep_search::GrepSearchManager;
use anyhow::{Result, anyhow};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tracing::{debug, warn};

use super::bash_tool::BashTool;
use super::command::CommandTool;
use super::curl_tool::CurlTool;
use super::file_ops::FileOpsTool;
use super::plan::PlanManager;
use super::pty::PtyManager;
use super::search::SearchTool;
use super::simple_search::SimpleSearchTool;
use super::srgn::SrgnTool;
use crate::mcp_client::{McpClient, McpToolExecutor, McpToolInfo};

#[cfg(test)]
use super::traits::Tool;
#[cfg(test)]
use crate::config::types::CapabilityLevel;

#[derive(Clone)]
pub struct ToolRegistry {
    workspace_root: PathBuf,
    search_tool: SearchTool,
    simple_search_tool: SimpleSearchTool,
    bash_tool: BashTool,
    file_ops_tool: FileOpsTool,
    command_tool: CommandTool,
    curl_tool: CurlTool,
    grep_search: Arc<GrepSearchManager>,
    ast_grep_engine: Option<Arc<AstGrepEngine>>,
    tool_policy: Option<ToolPolicyManager>,
    pty_manager: PtyManager,
    pty_config: PtyConfig,
    active_pty_sessions: Arc<AtomicUsize>,
    srgn_tool: SrgnTool,
    plan_manager: PlanManager,
    mcp_client: Option<Arc<McpClient>>,
    mcp_tool_index: HashMap<String, Vec<String>>,
    tool_registrations: Vec<ToolRegistration>,
    tool_lookup: HashMap<&'static str, usize>,
    preapproved_tools: HashSet<String>,
    full_auto_allowlist: Option<HashSet<String>>,
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

    fn build(workspace_root: PathBuf, pty_config: PtyConfig, todo_planning_enabled: bool) -> Self {
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));

        let search_tool = SearchTool::new(workspace_root.clone(), grep_search.clone());
        let simple_search_tool = SimpleSearchTool::new(workspace_root.clone());
        let bash_tool = BashTool::new(workspace_root.clone());
        let file_ops_tool = FileOpsTool::new(workspace_root.clone(), grep_search.clone());
        let command_tool = CommandTool::new(workspace_root.clone());
        let curl_tool = CurlTool::new();
        let srgn_tool = SrgnTool::new(workspace_root.clone());
        let plan_manager = PlanManager::new();
        let pty_manager = PtyManager::new(workspace_root.clone(), pty_config.clone());

        let ast_grep_engine = match AstGrepEngine::new() {
            Ok(engine) => Some(Arc::new(engine)),
            Err(err) => {
                eprintln!("Warning: Failed to initialize AST-grep engine: {}", err);
                None
            }
        };

        let policy_manager = match ToolPolicyManager::new_with_workspace(&workspace_root) {
            Ok(manager) => Some(manager),
            Err(err) => {
                eprintln!("Warning: Failed to initialize tool policy manager: {}", err);
                None
            }
        };

        let mut registry = Self {
            workspace_root,
            search_tool,
            simple_search_tool,
            bash_tool,
            file_ops_tool,
            command_tool,
            curl_tool,
            grep_search,
            ast_grep_engine,
            tool_policy: policy_manager,
            pty_manager,
            pty_config,
            active_pty_sessions: Arc::new(AtomicUsize::new(0)),
            srgn_tool,
            plan_manager,
            mcp_client: None,
            mcp_tool_index: HashMap::new(),
            tool_registrations: Vec::new(),
            tool_lookup: HashMap::new(),
            preapproved_tools: HashSet::new(),
            full_auto_allowlist: None,
        };

        register_builtin_tools(&mut registry, todo_planning_enabled);
        registry
    }

    pub fn register_tool(&mut self, registration: ToolRegistration) -> Result<()> {
        if self.tool_lookup.contains_key(registration.name()) {
            return Err(anyhow!(format!(
                "Tool '{}' is already registered",
                registration.name()
            )));
        }

        let index = self.tool_registrations.len();
        self.tool_lookup.insert(registration.name(), index);
        self.tool_registrations.push(registration);
        Ok(())
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.tool_registrations
            .iter()
            .map(|registration| registration.name().to_string())
            .collect()
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
        let mut normalized: HashSet<String> = HashSet::new();
        if allowed_tools
            .iter()
            .any(|tool| tool.trim() == tools::WILDCARD_ALL)
        {
            for tool in self.available_tools() {
                normalized.insert(tool);
            }
        } else {
            for tool in allowed_tools {
                let trimmed = tool.trim();
                if !trimmed.is_empty() {
                    normalized.insert(trimmed.to_string());
                }
            }
        }

        self.full_auto_allowlist = Some(normalized);
    }

    pub fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.full_auto_allowlist.as_ref().map(|set| {
            let mut items: Vec<String> = set.iter().cloned().collect();
            items.sort();
            items
        })
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tool_lookup.contains_key(name)
    }

    pub fn with_ast_grep(mut self, engine: Arc<AstGrepEngine>) -> Self {
        self.ast_grep_engine = Some(engine);
        self
    }

    pub fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    pub fn pty_manager(&self) -> &PtyManager {
        &self.pty_manager
    }

    pub fn plan_manager(&self) -> PlanManager {
        self.plan_manager.clone()
    }

    pub fn current_plan(&self) -> crate::tools::TaskPlan {
        self.plan_manager.snapshot()
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
        if let Some(allowlist) = &self.full_auto_allowlist
            && !allowlist.contains(name)
        {
            let error = ToolExecutionError::new(
                name.to_string(),
                ToolErrorType::PolicyViolation,
                format!(
                    "Tool '{}' is not permitted while full-auto mode is active",
                    name
                ),
            );
            return Ok(error.to_json_value());
        }

        let skip_policy_prompt = self.preapproved_tools.remove(name);

        if !skip_policy_prompt
            && let Ok(policy_manager) = self.policy_manager_mut()
            && !policy_manager.should_execute_tool(name)?
        {
            let error = ToolExecutionError::new(
                name.to_string(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{}' execution denied by policy", name),
            );
            return Ok(error.to_json_value());
        }

        let args = match self.apply_policy_constraints(name, args) {
            Ok(args) => args,
            Err(err) => {
                let error = ToolExecutionError::with_original_error(
                    name.to_string(),
                    ToolErrorType::InvalidParameters,
                    "Failed to apply policy constraints".to_string(),
                    err.to_string(),
                );
                return Ok(error.to_json_value());
            }
        };

        let registration = match self
            .tool_lookup
            .get(name)
            .and_then(|index| self.tool_registrations.get(*index))
        {
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
                                    name.to_string(),
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
                                    name.to_string(),
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
                        match mcp_client.has_mcp_tool(name).await {
                            Ok(true) => {
                                debug!(
                                    "Tool '{}' not found in registry, delegating to MCP client",
                                    name
                                );
                                return self.execute_mcp_tool(name, args).await;
                            }
                            Ok(false) => {
                                // MCP client doesn't have this tool either
                                let error = ToolExecutionError::new(
                                    name.to_string(),
                                    ToolErrorType::ToolNotFound,
                                    format!("Unknown tool: {}", name),
                                );
                                return Ok(error.to_json_value());
                            }
                            Err(e) => {
                                warn!("Error checking MCP tool availability for '{}': {}", name, e);
                                let error = ToolExecutionError::with_original_error(
                                    name.to_string(),
                                    ToolErrorType::ExecutionError,
                                    format!(
                                        "Failed to verify MCP tool '{}' due to provider errors",
                                        name
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
                        name.to_string(),
                        ToolErrorType::ToolNotFound,
                        format!("Unknown tool: {}", name),
                    );
                    return Ok(error.to_json_value());
                }
            }
        };

        let uses_pty = registration.uses_pty();
        if uses_pty && let Err(err) = self.start_pty_session() {
            let error = ToolExecutionError::with_original_error(
                name.to_string(),
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
                    name.to_string(),
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

            if let Some(policy_manager) = self.tool_policy.as_mut() {
                policy_manager.update_mcp_tools(&self.mcp_tool_index)?;
                let allowlist = policy_manager.mcp_allowlist().clone();
                mcp_client.update_allowlist(allowlist);
            }

            self.sync_policy_available_tools();
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

        if let Some(allowlist) = self.full_auto_allowlist.as_ref() {
            if !allowlist.contains(name) {
                return Ok(ToolPermissionDecision::Deny);
            }

            if let Some(policy_manager) = self.tool_policy.as_mut() {
                match policy_manager.get_policy(name) {
                    ToolPolicy::Deny => return Ok(ToolPermissionDecision::Deny),
                    ToolPolicy::Allow | ToolPolicy::Prompt => {
                        self.preapproved_tools.insert(name.to_string());
                        return Ok(ToolPermissionDecision::Allow);
                    }
                }
            }

            self.preapproved_tools.insert(name.to_string());
            return Ok(ToolPermissionDecision::Allow);
        }

        if let Some(policy_manager) = self.tool_policy.as_mut() {
            match policy_manager.get_policy(name) {
                ToolPolicy::Allow => {
                    self.preapproved_tools.insert(name.to_string());
                    Ok(ToolPermissionDecision::Allow)
                }
                ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
                ToolPolicy::Prompt => {
                    if ToolPolicyManager::is_auto_allow_tool(name) {
                        policy_manager.set_policy(name, ToolPolicy::Allow)?;
                        self.preapproved_tools.insert(name.to_string());
                        Ok(ToolPermissionDecision::Allow)
                    } else {
                        Ok(ToolPermissionDecision::Prompt)
                    }
                }
            }
        } else {
            self.preapproved_tools.insert(name.to_string());
            Ok(ToolPermissionDecision::Allow)
        }
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

        if let Some(allowlist) = self.full_auto_allowlist.as_ref() {
            if !allowlist.contains(full_name) {
                return Ok(ToolPermissionDecision::Deny);
            }

            if let Some(policy_manager) = self.tool_policy.as_mut() {
                match policy_manager.get_mcp_tool_policy(&provider, tool_name) {
                    ToolPolicy::Deny => return Ok(ToolPermissionDecision::Deny),
                    ToolPolicy::Allow | ToolPolicy::Prompt => {
                        self.preapproved_tools.insert(full_name.to_string());
                        return Ok(ToolPermissionDecision::Allow);
                    }
                }
            }

            self.preapproved_tools.insert(full_name.to_string());
            return Ok(ToolPermissionDecision::Allow);
        }

        if let Some(policy_manager) = self.tool_policy.as_mut() {
            match policy_manager.get_mcp_tool_policy(&provider, tool_name) {
                ToolPolicy::Allow => {
                    self.preapproved_tools.insert(full_name.to_string());
                    Ok(ToolPermissionDecision::Allow)
                }
                ToolPolicy::Deny => Ok(ToolPermissionDecision::Deny),
                ToolPolicy::Prompt => Ok(ToolPermissionDecision::Prompt),
            }
        } else {
            self.preapproved_tools.insert(full_name.to_string());
            Ok(ToolPermissionDecision::Allow)
        }
    }

    pub fn mark_tool_preapproved(&mut self, name: &str) {
        self.preapproved_tools.insert(name.to_string());
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

        if let Some(manager) = self.tool_policy.as_mut() {
            manager.set_mcp_tool_policy(&provider, tool_name, policy)?;
        }

        Ok(())
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
