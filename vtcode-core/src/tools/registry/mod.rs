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
mod telemetry;
mod utils;

pub use declarations::{
    build_function_declarations, build_function_declarations_for_level,
    build_function_declarations_with_mode,
};
pub use error::{ToolErrorType, ToolExecutionError, classify_error};
pub use registration::{ToolExecutorFn, ToolHandler, ToolRegistration};
pub use telemetry::ToolTelemetryEvent;

use builtins::register_builtin_tools;
use inventory::ToolInventory;
use policy::ToolPolicyGateway;
use pty::PtySessionManager;
use utils::normalize_tool_output;

#[cfg(test)]
use crate::config::constants::tools;
use crate::config::{CommandsConfig, PtyConfig, TimeoutsConfig, ToolsConfig};
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
use std::time::Duration;
use tracing::{debug, warn};

use super::plan::PlanManager;
use crate::mcp_client::{McpClient, McpToolExecutor, McpToolInfo};

#[cfg(test)]
use super::traits::Tool;
#[cfg(test)]
use crate::config::types::CapabilityLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTimeoutCategory {
    Default,
    Pty,
    Mcp,
}

impl ToolTimeoutCategory {
    pub fn label(&self) -> &'static str {
        match self {
            ToolTimeoutCategory::Default => "standard",
            ToolTimeoutCategory::Pty => "PTY",
            ToolTimeoutCategory::Mcp => "MCP",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolTimeoutPolicy {
    default_ceiling: Option<Duration>,
    pty_ceiling: Option<Duration>,
    mcp_ceiling: Option<Duration>,
    warning_fraction: f32,
}

impl Default for ToolTimeoutPolicy {
    fn default() -> Self {
        Self {
            default_ceiling: Some(Duration::from_secs(180)),
            pty_ceiling: Some(Duration::from_secs(300)),
            mcp_ceiling: Some(Duration::from_secs(120)),
            warning_fraction: 0.8,
        }
    }
}

impl ToolTimeoutPolicy {
    pub fn from_config(config: &TimeoutsConfig) -> Self {
        Self {
            default_ceiling: config.ceiling_duration(config.default_ceiling_seconds),
            pty_ceiling: config.ceiling_duration(config.pty_ceiling_seconds),
            mcp_ceiling: config.ceiling_duration(config.mcp_ceiling_seconds),
            warning_fraction: config.warning_threshold_fraction().clamp(0.0, 0.99),
        }
    }

    /// Validate the timeout policy configuration
    ///
    /// Ensures that:
    /// - Ceiling values are within reasonable bounds (1s - 3600s)
    /// - Warning fraction is between 0.0 and 1.0
    /// - No ceiling is configured as 0 seconds
    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate default ceiling
        if let Some(ceiling) = self.default_ceiling {
            if ceiling < Duration::from_secs(1) {
                anyhow::bail!(
                    "default_ceiling_seconds must be at least 1 second (got {}s)",
                    ceiling.as_secs()
                );
            }
            if ceiling > Duration::from_secs(3600) {
                anyhow::bail!(
                    "default_ceiling_seconds must not exceed 3600 seconds/1 hour (got {}s)",
                    ceiling.as_secs()
                );
            }
        }

        // Validate PTY ceiling
        if let Some(ceiling) = self.pty_ceiling {
            if ceiling < Duration::from_secs(1) {
                anyhow::bail!(
                    "pty_ceiling_seconds must be at least 1 second (got {}s)",
                    ceiling.as_secs()
                );
            }
            if ceiling > Duration::from_secs(3600) {
                anyhow::bail!(
                    "pty_ceiling_seconds must not exceed 3600 seconds/1 hour (got {}s)",
                    ceiling.as_secs()
                );
            }
        }

        // Validate MCP ceiling
        if let Some(ceiling) = self.mcp_ceiling {
            if ceiling < Duration::from_secs(1) {
                anyhow::bail!(
                    "mcp_ceiling_seconds must be at least 1 second (got {}s)",
                    ceiling.as_secs()
                );
            }
            if ceiling > Duration::from_secs(3600) {
                anyhow::bail!(
                    "mcp_ceiling_seconds must not exceed 3600 seconds/1 hour (got {}s)",
                    ceiling.as_secs()
                );
            }
        }

        // Validate warning fraction
        if self.warning_fraction <= 0.0 {
            anyhow::bail!(
                "warning_threshold_percent must be greater than 0 (got {})",
                self.warning_fraction * 100.0
            );
        }
        if self.warning_fraction >= 1.0 {
            anyhow::bail!(
                "warning_threshold_percent must be less than 100 (got {})",
                self.warning_fraction * 100.0
            );
        }

        Ok(())
    }

    pub fn ceiling_for(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        match category {
            ToolTimeoutCategory::Default => self.default_ceiling,
            ToolTimeoutCategory::Pty => self.pty_ceiling.or(self.default_ceiling),
            ToolTimeoutCategory::Mcp => self.mcp_ceiling.or(self.default_ceiling),
        }
    }

    pub fn warning_fraction(&self) -> f32 {
        self.warning_fraction
    }
}

#[derive(Clone)]
pub struct ToolRegistry {
    inventory: ToolInventory,
    policy_gateway: ToolPolicyGateway,
    pty_sessions: PtySessionManager,
    mcp_client: Option<Arc<McpClient>>,
    mcp_tool_index: HashMap<String, Vec<String>>,
    mcp_tool_presence: HashMap<String, bool>,
    timeout_policy: ToolTimeoutPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionDecision {
    Allow,
    Deny,
    Prompt,
}

impl ToolRegistry {
    pub async fn new(workspace_root: PathBuf) -> Self {
        Self::build(workspace_root, PtyConfig::default(), true).await
    }

