//! Tool policy management system
//!
//! This module manages user preferences for tool usage, storing choices in
//! ~/.vtcode/tool-policy.json to minimize repeated prompts while maintaining
//! user control overwhich tools the agent can use.

use crate::utils::error_messages::ERR_CREATE_POLICY_DIR;
use anyhow::{Context, Result};
use dialoguer::console::style;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::constants::tools;
use crate::config::core::tools::{ToolPolicy as ConfigToolPolicy, ToolsConfig};
use crate::config::loader::{ConfigManager, VTCodeConfig};
use crate::config::mcp::{McpAllowListConfig, McpAllowListRules};
use crate::tools::names::canonical_tool_name;
use crate::utils::file_utils::{
    ensure_dir_exists, read_file_with_context, write_file_with_context,
};

const AUTO_ALLOW_TOOLS: &[&str] = &[
    tools::GREP_FILE,
    tools::LIST_FILES,
    tools::READ_FILE,
    tools::SEARCH_TOOLS,
    tools::LIST_PTY_SESSIONS,
    tools::READ_PTY_SESSION,
    // RUN_PTY_CMD removed - should require prompt in tools_policy mode
    // since it can execute arbitrary shell commands
    "cargo_check",
    "cargo_test",
    "git_status",
    "git_diff",
    "git_log",
];

/// Tool execution policy
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    /// Allow tool execution without prompting
    Allow,
    /// Prompt user for confirmation each time
    #[default]
    Prompt,
    /// Never allow tool execution
    Deny,
}

/// Decision result for tool execution
#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecutionDecision {
    Allowed,
    Denied,
    DeniedWithFeedback(String),
}

impl ToolExecutionDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Tool policy configuration stored in ~/.vtcode/tool-policy.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicyConfig {
    /// Configuration version for future compatibility
    pub version: u32,
    /// Available tools at time of last update
    pub available_tools: Vec<String>,
    /// Policy for each tool
    pub policies: IndexMap<String, ToolPolicy>,
    /// Optional per-tool constraints to scope permissions and enforce safety
    #[serde(default)]
    pub constraints: IndexMap<String, ToolConstraints>,
    /// MCP-specific policy configuration
    #[serde(default)]
    pub mcp: McpPolicyStore,
}

impl Default for ToolPolicyConfig {
    fn default() -> Self {
        Self {
            version: 1,
            available_tools: Vec::new(),
            policies: IndexMap::new(),
            constraints: IndexMap::new(),
            mcp: McpPolicyStore::default(),
        }
    }
}

/// Stored MCP policy state, persisted alongside standard tool policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPolicyStore {
    /// Active MCP allow list configuration
    #[serde(default = "default_secure_mcp_allowlist")]
    pub allowlist: McpAllowListConfig,
    /// Provider-specific tool policies (allow/prompt/deny)
    #[serde(default)]
    pub providers: IndexMap<String, McpProviderPolicy>,
}

impl Default for McpPolicyStore {
    fn default() -> Self {
        Self {
            allowlist: default_secure_mcp_allowlist(),
            providers: IndexMap::new(),
        }
    }
}

/// MCP provider policy entry containing per-tool permissions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpProviderPolicy {
    #[serde(default)]
    pub tools: IndexMap<String, ToolPolicy>,
}

// Helper constants to reduce allocations in MCP allowlist configuration
const MCP_LOGGING_EVENTS: &[&str] = &[
    "mcp.tool_execution",
    "mcp.tool_failed",
    "mcp.tool_denied",
    "mcp.tool_filtered",
    "mcp.provider_initialized",
];

const MCP_DEFAULT_LOGGING_EVENTS: &[&str] = &[
    "mcp.provider_initialized",
    "mcp.provider_initialization_failed",
    "mcp.tool_filtered",
    "mcp.tool_execution",
    "mcp.tool_failed",
    "mcp.tool_denied",
];

/// Helper to create standard MCP logging configuration
#[inline]
fn mcp_standard_logging() -> Vec<String> {
    MCP_LOGGING_EVENTS.iter().map(|s| (*s).into()).collect()
}

/// Helper to create provider configuration with max_concurrent_requests
#[inline]
fn mcp_provider_config_with(extra: (&str, Vec<&str>)) -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([
        ("provider".into(), vec!["max_concurrent_requests".into()]),
        (
            extra.0.into(),
            extra.1.into_iter().map(Into::into).collect(),
        ),
    ])
}

fn default_secure_mcp_allowlist() -> McpAllowListConfig {
    let default_logging = Some(
        MCP_DEFAULT_LOGGING_EVENTS
            .iter()
            .map(|s| (*s).into())
            .collect(),
    );

    let default_configuration = Some(BTreeMap::from([
        (
            "client".into(),
            vec![
                "max_concurrent_connections".into(),
                "request_timeout_seconds".into(),
                "retry_attempts".into(),
                "startup_timeout_seconds".into(),
                "tool_timeout_seconds".into(),
                "experimental_use_rmcp_client".into(),
            ],
        ),
        (
            "ui".into(),
            vec![
                "mode".into(),
                "max_events".into(),
                "show_provider_names".into(),
            ],
        ),
        (
            "server".into(),
            vec![
                "enabled".into(),
                "bind_address".into(),
                "port".into(),
                "transport".into(),
                "name".into(),
                "version".into(),
            ],
        ),
    ]));

    let time_rules = McpAllowListRules {
        tools: Some(vec![
            "get_*".into(),
            "list_*".into(),
            "convert_timezone".into(),
            "describe_timezone".into(),
            "time_*".into(),
        ]),
        resources: Some(vec!["timezone:*".into(), "location:*".into()]),
        logging: Some(mcp_standard_logging()),
        configuration: Some(mcp_provider_config_with((
            "time",
            vec!["local_timezone_override"],
        ))),
        ..Default::default()
    };

    let context_rules = McpAllowListRules {
        tools: Some(vec![
            "search_*".into(),
            "fetch_*".into(),
            "list_*".into(),
            "context7_*".into(),
            "get_*".into(),
        ]),
        resources: Some(vec![
            "docs::*".into(),
            "snippets::*".into(),
            "repositories::*".into(),
            "context7::*".into(),
        ]),
        prompts: Some(vec![
            "context7::*".into(),
            "support::*".into(),
            "docs::*".into(),
        ]),
        logging: Some(mcp_standard_logging()),
        configuration: Some(mcp_provider_config_with((
            "context7",
            vec!["workspace", "search_scope", "max_results"],
        ))),
    };

    let seq_rules = McpAllowListRules {
        tools: Some(vec![
            "plan".into(),
            "critique".into(),
            "reflect".into(),
            "decompose".into(),
            "sequential_*".into(),
        ]),
        resources: None,
        prompts: Some(vec![
            "sequential-thinking::*".into(),
            "plan".into(),
            "reflect".into(),
            "critique".into(),
        ]),
        logging: Some(mcp_standard_logging()),
        configuration: Some(mcp_provider_config_with((
            "sequencing",
            vec!["max_depth", "max_branches"],
        ))),
    };

    let mut allowlist = McpAllowListConfig {
        enforce: true,
        default: McpAllowListRules {
            logging: default_logging,
            configuration: default_configuration,
            ..Default::default()
        },
        ..Default::default()
    };

    allowlist.providers.insert("time".into(), time_rules);
    allowlist.providers.insert("context7".into(), context_rules);
    allowlist
        .providers
        .insert("sequential-thinking".into(), seq_rules);

    allowlist
}

fn parse_mcp_policy_key(tool_name: &str) -> Option<(String, String)> {
    let mut parts = tool_name.splitn(3, "::");
    match (parts.next()?, parts.next(), parts.next()) {
        ("mcp", Some(provider), Some(tool)) if !provider.is_empty() && !tool.is_empty() => {
            // OPTIMIZATION: Use into() for cleaner conversion
            Some((provider.into(), tool.into()))
        }
        _ => None,
    }
}

/// Alternative tool policy configuration format (user's format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeToolPolicyConfig {
    /// Configuration version for future compatibility
    pub version: u32,
    /// Default policy settings
    pub default: AlternativeDefaultPolicy,
    /// Tool-specific policies
    pub tools: IndexMap<String, AlternativeToolPolicy>,
    /// Optional per-tool constraints (ignored if absent)
    #[serde(default)]
    pub constraints: IndexMap<String, ToolConstraints>,
}

/// Default policy in alternative format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeDefaultPolicy {
    /// Whether to allow by default
    pub allow: bool,
    /// Rate limit per run
    pub rate_limit_per_run: u32,
    /// Max concurrent executions
    pub max_concurrent: u32,
    /// Allow filesystem writes
    pub fs_write: bool,
    /// Allow network access
    pub network: bool,
}