    pub async fn new_with_config(workspace_root: PathBuf, pty_config: PtyConfig) -> Self {
        Self::build(workspace_root, pty_config, true).await
    }

    pub async fn new_with_features(workspace_root: PathBuf, todo_planning_enabled: bool) -> Self {
        Self::build(workspace_root, PtyConfig::default(), todo_planning_enabled).await
    }

    pub async fn new_with_config_and_features(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
    ) -> Self {
        Self::build(workspace_root, pty_config, todo_planning_enabled).await
    }

    pub async fn new_with_custom_policy(
        workspace_root: PathBuf,
        policy_manager: ToolPolicyManager,
    ) -> Self {
        Self::build_with_policy(
            workspace_root,
            PtyConfig::default(),
            true,
            Some(policy_manager),
        )
        .await
    }

    pub async fn new_with_custom_policy_and_config(
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
        .await
    }

    async fn build(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
    ) -> Self {
        Self::build_with_policy(workspace_root, pty_config, todo_planning_enabled, None).await
    }

    async fn build_with_policy(
        workspace_root: PathBuf,
        pty_config: PtyConfig,
        todo_planning_enabled: bool,
        policy_manager: Option<ToolPolicyManager>,
    ) -> Self {
        let mut inventory = ToolInventory::new(workspace_root.clone());
        register_builtin_tools(&mut inventory, todo_planning_enabled);

        let pty_sessions = PtySessionManager::new(workspace_root.clone(), pty_config);

        let policy_gateway = match policy_manager {
            Some(pm) => ToolPolicyGateway::with_policy_manager(pm),
            None => ToolPolicyGateway::new(&workspace_root).await,
        };

        let mut registry = Self {
            inventory,
            policy_gateway,
            pty_sessions,
            mcp_client: None,
            mcp_tool_index: HashMap::new(),
            mcp_tool_presence: HashMap::new(),
            timeout_policy: ToolTimeoutPolicy::default(),
        };

        registry.sync_policy_catalog().await;
        registry
    }

    async fn sync_policy_catalog(&mut self) {
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
            .sync_available_tools(available, &mcp_keys)
            .await;
    }

    /// Register a new tool with the registry
    ///
    /// # Arguments
    /// * `registration` - The tool registration to add
    ///
    /// # Returns
    /// `Result<()>` indicating success or an error if the tool is already registered
    pub fn register_tool(&mut self, registration: ToolRegistration) -> Result<()> {
        // Clone the name since we need it after moving registration
        let tool_name = registration.name().to_string();

        // Register the tool
        self.inventory.register_tool(registration)?;

        // Register any aliases for the tool
        for alias in tool_aliases(&tool_name) {
            self.inventory.add_alias(alias, &tool_name);
        }

        Ok(())
    }