/// Tool policy in alternative format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeToolPolicy {
    /// Whether to allow this tool
    pub allow: bool,
    /// Allow filesystem writes (optional)
    #[serde(default)]
    pub fs_write: bool,
    /// Allow network access (optional)
    #[serde(default)]
    pub network: bool,
    /// Arguments policy (optional)
    #[serde(default)]
    pub args_policy: Option<AlternativeArgsPolicy>,
}

/// Arguments policy in alternative format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeArgsPolicy {
    /// Substrings to deny
    pub deny_substrings: Vec<String>,
}

/// Handler for tool permission prompts
///
/// This trait allows different UI modes (CLI, TUI) to provide their own
/// implementation for prompting users about tool execution.
pub trait PermissionPromptHandler: Send + Sync {
    /// Prompt the user for tool execution permission
    fn prompt_tool_permission(&mut self, tool_name: &str) -> Result<ToolExecutionDecision>;
}

/// Tool policy manager
pub struct ToolPolicyManager {
    config_path: PathBuf,
    config: ToolPolicyConfig,
    permission_handler: Option<Box<dyn PermissionPromptHandler>>,
    workspace_root: Option<PathBuf>,
}

impl Clone for ToolPolicyManager {
    fn clone(&self) -> Self {
        // Note: Permission handler is not cloned - this is intentional as handlers
        // typically contain UI state that shouldn't be duplicated
        Self {
            config_path: self.config_path.clone(),
            config: self.config.clone(),
            permission_handler: None, // Handler is not cloned
            workspace_root: self.workspace_root.clone(),
        }
    }
}

impl ToolPolicyManager {
    /// Create a new tool policy manager
    pub async fn new() -> Result<Self> {
        let config_path = Self::get_config_path().await?;
        let config = Self::load_or_create_config(&config_path).await?;

        Ok(Self {
            config_path,
            config,
            permission_handler: None,
            workspace_root: None,
        })
    }

    /// Create a new tool policy manager with workspace-specific config
    pub async fn new_with_workspace(workspace_root: &Path) -> Result<Self> {
        let config_path = Self::get_workspace_config_path(workspace_root).await?;
        let config = Self::load_or_create_config(&config_path).await?;

        Ok(Self {
            config_path,
            config,
            permission_handler: None,
            workspace_root: Some(workspace_root.to_path_buf()),
        })
    }

    /// Create a new tool policy manager backed by a custom configuration path.
    ///
    /// This helper allows downstream consumers to store policy data alongside
    /// their own configuration hierarchy instead of writing to the default
    /// `.vtcode` directory.
    pub async fn new_with_config_path<P: Into<PathBuf>>(config_path: P) -> Result<Self> {
        let config_path = config_path.into();

        if let Some(parent) = config_path.parent()
            && !tokio::fs::try_exists(parent).await.unwrap_or(false)
        {
            ensure_dir_exists(parent)
                .await
                .with_context(|| format!("{} at {}", ERR_CREATE_POLICY_DIR, parent.display()))?;
        }

        let config = Self::load_or_create_config(&config_path).await?;

        Ok(Self {
            config_path,
            config,
            permission_handler: None,
            workspace_root: None,
        })
    }

    /// Set the permission handler for this manager
    pub fn set_permission_handler(&mut self, handler: Box<dyn PermissionPromptHandler>) {
        self.permission_handler = Some(handler);
    }

    /// Get the path to the tool policy configuration file
    async fn get_config_path() -> Result<PathBuf> {
        let home_dir = dirs::home_dir().context("Could not determine home directory")?;

        let vtcode_dir = home_dir.join(".vtcode");
        if !tokio::fs::try_exists(&vtcode_dir).await.unwrap_or(false) {
            ensure_dir_exists(&vtcode_dir)
                .await
                .context("Failed to create ~/.vtcode directory")?;
        }

        Ok(vtcode_dir.join("tool-policy.json"))
    }

    /// Get the path to the workspace-specific tool policy configuration file
    async fn get_workspace_config_path(workspace_root: &Path) -> Result<PathBuf> {
        let workspace_vtcode_dir = workspace_root.join(".vtcode");

        if !tokio::fs::try_exists(&workspace_vtcode_dir)
            .await
            .unwrap_or(false)
        {
            ensure_dir_exists(&workspace_vtcode_dir)
                .await
                .with_context(|| {
                    format!(
                        "Failed to create workspace policy directory at {}",
                        workspace_vtcode_dir.display()
                    )
                })?;
        }

        Ok(workspace_vtcode_dir.join("tool-policy.json"))
    }

    /// Load existing config or create new one with all tools as "prompt"
    async fn load_or_create_config(config_path: &PathBuf) -> Result<ToolPolicyConfig> {
        if tokio::fs::try_exists(config_path).await.unwrap_or(false) {
            let content = read_file_with_context(config_path, "tool policy config")
                .await
                .context("Failed to read tool policy config")?;

            // Try to parse as alternative format first
            if let Ok(alt_config) = serde_json::from_str::<AlternativeToolPolicyConfig>(&content) {
                // Convert alternative format to standard format
                return Ok(Self::convert_from_alternative(alt_config));
            }

            // Fall back to standard format with graceful recovery on parse errors
            match serde_json::from_str(&content) {
                Ok(mut config) => {
                    Self::apply_auto_allow_defaults(&mut config);
                    Self::ensure_network_constraints(&mut config);
                    Ok(config)
                }
                Err(parse_err) => {
                    tracing::warn!(
                        "Invalid tool policy config at {} ({}). Resetting to defaults.",
                        config_path.display(),
                        parse_err
                    );
                    Self::reset_to_default(config_path).await
                }
            }
        } else {
            // Create new config with empty tools list
            let mut config = ToolPolicyConfig::default();
            Self::apply_auto_allow_defaults(&mut config);
            Self::ensure_network_constraints(&mut config);
            Ok(config)
        }
    }

    fn apply_auto_allow_defaults(config: &mut ToolPolicyConfig) {
        // OPTIMIZATION: Avoid unnecessary allocations in loop
        for &tool in AUTO_ALLOW_TOOLS {
            config
                .policies
                .entry(tool.into())
                .and_modify(|policy| *policy = ToolPolicy::Allow)
                .or_insert(ToolPolicy::Allow);
            if !config.available_tools.iter().any(|t| t == tool) {
                config.available_tools.push(tool.into());
            }
        }
        Self::ensure_network_constraints(config);
    }

    fn ensure_network_constraints(_config: &mut ToolPolicyConfig) {
        // Network constraints removed with curl tool removal
    }

    async fn reset_to_default(config_path: &PathBuf) -> Result<ToolPolicyConfig> {
        let backup_path = config_path.with_extension("json.bak");

        if let Err(err) = tokio::fs::rename(config_path, &backup_path).await {
            tracing::warn!(
                "Unable to back up invalid tool policy config ({}). {}",
                config_path.display(),
                err
            );
        } else {
            tracing::info!(
                "Backed up invalid tool policy config to {}",
                backup_path.display()
            );
        }

        let default_config = ToolPolicyConfig::default();
        Self::write_config(config_path.as_path(), &default_config).await?;
        Ok(default_config)
    }

    async fn write_config(path: &Path, config: &ToolPolicyConfig) -> Result<()> {
        if let Some(parent) = path.parent()
            && !tokio::fs::try_exists(parent).await.unwrap_or(false)
        {
            ensure_dir_exists(parent)
                .await
                .with_context(|| format!("{} at {}", ERR_CREATE_POLICY_DIR, parent.display()))?;
        }

        let serialized = serde_json::to_string_pretty(config)
            .context("Failed to serialize tool policy config")?;

        write_file_with_context(path, &serialized, "tool policy config")
            .await
            .with_context(|| format!("Failed to write tool policy config: {}", path.display()))
    }

    /// Convert alternative format to standard format
    fn convert_from_alternative(alt_config: AlternativeToolPolicyConfig) -> ToolPolicyConfig {
        let mut policies = IndexMap::new();

        // Convert tool policies
        for (tool_name, alt_policy) in alt_config.tools {
            let policy = if alt_policy.allow {
                ToolPolicy::Allow
            } else {
                ToolPolicy::Deny
            };
            policies.insert(tool_name, policy);
        }

        let mut config = ToolPolicyConfig {
            version: alt_config.version,
            available_tools: policies.keys().cloned().collect(),
            policies,
            constraints: alt_config.constraints,
            mcp: McpPolicyStore::default(),
        };
        Self::apply_auto_allow_defaults(&mut config);
        config
    }