    /// Get a list of all available tools, including MCP tools
    ///
    /// # Returns
    /// A `Vec<String>` containing the names of all available tools
    pub async fn available_tools(&self) -> Vec<String> {
        let mut tools = self.inventory.available_tools();

        // Add MCP tools if available
        if let Some(mcp_client) = &self.mcp_client {
            if let Ok(mcp_tools) = mcp_client.list_mcp_tools().await {
                for tool in mcp_tools {
                    tools.push(format!("mcp_{}", tool.name));
                }
            }
        }

        tools.sort();
        tools
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

    pub async fn enable_full_auto_mode(&mut self, allowed_tools: &[String]) {
        let available = self.available_tools().await;
        self.policy_gateway
            .enable_full_auto_mode(allowed_tools, &available);
    }

    pub fn disable_full_auto_mode(&mut self) {
        self.policy_gateway.disable_full_auto_mode();
    }

    pub fn current_full_auto_allowlist(&self) -> Option<Vec<String>> {
        self.policy_gateway.current_full_auto_allowlist()
    }

    /// Check if a tool with the given name is registered
    ///
    /// # Arguments
    /// * `name` - The name of the tool to check
    ///
    /// # Returns
    /// `bool` indicating whether the tool exists (including aliases)
    pub async fn has_tool(&self, name: &str) -> bool {
        // First check the main tool registry
        if self.inventory.has_tool(name) {
            return true;
        }

        // If not found, check if it's an MCP tool
        if name.starts_with("mcp_") {
            let tool_name = &name[4..];
            if self.find_mcp_provider(tool_name).is_some() {
                return true;
            }

            if let Some(mcp_client) = &self.mcp_client {
                if let Ok(true) = mcp_client.has_mcp_tool(tool_name).await {
                    return true;
                }
                // Check if it's an alias
                if let Some(resolved_name) = self.resolve_mcp_tool_alias(tool_name).await {
                    if resolved_name != tool_name {
                        return true;
                    }
                }
            }
        }

        false
    }

    pub async fn with_ast_grep(mut self, engine: Arc<AstGrepEngine>) -> Self {
        self.inventory.set_ast_grep_engine(engine);
        self.sync_policy_catalog().await;
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

    pub async fn set_policy_manager(&mut self, manager: ToolPolicyManager) {
        self.policy_gateway.set_policy_manager(manager);
        self.sync_policy_catalog().await;
    }

    pub async fn set_tool_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        self.policy_gateway.set_tool_policy(tool_name, policy).await
    }

    pub fn get_tool_policy(&self, tool_name: &str) -> ToolPolicy {
        self.policy_gateway.get_tool_policy(tool_name)
    }

    pub async fn reset_tool_policies(&mut self) -> Result<()> {
        self.policy_gateway.reset_tool_policies().await
    }

    pub async fn allow_all_tools(&mut self) -> Result<()> {
        self.policy_gateway.allow_all_tools().await
    }

    pub async fn deny_all_tools(&mut self) -> Result<()> {
        self.policy_gateway.deny_all_tools().await
    }

    pub fn print_policy_status(&self) {
        self.policy_gateway.print_policy_status();
    }

    pub async fn initialize_async(&mut self) -> Result<()> {
        Ok(())
    }

    pub async fn apply_config_policies(&mut self, tools_config: &ToolsConfig) -> Result<()> {
        if let Ok(policy_manager) = self.policy_manager_mut() {
            policy_manager.apply_tools_config(tools_config).await?;
        }

        Ok(())
    }

    pub fn apply_commands_config(&mut self, commands_config: &CommandsConfig) {
        self.inventory
            .command_tool_mut()
            .update_commands_config(commands_config.clone());
    }

    pub fn apply_timeout_policy(&mut self, timeouts: &TimeoutsConfig) {
        let policy = ToolTimeoutPolicy::from_config(timeouts);

        // Validate the policy before applying
        if let Err(e) = policy.validate() {
            warn!(
                error = %e,
                "Invalid timeout configuration detected, using defaults"
            );
            self.timeout_policy = ToolTimeoutPolicy::default();
        } else {
            self.timeout_policy = policy;
        }
    }

    pub fn timeout_policy(&self) -> &ToolTimeoutPolicy {
        &self.timeout_policy
    }

    pub async fn timeout_category_for(&mut self, name: &str) -> ToolTimeoutCategory {
        let canonical_name = canonical_tool_name(name);
        let tool_name = canonical_name.as_ref();

        if let Some(registration) = self.inventory.registration_for(tool_name) {
            return if registration.uses_pty() {
                ToolTimeoutCategory::Pty
            } else {
                ToolTimeoutCategory::Default
            };
        }

        if let Some(stripped) = name.strip_prefix("mcp_") {
            if self.has_mcp_tool(stripped).await {
                return ToolTimeoutCategory::Mcp;
            }
        } else if self.find_mcp_provider(tool_name).is_some() || self.has_mcp_tool(tool_name).await
        {
            return ToolTimeoutCategory::Mcp;
        }

        ToolTimeoutCategory::Default
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

        if !skip_policy_prompt && !self.policy_gateway.should_execute_tool(tool_name).await? {
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

        // First, check if we need a PTY session by checking if the tool exists and needs PTY
        let mut needs_pty = false;
        let mut tool_exists = false;
        let mut is_mcp_tool = false;
        let mut mcp_lookup_error: Option<anyhow::Error> = None;

        // Check if it's a standard tool first
        if let Some(registration) = self.inventory.registration_for(tool_name) {
            needs_pty = registration.uses_pty();
            tool_exists = true;
        }
        // If not a standard tool, check if it's an MCP tool
        else if let Some(mcp_client) = &self.mcp_client {
            let resolved_name = if let Some(stripped) = name.strip_prefix("mcp_") {
                stripped.to_string()
            } else {
                tool_name.to_string()
            };

            match mcp_client.has_mcp_tool(&resolved_name).await {
                Ok(true) => {
                    needs_pty = true;
                    tool_exists = true;
                    is_mcp_tool = true;
                }
                Ok(false) => {
                    tool_exists = false;
                }
                Err(err) => {
                    warn!("Error checking MCP tool '{}': {}", resolved_name, err);
                    mcp_lookup_error = Some(err);
                }
            }
        }

        // If tool doesn't exist in either registry, return an error
        if !tool_exists {
            if let Some(err) = mcp_lookup_error {
                let error = ToolExecutionError::with_original_error(
                    tool_name.to_string(),
                    ToolErrorType::ExecutionError,
                    format!("Failed to resolve MCP tool '{}': {}", display_name, err),
                    err.to_string(),
                );
                return Ok(error.to_json_value());
            }

            let error = ToolExecutionError::new(
                tool_name.to_string(),
                ToolErrorType::ToolNotFound,
                format!("Unknown tool: {}", display_name),
            );
            return Ok(error.to_json_value());
        }

        // Start PTY session if needed
        if needs_pty {
            if let Err(err) = self.start_pty_session() {
                let error = ToolExecutionError::with_original_error(
                    tool_name.to_string(),
                    ToolErrorType::ExecutionError,
                    "Failed to start PTY session".to_string(),
                    err.to_string(),
                );
                return Ok(error.to_json_value());
            }
        }

        // Execute the appropriate tool based on its type
        let result = if is_mcp_tool {
            self.execute_mcp_tool(tool_name, args).await
        } else if let Some(registration) = self.inventory.registration_for(tool_name) {
            // Log deprecation warning if tool is deprecated
            if registration.is_deprecated() {
                if let Some(msg) = registration.deprecation_message() {
                    warn!("Tool '{}' is deprecated: {}", tool_name, msg);
                } else {
                    warn!(
                        "Tool '{}' is deprecated and may be removed in a future version",
                        tool_name
                    );
                }
            }

            let handler = registration.handler();
            match handler {
                ToolHandler::RegistryFn(executor) => executor(self, args).await,
                ToolHandler::TraitObject(tool) => tool.execute(args).await,
            }
        } else {
            // This should theoretically never happen since we checked tool_exists above
            return Ok(ToolExecutionError::new(
                tool_name.to_string(),
                ToolErrorType::ToolNotFound,
                "Tool not found in registry".to_string(),
            )
            .to_json_value());
        };

        // Clean up PTY session if we started one
        if needs_pty {
            self.end_pty_session();
        }

        // Handle the execution result
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
        self.mcp_tool_index.clear();
        self.mcp_tool_presence.clear();
        self
    }

    /// Attach an MCP client without consuming the registry
    pub fn set_mcp_client(&mut self, mcp_client: Arc<McpClient>) {
        self.mcp_client = Some(mcp_client);
        self.mcp_tool_index.clear();
        self.mcp_tool_presence.clear();
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
    pub async fn has_mcp_tool(&mut self, tool_name: &str) -> bool {
        if self
            .mcp_tool_index
            .values()
            .any(|tools| tools.iter().any(|candidate| candidate == tool_name))
        {
            self.mcp_tool_presence.insert(tool_name.to_string(), true);
            return true;
        }

        if let Some(cached) = self.mcp_tool_presence.get(tool_name) {
            return *cached;
        }

        let Some(mcp_client) = &self.mcp_client else {
            self.mcp_tool_presence.insert(tool_name.to_string(), false);
            return false;
        };

        match mcp_client.has_mcp_tool(tool_name).await {
            Ok(result) => {
                self.mcp_tool_presence.insert(tool_name.to_string(), result);
                result
            }
            Err(err) => {
                warn!(
                    tool = tool_name,
                    error = %err,
                    "failed to query MCP tool presence"
                );
                self.mcp_tool_presence.insert(tool_name.to_string(), false);
                false
            }
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
            self.mcp_tool_presence.clear();
            for tools in self.mcp_tool_index.values() {
                for tool in tools {
                    self.mcp_tool_presence.insert(tool.clone(), true);
                }
            }

            if let Some(allowlist) = self
                .policy_gateway
                .update_mcp_tools(&self.mcp_tool_index)
                .await?
            {
                mcp_client.update_allowlist(allowlist);
            }

            self.sync_policy_catalog().await;
            Ok(())
        } else {
            debug!("No MCP client configured, nothing to refresh");
            Ok(())
        }
    }
}

impl ToolRegistry {
    /// Prompt for permission before starting long-running tool executions to avoid spinner conflicts
    pub async fn preflight_tool_permission(&mut self, name: &str) -> Result<bool> {
        match self.evaluate_tool_policy(name).await? {
            ToolPermissionDecision::Allow => Ok(true),
            ToolPermissionDecision::Deny => Ok(false),
            ToolPermissionDecision::Prompt => Ok(true),
        }
    }

    pub async fn evaluate_tool_policy(&mut self, name: &str) -> Result<ToolPermissionDecision> {
        if let Some(tool_name) = name.strip_prefix("mcp_") {
            return self.evaluate_mcp_tool_policy(name, tool_name).await;
        }

        self.policy_gateway.evaluate_tool_policy(name).await
    }

    async fn evaluate_mcp_tool_policy(
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
                    // Always prompt for explicit "prompt" policy, even in full-auto mode
                    // This ensures human-in-the-loop approval for sensitive operations
                    Ok(ToolPermissionDecision::Prompt)
                }
            }
        } else {
            // Policy manager not available - default to prompt for safety
            // instead of auto-approving
            Ok(ToolPermissionDecision::Prompt)
        }
    }