    fn apply_config_policy(&mut self, tool_name: &str, policy: ConfigToolPolicy) {
        let canonical = canonical_tool_name(tool_name);
        let runtime_policy = match policy {
            ConfigToolPolicy::Allow => ToolPolicy::Allow,
            ConfigToolPolicy::Prompt => ToolPolicy::Prompt,
            ConfigToolPolicy::Deny => ToolPolicy::Deny,
        };

        self.config
            .policies
            .insert(canonical.into_owned(), runtime_policy);
    }

    fn resolve_config_policy(tools_config: &ToolsConfig, tool_name: &str) -> ConfigToolPolicy {
        let canonical = canonical_tool_name(tool_name);
        let lookup: &str = &canonical;

        if let Some(policy) = tools_config.policies.get(lookup) {
            return *policy;
        }

        match tool_name {
            tools::LIST_FILES => tools_config
                .policies
                .get("list_dir")
                .or_else(|| tools_config.policies.get("list_directory"))
                .cloned(),
            _ => None,
        }
        .unwrap_or(tools_config.default_policy)
    }

    /// Apply policies defined in vtcode.toml to the runtime policy manager
    pub async fn apply_tools_config(&mut self, tools_config: &ToolsConfig) -> Result<()> {
        if self.config.available_tools.is_empty() {
            return Ok(());
        }

        // Clone once to avoid borrow issues with self.apply_config_policy
        let tools: Vec<_> = self.config.available_tools.to_vec();
        for tool in tools {
            let config_policy = Self::resolve_config_policy(tools_config, &tool);
            self.apply_config_policy(&tool, config_policy);
        }

        Self::apply_auto_allow_defaults(&mut self.config);
        self.save_config().await
    }

    /// Update the tool list and save configuration
    pub async fn update_available_tools(&mut self, tools: Vec<String>) -> Result<()> {
        // OPTIMIZATION: Use HashSet for deduplication, then convert to sorted Vec
        let mut canonical_tools = Vec::with_capacity(tools.len());
        let mut seen = HashSet::with_capacity(tools.len());

        for tool in tools {
            let canonical = canonical_tool_name(&tool).into_owned();
            if seen.insert(canonical.clone()) {
                canonical_tools.push(canonical);
            }
        }
        canonical_tools.sort();

        let current_tools: HashSet<_> = self.config.policies.keys().cloned().collect();
        let new_tools: HashSet<_> = canonical_tools
            .iter()
            .filter(|name| !name.starts_with("mcp::"))
            .cloned()
            .collect();

        let mut has_changes = false;

        for tool in canonical_tools
            .iter()
            .filter(|tool| !tool.starts_with("mcp::") && !current_tools.contains(*tool))
        {
            let default_policy = if AUTO_ALLOW_TOOLS.contains(&tool.as_str()) {
                ToolPolicy::Allow
            } else {
                ToolPolicy::Prompt
            };
            self.config.policies.insert(tool.clone(), default_policy);
            has_changes = true;
        }

        let tools_to_remove: Vec<_> = self
            .config
            .policies
            .keys()
            .filter(|tool| !new_tools.contains(*tool))
            .cloned()
            .collect();

        for tool in tools_to_remove {
            self.config.policies.shift_remove(&tool);
            has_changes = true;
        }

        // Only clone if we need to compare/sort
        let mut sorted_available = self.config.available_tools.clone();
        sorted_available.sort();
        if sorted_available != canonical_tools {
            self.config.available_tools = canonical_tools;
            has_changes = true;
        }

        Self::ensure_network_constraints(&mut self.config);

        if has_changes {
            self.save_config().await
        } else {
            Ok(())
        }
    }

    /// Synchronize MCP provider tool lists with persisted policies
    pub async fn update_mcp_tools(
        &mut self,
        provider_tools: &HashMap<String, Vec<String>>,
    ) -> Result<()> {
        let stored_providers: HashSet<String> = self.config.mcp.providers.keys().cloned().collect();
        let mut has_changes = false;

        // Update or insert provider entries
        for (provider, tools) in provider_tools {
            let entry = self
                .config
                .mcp
                .providers
                .entry(provider.clone())
                .or_default();

            let existing_tools: HashSet<_> = entry.tools.keys().cloned().collect();
            let advertised: HashSet<_> = tools.iter().cloned().collect();

            // Add new tools with default Prompt policy
            for tool in tools {
                if !existing_tools.contains(tool) {
                    entry.tools.insert(tool.clone(), ToolPolicy::Prompt);
                    has_changes = true;
                }
            }

            // Remove tools no longer advertised
            for stale in existing_tools.difference(&advertised) {
                entry.tools.shift_remove(stale);
                has_changes = true;
            }
        }

        // Remove providers that are no longer present
        let advertised_providers: HashSet<String> = provider_tools.keys().cloned().collect();
        for provider in stored_providers
            .difference(&advertised_providers)
            .cloned()
            .collect::<Vec<_>>()
        {
            self.config.mcp.providers.shift_remove(provider.as_str());
            has_changes = true;
        }

        // Remove any stale MCP keys from the primary policy map
        let stale_runtime_keys: Vec<_> = self
            .config
            .policies
            .keys()
            .filter(|name| name.starts_with("mcp::"))
            .cloned()
            .collect();

        for key in stale_runtime_keys {
            self.config.policies.shift_remove(&key);
            has_changes = true;
        }

        // Refresh available tools list with MCP entries included
        let mut available: Vec<String> = self
            .config
            .available_tools
            .iter()
            .filter(|name| !name.starts_with("mcp::"))
            .cloned()
            .collect();

        available.extend(
            self.config
                .mcp
                .providers
                .iter()
                .flat_map(|(provider, policy)| {
                    policy
                        .tools
                        .keys()
                        .map(move |tool| format!("mcp::{}::{}", provider, tool))
                }),
        );

        available.sort();
        available.dedup();

        // Check if the available tools list has actually changed
        if self.config.available_tools != available {
            self.config.available_tools = available;
            has_changes = true;
        }

        if has_changes {
            self.save_config().await
        } else {
            Ok(())
        }
    }

    /// Retrieve policy for a specific MCP tool
    pub fn get_mcp_tool_policy(&self, provider: &str, tool: &str) -> ToolPolicy {
        self.config
            .mcp
            .providers
            .get(provider)
            .and_then(|policy| policy.tools.get(tool))
            .cloned()
            .unwrap_or(ToolPolicy::Prompt)
    }

    /// Update policy for a specific MCP tool
    pub async fn set_mcp_tool_policy(
        &mut self,
        provider: &str,
        tool: &str,
        policy: ToolPolicy,
    ) -> Result<()> {
        // OPTIMIZATION: Use into() for cleaner conversion
        let entry = self
            .config
            .mcp
            .providers
            .entry(provider.into())
            .or_default();
        entry.tools.insert(tool.into(), policy);
        self.save_config().await
    }

    /// Access the persisted MCP allow list configuration
    pub fn mcp_allowlist(&self) -> &McpAllowListConfig {
        &self.config.mcp.allowlist
    }

    /// Replace the persisted MCP allow list configuration
    pub async fn set_mcp_allowlist(&mut self, allowlist: McpAllowListConfig) -> Result<()> {
        self.config.mcp.allowlist = allowlist;
        self.save_config().await
    }

    /// Get policy for a specific tool
    pub fn get_policy(&self, tool_name: &str) -> ToolPolicy {
        let canonical = canonical_tool_name(tool_name);
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return self.get_mcp_tool_policy(&provider, &tool);
        }