    pub fn mark_tool_preapproved(&mut self, name: &str) {
        self.policy_gateway.preapprove(name);
    }

    pub async fn persist_mcp_tool_policy(&mut self, name: &str, policy: ToolPolicy) -> Result<()> {
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
            .await
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
    use std::time::Duration;
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
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let available = registry.available_tools().await;

        assert!(available.contains(&tools::READ_FILE.to_string()));
        assert!(available.contains(&tools::RUN_COMMAND.to_string()));
        assert!(available.contains(&tools::CURL.to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn allows_registering_custom_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))?;

        registry.allow_all_tools().await.ok();

        let available = registry.available_tools().await;
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
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.enable_full_auto_mode(&vec![tools::READ_FILE.to_string()]);

        assert!(registry.preflight_tool_permission(tools::READ_FILE).await?);
        assert!(
            !registry
                .preflight_tool_permission(tools::RUN_COMMAND)
                .await?
        );

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

    #[test]
    fn timeout_policy_derives_from_config() {
        let mut config = TimeoutsConfig::default();
        config.default_ceiling_seconds = 0;
        config.pty_ceiling_seconds = 600;
        config.mcp_ceiling_seconds = 90;
        config.warning_threshold_percent = 75;

        let policy = ToolTimeoutPolicy::from_config(&config);
        assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Default), None);
        assert_eq!(
            policy.ceiling_for(ToolTimeoutCategory::Pty),
            Some(Duration::from_secs(600))
        );
        assert_eq!(
            policy.ceiling_for(ToolTimeoutCategory::Mcp),
            Some(Duration::from_secs(90))
        );
        assert!((policy.warning_fraction() - 0.75).abs() < f32::EPSILON);
    }
}