        self.config
            .policies
            .get(&*canonical)
            .cloned()
            .unwrap_or(ToolPolicy::Prompt)
    }

    /// Get optional constraints for a specific tool
    pub fn get_constraints(&self, tool_name: &str) -> Option<&ToolConstraints> {
        let canonical = canonical_tool_name(tool_name);
        self.config.constraints.get(&*canonical)
    }

    /// Check if tool should be executed based on policy
    pub async fn should_execute_tool(&mut self, tool_name: &str) -> Result<ToolExecutionDecision> {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return match self.get_mcp_tool_policy(&provider, &tool) {
                ToolPolicy::Allow => Ok(ToolExecutionDecision::Allowed),
                ToolPolicy::Deny => Ok(ToolExecutionDecision::Denied),
                ToolPolicy::Prompt => {
                    if ToolPolicyManager::is_auto_allow_tool(tool_name) {
                        self.set_mcp_tool_policy(&provider, &tool, ToolPolicy::Allow)
                            .await?;
                        Ok(ToolExecutionDecision::Allowed)
                    } else {
                        // Use permission handler if available
                        if let Some(ref mut handler) = self.permission_handler {
                            handler.prompt_tool_permission(tool_name)
                        } else {
                            // Default: allow through (for backward compatibility)
                            Ok(ToolExecutionDecision::Allowed)
                        }
                    }
                }
            };
        }

        let canonical = canonical_tool_name(tool_name);

        match self.get_policy(canonical.as_ref()) {
            ToolPolicy::Allow => Ok(ToolExecutionDecision::Allowed),
            ToolPolicy::Deny => Ok(ToolExecutionDecision::Denied),
            ToolPolicy::Prompt => {
                let canonical_name = canonical.as_ref();
                if AUTO_ALLOW_TOOLS.contains(&canonical_name) {
                    self.set_policy(canonical_name, ToolPolicy::Allow).await?;
                    return Ok(ToolExecutionDecision::Allowed);
                }
                // Use permission handler if available
                if let Some(ref mut handler) = self.permission_handler {
                    handler.prompt_tool_permission(tool_name)
                } else {
                    // Default: allow through (for backward compatibility)
                    Ok(ToolExecutionDecision::Allowed)
                }
            }
        }
    }

    pub fn is_auto_allow_tool(tool_name: &str) -> bool {
        let canonical = canonical_tool_name(tool_name);
        AUTO_ALLOW_TOOLS.contains(&canonical.as_ref())
    }

    /// Prompt user for tool execution permission using the configured handler.
    ///
    /// This function delegates to the PermissionPromptHandler if one is configured.
    /// In TUI mode, the handler should be set to use TUI-based prompts via the
    /// permission handler mechanism.
    pub fn prompt_user_for_tool(&mut self, tool_name: &str) -> Result<ToolExecutionDecision> {
        if let Some(ref mut handler) = self.permission_handler {
            handler.prompt_tool_permission(tool_name)
        } else {
            // Default behavior if no handler is configured: allow through
            Ok(ToolExecutionDecision::Allowed)
        }
    }

    /// Set policy for a specific tool
    pub async fn set_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return self.set_mcp_tool_policy(&provider, &tool, policy).await;
        }

        let canonical = canonical_tool_name(tool_name).into_owned();
        self.config
            .policies
            .insert(canonical.clone(), policy.clone());
        self.save_config().await?;
        self.persist_policy_to_workspace_config(&canonical, policy)
    }

    /// Reset all tools to prompt
    pub async fn reset_all_to_prompt(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Prompt;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Prompt;
            }
        }
        self.save_config().await
    }

    /// Allow all tools
    pub async fn allow_all_tools(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Allow;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Allow;
            }
        }
        self.save_config().await
    }

    /// Deny all tools
    pub async fn deny_all_tools(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Deny;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Deny;
            }
        }
        self.save_config().await
    }

    /// Get summary of current policies
    pub fn get_policy_summary(&self) -> IndexMap<String, ToolPolicy> {
        let mut summary = self.config.policies.clone();
        for (provider, policy) in &self.config.mcp.providers {
            for (tool, status) in &policy.tools {
                summary.insert(format!("mcp::{}::{}", provider, tool), status.clone());
            }
        }
        summary
    }

    /// Save configuration to file
    async fn save_config(&self) -> Result<()> {
        Self::write_config(&self.config_path, &self.config).await
    }

    fn persist_policy_to_workspace_config(
        &self,
        tool_name: &str,
        policy: ToolPolicy,
    ) -> Result<()> {
        let Some(workspace_root) = self.workspace_root.as_ref() else {
            return Ok(());
        };

        let config_path = workspace_root.join("vtcode.toml");
        let mut config = if config_path.exists() {
            ConfigManager::load_from_file(&config_path)
                .with_context(|| {
                    format!(
                        "Failed to load config for tool policy persistence at {}",
                        config_path.display()
                    )
                })?
                .config()
                .clone()
        } else {
            VTCodeConfig::default()
        };

        config
            .tools
            .policies
            .insert(tool_name.to_string(), Self::to_config_policy(policy));

        ConfigManager::save_config_to_path(&config_path, &config)
            .with_context(|| format!("Failed to persist tool policy to {}", config_path.display()))
    }

    fn to_config_policy(policy: ToolPolicy) -> ConfigToolPolicy {
        match policy {
            ToolPolicy::Allow => ConfigToolPolicy::Allow,
            ToolPolicy::Prompt => ConfigToolPolicy::Prompt,
            ToolPolicy::Deny => ConfigToolPolicy::Deny,
        }
    }

    /// Print current policy status
    pub fn print_status(&self) {
        println!("{}", style("Tool Policy Status").cyan().bold());
        println!("Config file: {}", self.config_path.display());
        println!();

        let summary = self.get_policy_summary();

        if summary.is_empty() {
            println!("No tools configured yet.");
            return;
        }

        let mut allow_count = 0;
        let mut prompt_count = 0;
        let mut deny_count = 0;

        for (tool, policy) in &summary {
            let (status, color_name) = match policy {
                ToolPolicy::Allow => {
                    allow_count += 1;
                    ("ALLOW", "green")
                }
                ToolPolicy::Prompt => {
                    prompt_count += 1;
                    ("PROMPT", "cyan")
                }
                ToolPolicy::Deny => {
                    deny_count += 1;
                    ("DENY", "red")
                }
            };

            let status_styled = match color_name {
                "green" => style(status).green(),
                "cyan" => style(status).cyan(),
                "red" => style(status).red(),
                _ => style(status),
            };

            println!(
                "  {} {}",
                style(format!("{:15}", tool)).cyan(),
                status_styled
            );
        }

        println!();
        println!(
            "Summary: {} allowed, {} prompt, {} denied",
            style(allow_count).green(),
            style(prompt_count).cyan(),
            style(deny_count).red()
        );
    }

    /// Expose path of the underlying policy configuration file
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}

/// Scoped, optional constraints for a tool to align with safe defaults
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolConstraints {
    /// Whitelisted modes for tools that support modes (e.g., 'terminal')
    #[serde(default)]
    pub allowed_modes: Option<Vec<String>>,
    /// Cap on results for list/search-like tools
    #[serde(default)]
    pub max_results_per_call: Option<usize>,
    /// Cap on items scanned for file listing
    #[serde(default)]
    pub max_items_per_call: Option<usize>,
    /// Default response format if unspecified by caller
    #[serde(default)]
    pub default_response_format: Option<String>,
    /// Cap maximum bytes when reading files
    #[serde(default)]
    pub max_bytes_per_read: Option<usize>,
    /// Cap maximum bytes when fetching over the network
    #[serde(default)]
    pub max_response_bytes: Option<usize>,
    /// Allowed URL schemes for network tools
    #[serde(default)]
    pub allowed_url_schemes: Option<Vec<String>>,
    /// Denied URL hosts or suffixes for network tools
    #[serde(default)]
    pub denied_url_hosts: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use tempfile::tempdir;

    #[test]
    fn test_tool_policy_config_serialization() {
        let mut config = ToolPolicyConfig::default();
        config.available_tools = vec![tools::READ_FILE.to_owned(), tools::WRITE_FILE.to_owned()];
        config
            .policies
            .insert(tools::READ_FILE.to_owned(), ToolPolicy::Allow);
        config
            .policies
            .insert(tools::WRITE_FILE.to_owned(), ToolPolicy::Prompt);

        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: ToolPolicyConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.available_tools, deserialized.available_tools);
        assert_eq!(config.policies, deserialized.policies);
    }

    #[tokio::test]
    async fn test_policy_updates() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("tool-policy.json");

        let mut config = ToolPolicyConfig::default();
        config.available_tools = vec!["tool1".to_owned()];
        config
            .policies
            .insert("tool1".to_owned(), ToolPolicy::Prompt);

        // Save initial config
        let content = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, content).unwrap();

        // Load and update
        let mut loaded_config = ToolPolicyManager::load_or_create_config(&config_path)
            .await
            .unwrap();

        // Add new tool
        let new_tools = vec!["tool1".to_owned(), "tool2".to_owned()];
        let current_tools: std::collections::HashSet<_> =
            loaded_config.available_tools.iter().collect();

        for tool in &new_tools {
            if !current_tools.contains(tool) {
                loaded_config
                    .policies
                    .insert(tool.clone(), ToolPolicy::Prompt);
            }
        }

        loaded_config.available_tools = new_tools;

        assert!(loaded_config.policies.len() >= 2);
        assert_eq!(
            loaded_config.policies.get("tool2"),
            Some(&ToolPolicy::Prompt)
        );
        assert_eq!(
            loaded_config.policies.get("tool1"),
            Some(&ToolPolicy::Prompt)
        );
    }
}
