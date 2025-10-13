//! Tool policy management system
//!
//! This module manages user preferences for tool usage, storing choices in
//! ~/.vtcode/tool-policy.json to minimize repeated prompts while maintaining
//! user control overwhich tools the agent can use.

use anyhow::{Context, Result};
use dialoguer::{
    Confirm,
    console::{Color as ConsoleColor, Style as ConsoleStyle, style},
    theme::ColorfulTheme,
};
use indexmap::IndexMap;
use is_terminal::IsTerminal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ui::theme;
use crate::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::config::constants::tools;
use crate::config::core::tools::{ToolPolicy as ConfigToolPolicy, ToolsConfig};
use crate::config::mcp::{McpAllowListConfig, McpAllowListRules};

const AUTO_ALLOW_TOOLS: &[&str] = &[
    tools::GREP_SEARCH,
    tools::LIST_FILES,
    tools::UPDATE_PLAN,
    tools::RUN_TERMINAL_CMD,
    tools::READ_FILE,
    tools::EDIT_FILE,
    tools::AST_GREP_SEARCH,
    tools::SIMPLE_SEARCH,
    tools::BASH,
];
const DEFAULT_CURL_MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Tool execution policy
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    /// Allow tool execution without prompting
    Allow,
    /// Prompt user for confirmation each time
    Prompt,
    /// Never allow tool execution
    Deny,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        ToolPolicy::Prompt
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

/// Prompt request describing the context for an approval decision.
pub struct ToolPromptRequest<'a> {
    /// Name of the tool being evaluated.
    pub tool_name: &'a str,
    /// Whether the tool is part of the trusted auto-allow set.
    pub is_trusted: bool,
    /// Optional persisted constraints for the tool.
    pub constraints: Option<&'a ToolConstraints>,
}

/// Outcome of a prompt request returned by a [`ToolPromptBackend`].
#[derive(Debug, Clone)]
pub enum ToolPromptDecision {
    /// Allow the tool invocation for the current request only.
    AllowOnce,
    /// Deny the tool invocation for the current request only.
    DenyOnce,
    /// Persist a policy update before proceeding (e.g., remember allow/deny).
    Remember(ToolPolicy),
}

/// Abstraction for prompting a user (or policy engine) to approve tool usage.
pub trait ToolPromptBackend: Send + Sync {
    /// Request a decision for the provided tool prompt context.
    fn request_decision(&self, request: &ToolPromptRequest<'_>) -> Result<ToolPromptDecision>;
}

/// Scope describing where a policy change was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPolicyScope {
    /// Standard vtcode tool registered in the primary policy map.
    Standard,
    /// MCP-provided tool scoped to a specific provider.
    Mcp { provider: String },
}

/// Reason describing what triggered a policy change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPolicyChangeSource {
    /// Explicit updates performed through the public API (`set_policy`).
    Manual,
    /// Result of a remembered interactive prompt decision.
    PromptRemember,
    /// Auto-approval of trusted tools while evaluating execution flow.
    AutoAllow,
    /// Bulk reset of all policies to prompt.
    BulkReset,
    /// Bulk allow of all tools.
    BulkAllow,
    /// Bulk deny of all tools.
    BulkDeny,
    /// Synchronization from configuration files (e.g., `vtcode.toml`).
    ConfigSync,
    /// Synchronization with workspace tool discovery.
    AvailabilitySync,
    /// Synchronization with MCP provider advertisements.
    McpSync,
}

/// Recorded change that can be forwarded to audit subscribers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPolicyAuditEvent {
    /// Tool identifier (standard tool name or MCP composite key).
    pub tool_name: String,
    /// Location where the policy is stored.
    pub scope: ToolPolicyScope,
    /// Previous policy, if the tool had one.
    pub previous_policy: Option<ToolPolicy>,
    /// New policy that was persisted.
    pub new_policy: ToolPolicy,
    /// Source describing why the policy changed.
    pub source: ToolPolicyChangeSource,
}

/// Sink that receives policy change notifications.
pub trait ToolPolicyAuditSink: Send + Sync {
    /// Record a policy change event.
    fn record_event(&self, event: &ToolPolicyAuditEvent) -> Result<()>;
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

fn default_secure_mcp_allowlist() -> McpAllowListConfig {
    let mut allowlist = McpAllowListConfig::default();
    allowlist.enforce = true;

    allowlist.default.logging = Some(vec![
        "mcp.provider_initialized".to_string(),
        "mcp.provider_initialization_failed".to_string(),
        "mcp.tool_filtered".to_string(),
        "mcp.tool_execution".to_string(),
        "mcp.tool_failed".to_string(),
        "mcp.tool_denied".to_string(),
    ]);

    allowlist.default.configuration = Some(BTreeMap::from([
        (
            "client".to_string(),
            vec![
                "max_concurrent_connections".to_string(),
                "request_timeout_seconds".to_string(),
                "retry_attempts".to_string(),
            ],
        ),
        (
            "ui".to_string(),
            vec![
                "mode".to_string(),
                "max_events".to_string(),
                "show_provider_names".to_string(),
            ],
        ),
        (
            "server".to_string(),
            vec![
                "enabled".to_string(),
                "bind_address".to_string(),
                "port".to_string(),
                "transport".to_string(),
                "name".to_string(),
                "version".to_string(),
            ],
        ),
    ]));

    let mut time_rules = McpAllowListRules::default();
    time_rules.tools = Some(vec![
        "get_*".to_string(),
        "list_*".to_string(),
        "convert_timezone".to_string(),
        "describe_timezone".to_string(),
        "time_*".to_string(),
    ]);
    time_rules.resources = Some(vec!["timezone:*".to_string(), "location:*".to_string()]);
    time_rules.logging = Some(vec![
        "mcp.tool_execution".to_string(),
        "mcp.tool_failed".to_string(),
        "mcp.tool_denied".to_string(),
        "mcp.tool_filtered".to_string(),
        "mcp.provider_initialized".to_string(),
    ]);
    time_rules.configuration = Some(BTreeMap::from([
        (
            "provider".to_string(),
            vec!["max_concurrent_requests".to_string()],
        ),
        (
            "time".to_string(),
            vec!["local_timezone_override".to_string()],
        ),
    ]));
    allowlist.providers.insert("time".to_string(), time_rules);

    let mut context_rules = McpAllowListRules::default();
    context_rules.tools = Some(vec![
        "search_*".to_string(),
        "fetch_*".to_string(),
        "list_*".to_string(),
        "context7_*".to_string(),
        "get_*".to_string(),
    ]);
    context_rules.resources = Some(vec![
        "docs::*".to_string(),
        "snippets::*".to_string(),
        "repositories::*".to_string(),
        "context7::*".to_string(),
    ]);
    context_rules.prompts = Some(vec![
        "context7::*".to_string(),
        "support::*".to_string(),
        "docs::*".to_string(),
    ]);
    context_rules.logging = Some(vec![
        "mcp.tool_execution".to_string(),
        "mcp.tool_failed".to_string(),
        "mcp.tool_denied".to_string(),
        "mcp.tool_filtered".to_string(),
        "mcp.provider_initialized".to_string(),
    ]);
    context_rules.configuration = Some(BTreeMap::from([
        (
            "provider".to_string(),
            vec!["max_concurrent_requests".to_string()],
        ),
        (
            "context7".to_string(),
            vec![
                "workspace".to_string(),
                "search_scope".to_string(),
                "max_results".to_string(),
            ],
        ),
    ]));
    allowlist
        .providers
        .insert("context7".to_string(), context_rules);

    let mut seq_rules = McpAllowListRules::default();
    seq_rules.tools = Some(vec![
        "plan".to_string(),
        "critique".to_string(),
        "reflect".to_string(),
        "decompose".to_string(),
        "sequential_*".to_string(),
    ]);
    seq_rules.prompts = Some(vec![
        "sequential-thinking::*".to_string(),
        "plan".to_string(),
        "reflect".to_string(),
        "critique".to_string(),
    ]);
    seq_rules.logging = Some(vec![
        "mcp.tool_execution".to_string(),
        "mcp.tool_failed".to_string(),
        "mcp.tool_denied".to_string(),
        "mcp.tool_filtered".to_string(),
        "mcp.provider_initialized".to_string(),
    ]);
    seq_rules.configuration = Some(BTreeMap::from([
        (
            "provider".to_string(),
            vec!["max_concurrent_requests".to_string()],
        ),
        (
            "sequencing".to_string(),
            vec!["max_depth".to_string(), "max_branches".to_string()],
        ),
    ]));
    allowlist
        .providers
        .insert("sequential-thinking".to_string(), seq_rules);

    allowlist
}

fn parse_mcp_policy_key(tool_name: &str) -> Option<(String, String)> {
    let mut parts = tool_name.splitn(3, "::");
    match (parts.next()?, parts.next(), parts.next()) {
        ("mcp", Some(provider), Some(tool)) if !provider.is_empty() && !tool.is_empty() => {
            Some((provider.to_string(), tool.to_string()))
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

/// Tool policy manager
pub struct ToolPolicyManager {
    config_path: PathBuf,
    config: ToolPolicyConfig,
    prompt_backend: Arc<dyn ToolPromptBackend>,
    audit_sinks: Vec<Arc<dyn ToolPolicyAuditSink>>,
}

impl Clone for ToolPolicyManager {
    fn clone(&self) -> Self {
        Self {
            config_path: self.config_path.clone(),
            config: self.config.clone(),
            prompt_backend: Arc::clone(&self.prompt_backend),
            audit_sinks: self.audit_sinks.iter().cloned().collect(),
        }
    }
}

impl ToolPolicyManager {
    /// Create a new tool policy manager
    pub fn new() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        Self::new_with_config_path_and_prompt(config_path, Arc::new(DialoguerToolPrompt::default()))
    }

    /// Create a new tool policy manager that uses a custom prompt backend.
    pub fn new_with_prompt_backend(prompt_backend: Arc<dyn ToolPromptBackend>) -> Result<Self> {
        let config_path = Self::get_config_path()?;
        Self::new_with_config_path_and_prompt(config_path, prompt_backend)
    }

    /// Create a new tool policy manager with workspace-specific config
    pub fn new_with_workspace(workspace_root: &PathBuf) -> Result<Self> {
        Self::new_with_workspace_and_prompt(
            workspace_root,
            Arc::new(DialoguerToolPrompt::default()),
        )
    }

    /// Create a new tool policy manager with workspace-specific config and custom prompt backend
    pub fn new_with_workspace_and_prompt(
        workspace_root: &PathBuf,
        prompt_backend: Arc<dyn ToolPromptBackend>,
    ) -> Result<Self> {
        let config_path = Self::get_workspace_config_path(workspace_root)?;
        Self::new_with_config_path_and_prompt(config_path, prompt_backend)
    }

    /// Get the path to the tool policy configuration file
    fn get_config_path() -> Result<PathBuf> {
        let home_dir = dirs::home_dir().context("Could not determine home directory")?;

        let vtcode_dir = home_dir.join(".vtcode");
        if !vtcode_dir.exists() {
            fs::create_dir_all(&vtcode_dir).context("Failed to create ~/.vtcode directory")?;
        }

        Ok(vtcode_dir.join("tool-policy.json"))
    }

    /// Get the path to the workspace-specific tool policy configuration file
    fn get_workspace_config_path(workspace_root: &PathBuf) -> Result<PathBuf> {
        let workspace_vtcode_dir = workspace_root.join(".vtcode");

        if !workspace_vtcode_dir.exists() {
            fs::create_dir_all(&workspace_vtcode_dir).with_context(|| {
                format!(
                    "Failed to create workspace policy directory at {}",
                    workspace_vtcode_dir.display()
                )
            })?;
        }

        Ok(workspace_vtcode_dir.join("tool-policy.json"))
    }

    /// Load existing config or create new one with all tools as "prompt"
    fn load_or_create_config(config_path: &PathBuf) -> Result<ToolPolicyConfig> {
        if config_path.exists() {
            let content =
                fs::read_to_string(config_path).context("Failed to read tool policy config")?;

            // Try to parse as alternative format first
            if let Ok(alt_config) = serde_json::from_str::<AlternativeToolPolicyConfig>(&content) {
                // Convert alternative format to standard format
                return Ok(Self::convert_from_alternative(alt_config));
            }

            // Fall back to standard format with graceful recovery on parse errors
            match serde_json::from_str(&content) {
                Ok(mut config) => {
                    let _ = Self::apply_auto_allow_defaults(&mut config);
                    Self::ensure_network_constraints(&mut config);
                    Ok(config)
                }
                Err(parse_err) => {
                    eprintln!(
                        "Warning: Invalid tool policy config at {} ({}). Resetting to defaults.",
                        config_path.display(),
                        parse_err
                    );
                    Self::reset_to_default(config_path)
                }
            }
        } else {
            // Create new config with empty tools list
            let mut config = ToolPolicyConfig::default();
            let _ = Self::apply_auto_allow_defaults(&mut config);
            Self::ensure_network_constraints(&mut config);
            Ok(config)
        }
    }

    fn new_with_config_path_and_prompt(
        config_path: PathBuf,
        prompt_backend: Arc<dyn ToolPromptBackend>,
    ) -> Result<Self> {
        let config = Self::load_or_create_config(&config_path)?;

        Ok(Self {
            config_path,
            config,
            prompt_backend,
            audit_sinks: Vec::new(),
        })
    }

    fn apply_auto_allow_defaults(
        config: &mut ToolPolicyConfig,
    ) -> Vec<(String, Option<ToolPolicy>)> {
        let mut changes = Vec::new();
        for tool in AUTO_ALLOW_TOOLS {
            let previous = config
                .policies
                .insert((*tool).to_string(), ToolPolicy::Allow);
            if previous
                .as_ref()
                .map(|policy| policy != &ToolPolicy::Allow)
                .unwrap_or(true)
            {
                changes.push(((*tool).to_string(), previous));
            }
            if !config.available_tools.contains(&tool.to_string()) {
                config.available_tools.push(tool.to_string());
            }
        }
        Self::ensure_network_constraints(config);
        changes
    }

    fn ensure_network_constraints(config: &mut ToolPolicyConfig) {
        let entry = config
            .constraints
            .entry(tools::CURL.to_string())
            .or_insert_with(ToolConstraints::default);

        if entry.max_response_bytes.is_none() {
            entry.max_response_bytes = Some(DEFAULT_CURL_MAX_RESPONSE_BYTES);
        }
        if entry.allowed_url_schemes.is_none() {
            entry.allowed_url_schemes = Some(vec!["https".to_string()]);
        }
        if entry.denied_url_hosts.is_none() {
            entry.denied_url_hosts = Some(vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "0.0.0.0".to_string(),
                "::1".to_string(),
                ".localhost".to_string(),
                ".local".to_string(),
                ".internal".to_string(),
                ".lan".to_string(),
            ]);
        }
    }

    fn reset_to_default(config_path: &PathBuf) -> Result<ToolPolicyConfig> {
        let backup_path = config_path.with_extension("json.bak");

        if let Err(err) = fs::rename(config_path, &backup_path) {
            eprintln!(
                "Warning: Unable to back up invalid tool policy config ({}). {}",
                config_path.display(),
                err
            );
        } else {
            eprintln!(
                "Backed up invalid tool policy config to {}",
                backup_path.display()
            );
        }

        let default_config = ToolPolicyConfig::default();
        Self::write_config(config_path.as_path(), &default_config)?;
        Ok(default_config)
    }

    fn write_config(path: &Path, config: &ToolPolicyConfig) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create directory for tool policy config at {}",
                        parent.display()
                    )
                })?;
            }
        }

        let serialized = serde_json::to_string_pretty(config)
            .context("Failed to serialize tool policy config")?;

        fs::write(path, serialized)
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
        let _ = Self::apply_auto_allow_defaults(&mut config);
        config
    }

    fn apply_config_policy(&mut self, tool_name: &str, policy: ConfigToolPolicy) {
        let runtime_policy = match policy {
            ConfigToolPolicy::Allow => ToolPolicy::Allow,
            ConfigToolPolicy::Prompt => ToolPolicy::Prompt,
            ConfigToolPolicy::Deny => ToolPolicy::Deny,
        };

        self.record_standard_policy_change(
            tool_name,
            runtime_policy,
            ToolPolicyChangeSource::ConfigSync,
        );
    }

    fn resolve_config_policy(tools_config: &ToolsConfig, tool_name: &str) -> ConfigToolPolicy {
        if let Some(policy) = tools_config.policies.get(tool_name) {
            return policy.clone();
        }

        match tool_name {
            tools::LIST_FILES => tools_config
                .policies
                .get("list_dir")
                .or_else(|| tools_config.policies.get("list_directory"))
                .cloned(),
            _ => None,
        }
        .unwrap_or_else(|| tools_config.default_policy.clone())
    }

    /// Apply policies defined in vtcode.toml to the runtime policy manager
    pub fn apply_tools_config(&mut self, tools_config: &ToolsConfig) -> Result<()> {
        if self.config.available_tools.is_empty() {
            return Ok(());
        }

        for tool in self.config.available_tools.clone() {
            let config_policy = Self::resolve_config_policy(tools_config, &tool);
            self.apply_config_policy(&tool, config_policy);
        }

        let default_changes = Self::apply_auto_allow_defaults(&mut self.config);
        for (tool, previous) in default_changes {
            self.emit_policy_change(
                &tool,
                ToolPolicyScope::Standard,
                previous,
                ToolPolicy::Allow,
                ToolPolicyChangeSource::ConfigSync,
            );
        }
        self.save_config()
    }

    /// Update the tool list and save configuration
    pub fn update_available_tools(&mut self, tools: Vec<String>) -> Result<()> {
        let current_tools: HashSet<_> = self.config.policies.keys().cloned().collect();
        let new_tools: HashSet<_> = tools
            .iter()
            .filter(|name| !name.starts_with("mcp::"))
            .cloned()
            .collect();

        let mut has_changes = false;

        // Add new tools with appropriate defaults
        for tool in tools
            .iter()
            .filter(|tool| !tool.starts_with("mcp::") && !current_tools.contains(*tool))
        {
            let default_policy = if AUTO_ALLOW_TOOLS.contains(&tool.as_str()) {
                ToolPolicy::Allow
            } else {
                ToolPolicy::Prompt
            };
            if self.record_standard_policy_change(
                tool,
                default_policy,
                ToolPolicyChangeSource::AvailabilitySync,
            ) {
                has_changes = true;
            }
        }

        // Remove deleted tools - use itertools to find tools to remove
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

        // Check if available tools list has actually changed
        if self.config.available_tools != tools {
            // Update available tools list
            self.config.available_tools = tools;
            has_changes = true;
        }

        Self::ensure_network_constraints(&mut self.config);

        if has_changes {
            self.save_config()
        } else {
            Ok(())
        }
    }

    /// Synchronize MCP provider tool lists with persisted policies
    pub fn update_mcp_tools(
        &mut self,
        provider_tools: &HashMap<String, Vec<String>>,
    ) -> Result<()> {
        let stored_providers: HashSet<String> = self.config.mcp.providers.keys().cloned().collect();
        let mut has_changes = false;

        // Update or insert provider entries
        for (provider, tools) in provider_tools {
            let provider_name = provider.clone();
            let existing_tools: HashSet<String> = self
                .config
                .mcp
                .providers
                .get(&provider_name)
                .map(|policy| policy.tools.keys().cloned().collect())
                .unwrap_or_default();
            let advertised: HashSet<String> = tools.iter().cloned().collect();

            // Add new tools with default Prompt policy
            for tool in tools {
                if !existing_tools.contains(tool) {
                    if self.record_mcp_policy_change(
                        &provider_name,
                        tool,
                        ToolPolicy::Prompt,
                        ToolPolicyChangeSource::McpSync,
                    ) {
                        has_changes = true;
                    }
                }
            }

            // Remove tools no longer advertised
            for stale in existing_tools.difference(&advertised) {
                if let Some(entry) = self.config.mcp.providers.get_mut(&provider_name) {
                    entry.tools.shift_remove(stale.as_str());
                    has_changes = true;
                }
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

        for (provider, policy) in &self.config.mcp.providers {
            for tool in policy.tools.keys() {
                available.push(format!("mcp::{}::{}", provider, tool));
            }
        }

        available.sort();
        available.dedup();

        // Check if the available tools list has actually changed
        if self.config.available_tools != available {
            self.config.available_tools = available;
            has_changes = true;
        }

        if has_changes {
            self.save_config()
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
    pub fn set_mcp_tool_policy(
        &mut self,
        provider: &str,
        tool: &str,
        policy: ToolPolicy,
    ) -> Result<()> {
        self.set_mcp_policy_with_source(provider, tool, policy, ToolPolicyChangeSource::Manual)
    }

    /// Access the persisted MCP allow list configuration
    pub fn mcp_allowlist(&self) -> &McpAllowListConfig {
        &self.config.mcp.allowlist
    }

    /// Replace the persisted MCP allow list configuration
    pub fn set_mcp_allowlist(&mut self, allowlist: McpAllowListConfig) -> Result<()> {
        self.config.mcp.allowlist = allowlist;
        self.save_config()
    }

    /// Get policy for a specific tool
    pub fn get_policy(&self, tool_name: &str) -> ToolPolicy {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return self.get_mcp_tool_policy(&provider, &tool);
        }

        self.config
            .policies
            .get(tool_name)
            .cloned()
            .unwrap_or(ToolPolicy::Prompt)
    }

    /// Get optional constraints for a specific tool
    pub fn get_constraints(&self, tool_name: &str) -> Option<&ToolConstraints> {
        self.config.constraints.get(tool_name)
    }

    /// Check if tool should be executed based on policy
    pub fn should_execute_tool(&mut self, tool_name: &str) -> Result<bool> {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return match self.get_mcp_tool_policy(&provider, &tool) {
                ToolPolicy::Allow => Ok(true),
                ToolPolicy::Deny => Ok(false),
                ToolPolicy::Prompt => {
                    if ToolPolicyManager::is_auto_allow_tool(tool_name) {
                        self.set_mcp_policy_with_source(
                            &provider,
                            &tool,
                            ToolPolicy::Allow,
                            ToolPolicyChangeSource::AutoAllow,
                        )?;
                        Ok(true)
                    } else {
                        let decision = self.prompt_user_for_tool(tool_name)?;
                        self.apply_prompt_decision(tool_name, decision)
                    }
                }
            };
        }

        match self.get_policy(tool_name) {
            ToolPolicy::Allow => Ok(true),
            ToolPolicy::Deny => Ok(false),
            ToolPolicy::Prompt => {
                if AUTO_ALLOW_TOOLS.contains(&tool_name) {
                    self.set_policy_with_source(
                        tool_name,
                        ToolPolicy::Allow,
                        ToolPolicyChangeSource::AutoAllow,
                    )?;
                    return Ok(true);
                }
                let decision = self.prompt_user_for_tool(tool_name)?;
                self.apply_prompt_decision(tool_name, decision)
            }
        }
    }

    pub fn is_auto_allow_tool(tool_name: &str) -> bool {
        AUTO_ALLOW_TOOLS.contains(&tool_name)
    }

    /// Prompt user for tool execution permission
    fn prompt_user_for_tool(&self, tool_name: &str) -> Result<ToolPromptDecision> {
        let constraints = self.get_constraints(tool_name);
        let request = ToolPromptRequest {
            tool_name,
            is_trusted: Self::is_auto_allow_tool(tool_name),
            constraints,
        };

        self.prompt_backend.request_decision(&request)
    }

    fn apply_prompt_decision(
        &mut self,
        tool_name: &str,
        decision: ToolPromptDecision,
    ) -> Result<bool> {
        match decision {
            ToolPromptDecision::AllowOnce => Ok(true),
            ToolPromptDecision::DenyOnce => Ok(false),
            ToolPromptDecision::Remember(policy) => {
                let allow = matches!(policy, ToolPolicy::Allow);
                self.set_policy_with_source(
                    tool_name,
                    policy,
                    ToolPolicyChangeSource::PromptRemember,
                )?;
                Ok(allow)
            }
        }
    }

    /// Set policy for a specific tool
    pub fn set_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        self.set_policy_with_source(tool_name, policy, ToolPolicyChangeSource::Manual)
    }

    /// Reset all tools to prompt
    pub fn reset_all_to_prompt(&mut self) -> Result<()> {
        let standard_tools: Vec<String> = self.config.policies.keys().cloned().collect();
        for tool in standard_tools {
            self.record_standard_policy_change(
                &tool,
                ToolPolicy::Prompt,
                ToolPolicyChangeSource::BulkReset,
            );
        }

        let mcp_tools: Vec<(String, Vec<String>)> = self
            .config
            .mcp
            .providers
            .iter()
            .map(|(provider, policy)| (provider.clone(), policy.tools.keys().cloned().collect()))
            .collect();

        for (provider, tools) in mcp_tools {
            for tool in tools {
                self.record_mcp_policy_change(
                    &provider,
                    &tool,
                    ToolPolicy::Prompt,
                    ToolPolicyChangeSource::BulkReset,
                );
            }
        }
        self.save_config()
    }

    /// Allow all tools
    pub fn allow_all_tools(&mut self) -> Result<()> {
        let standard_tools: Vec<String> = self.config.policies.keys().cloned().collect();
        for tool in standard_tools {
            self.record_standard_policy_change(
                &tool,
                ToolPolicy::Allow,
                ToolPolicyChangeSource::BulkAllow,
            );
        }

        let mcp_tools: Vec<(String, Vec<String>)> = self
            .config
            .mcp
            .providers
            .iter()
            .map(|(provider, policy)| (provider.clone(), policy.tools.keys().cloned().collect()))
            .collect();

        for (provider, tools) in mcp_tools {
            for tool in tools {
                self.record_mcp_policy_change(
                    &provider,
                    &tool,
                    ToolPolicy::Allow,
                    ToolPolicyChangeSource::BulkAllow,
                );
            }
        }
        self.save_config()
    }

    /// Deny all tools
    pub fn deny_all_tools(&mut self) -> Result<()> {
        let standard_tools: Vec<String> = self.config.policies.keys().cloned().collect();
        for tool in standard_tools {
            self.record_standard_policy_change(
                &tool,
                ToolPolicy::Deny,
                ToolPolicyChangeSource::BulkDeny,
            );
        }

        let mcp_tools: Vec<(String, Vec<String>)> = self
            .config
            .mcp
            .providers
            .iter()
            .map(|(provider, policy)| (provider.clone(), policy.tools.keys().cloned().collect()))
            .collect();

        for (provider, tools) in mcp_tools {
            for tool in tools {
                self.record_mcp_policy_change(
                    &provider,
                    &tool,
                    ToolPolicy::Deny,
                    ToolPolicyChangeSource::BulkDeny,
                );
            }
        }
        self.save_config()
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
    fn save_config(&self) -> Result<()> {
        Self::write_config(&self.config_path, &self.config)
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
                    ("PROMPT", "yellow")
                }
                ToolPolicy::Deny => {
                    deny_count += 1;
                    ("DENY", "red")
                }
            };

            let status_styled = match color_name {
                "green" => style(status).green(),
                "yellow" => style(status).yellow(),
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
            style(prompt_count).yellow(),
            style(deny_count).red()
        );
    }

    /// Expose path of the underlying policy configuration file
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Attach audit sinks to receive policy change notifications.
    pub fn with_audit_sinks(mut self, sinks: Vec<Arc<dyn ToolPolicyAuditSink>>) -> Self {
        self.audit_sinks = sinks;
        self
    }

    /// Register a single audit sink on an existing manager.
    pub fn register_audit_sink(&mut self, sink: Arc<dyn ToolPolicyAuditSink>) {
        self.audit_sinks.push(sink);
    }

    fn set_policy_with_source(
        &mut self,
        tool_name: &str,
        policy: ToolPolicy,
        source: ToolPolicyChangeSource,
    ) -> Result<()> {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return self.set_mcp_policy_with_source(&provider, &tool, policy, source);
        }

        self.record_standard_policy_change(tool_name, policy, source);
        self.save_config()
    }

    fn set_mcp_policy_with_source(
        &mut self,
        provider: &str,
        tool: &str,
        policy: ToolPolicy,
        source: ToolPolicyChangeSource,
    ) -> Result<()> {
        self.record_mcp_policy_change(provider, tool, policy, source);
        self.save_config()
    }

    fn record_standard_policy_change(
        &mut self,
        tool_name: &str,
        policy: ToolPolicy,
        source: ToolPolicyChangeSource,
    ) -> bool {
        let previous = self
            .config
            .policies
            .insert(tool_name.to_string(), policy.clone());
        let changed = previous
            .as_ref()
            .map(|existing| existing != &policy)
            .unwrap_or(true);
        if changed {
            self.emit_policy_change(
                tool_name,
                ToolPolicyScope::Standard,
                previous,
                policy,
                source,
            );
        }
        changed
    }

    fn record_mcp_policy_change(
        &mut self,
        provider: &str,
        tool: &str,
        policy: ToolPolicy,
        source: ToolPolicyChangeSource,
    ) -> bool {
        let entry = self
            .config
            .mcp
            .providers
            .entry(provider.to_string())
            .or_insert_with(McpProviderPolicy::default);
        let previous = entry.tools.insert(tool.to_string(), policy.clone());
        let changed = previous
            .as_ref()
            .map(|existing| existing != &policy)
            .unwrap_or(true);
        if changed {
            let composite = format!("mcp::{}::{}", provider, tool);
            self.emit_policy_change(
                &composite,
                ToolPolicyScope::Mcp {
                    provider: provider.to_string(),
                },
                previous,
                policy,
                source,
            );
        }
        changed
    }

    fn emit_policy_change(
        &self,
        tool_name: &str,
        scope: ToolPolicyScope,
        previous_policy: Option<ToolPolicy>,
        new_policy: ToolPolicy,
        source: ToolPolicyChangeSource,
    ) {
        if self.audit_sinks.is_empty() {
            return;
        }

        let event = ToolPolicyAuditEvent {
            tool_name: tool_name.to_string(),
            scope,
            previous_policy,
            new_policy,
            source,
        };

        for sink in &self.audit_sinks {
            if let Err(error) = sink.record_event(&event) {
                eprintln!(
                    "Failed to record tool policy audit event for '{}': {}",
                    tool_name, error
                );
            }
        }
    }
}

/// Default dialoguer-backed tool prompt backend used by vtcode.
#[derive(Default)]
pub struct DialoguerToolPrompt;

impl ToolPromptBackend for DialoguerToolPrompt {
    fn request_decision(&self, request: &ToolPromptRequest<'_>) -> Result<ToolPromptDecision> {
        let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        let mut renderer = AnsiRenderer::stdout();
        let banner_style = theme::banner_style();
        let header = format!("Tool Permission Request: {}", request.tool_name);

        renderer.line_with_style(banner_style.clone(), &header)?;
        renderer.line_with_style(
            banner_style.clone(),
            &format!("The agent wants to use the '{}' tool.", request.tool_name),
        )?;
        renderer.line_with_style(banner_style.clone(), "")?;
        renderer.line_with_style(
            banner_style.clone(),
            "This decision applies to the current request only.",
        )?;
        renderer.line_with_style(
            banner_style.clone(),
            "Update the policy file or use CLI flags to change the default.",
        )?;
        renderer.line_with_style(banner_style.clone(), "")?;

        if request.is_trusted {
            renderer.line_with_style(
                banner_style.clone(),
                &format!(
                    "Auto-approving '{}' tool (default trusted tool).",
                    request.tool_name
                ),
            )?;
            return Ok(ToolPromptDecision::Remember(ToolPolicy::Allow));
        }

        if !interactive {
            let message = format!(
                "Non-interactive environment detected. Auto-approving '{}' tool.",
                request.tool_name
            );
            renderer.line_with_style(banner_style.clone(), &message)?;
            return Ok(ToolPromptDecision::Remember(ToolPolicy::Allow));
        }

        let rgb = theme::banner_color();
        let to_ansi_256 = |value: u8| -> u8 {
            if value < 48 {
                0
            } else if value < 114 {
                1
            } else {
                ((value - 35) / 40).min(5)
            }
        };
        let rgb_to_index = |r: u8, g: u8, b: u8| -> u8 {
            let r_idx = to_ansi_256(r);
            let g_idx = to_ansi_256(g);
            let b_idx = to_ansi_256(b);
            16 + 36 * r_idx + 6 * g_idx + b_idx
        };
        let color_index = rgb_to_index(rgb.0, rgb.1, rgb.2);
        let dialog_color = ConsoleColor::Color256(color_index);
        let tinted_style = ConsoleStyle::new().for_stderr().fg(dialog_color);

        let mut dialog_theme = ColorfulTheme::default();
        dialog_theme.prompt_style = tinted_style;
        dialog_theme.prompt_prefix = style("—".to_string()).for_stderr().fg(dialog_color);
        dialog_theme.prompt_suffix = style("—".to_string()).for_stderr().fg(dialog_color);
        dialog_theme.hint_style = ConsoleStyle::new().for_stderr().fg(dialog_color);
        dialog_theme.defaults_style = dialog_theme.hint_style.clone();
        dialog_theme.success_prefix = style("✓".to_string()).for_stderr().fg(dialog_color);
        dialog_theme.success_suffix = style("·".to_string()).for_stderr().fg(dialog_color);
        dialog_theme.error_prefix = style("✗".to_string()).for_stderr().fg(dialog_color);
        dialog_theme.error_style = ConsoleStyle::new().for_stderr().fg(dialog_color);
        dialog_theme.values_style = ConsoleStyle::new().for_stderr().fg(dialog_color);

        let prompt_text = format!("Allow the agent to use '{}'?", request.tool_name);

        match Confirm::with_theme(&dialog_theme)
            .with_prompt(prompt_text)
            .default(false)
            .interact()
        {
            Ok(confirmed) => {
                let message = if confirmed {
                    format!("✓ Approved: '{}' tool will run now", request.tool_name)
                } else {
                    format!("✗ Denied: '{}' tool will not run", request.tool_name)
                };
                let style = if confirmed {
                    MessageStyle::Tool
                } else {
                    MessageStyle::Error
                };
                renderer.line(style, &message)?;
                if confirmed {
                    Ok(ToolPromptDecision::AllowOnce)
                } else {
                    Ok(ToolPromptDecision::DenyOnce)
                }
            }
            Err(error) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to read confirmation: {}", error),
                )?;
                Ok(ToolPromptDecision::DenyOnce)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    struct StaticDecisionPrompt {
        decision: ToolPromptDecision,
    }

    impl ToolPromptBackend for StaticDecisionPrompt {
        fn request_decision(&self, _request: &ToolPromptRequest<'_>) -> Result<ToolPromptDecision> {
            Ok(self.decision.clone())
        }
    }

    #[derive(Default)]
    struct RecordingAuditSink {
        events: Mutex<Vec<ToolPolicyAuditEvent>>,
    }

    impl ToolPolicyAuditSink for RecordingAuditSink {
        fn record_event(&self, event: &ToolPolicyAuditEvent) -> Result<()> {
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }
    }

    #[test]
    fn test_tool_policy_config_serialization() {
        let mut config = ToolPolicyConfig::default();
        config.available_tools = vec![tools::READ_FILE.to_string(), tools::WRITE_FILE.to_string()];
        config
            .policies
            .insert(tools::READ_FILE.to_string(), ToolPolicy::Allow);
        config
            .policies
            .insert(tools::WRITE_FILE.to_string(), ToolPolicy::Prompt);

        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: ToolPolicyConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.available_tools, deserialized.available_tools);
        assert_eq!(config.policies, deserialized.policies);
    }

    #[test]
    fn test_policy_updates() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("tool-policy.json");

        let mut config = ToolPolicyConfig::default();
        config.available_tools = vec!["tool1".to_string()];
        config
            .policies
            .insert("tool1".to_string(), ToolPolicy::Prompt);

        // Save initial config
        let content = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&config_path, content).unwrap();

        // Load and update
        let mut loaded_config = ToolPolicyManager::load_or_create_config(&config_path).unwrap();

        // Add new tool
        let new_tools = vec!["tool1".to_string(), "tool2".to_string()];
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

        assert_eq!(loaded_config.policies.len(), 2);
        assert_eq!(
            loaded_config.policies.get("tool2"),
            Some(&ToolPolicy::Prompt)
        );
    }

    #[test]
    fn custom_prompt_allow_once_does_not_persist_policy() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("tool-policy.json");
        let prompt = Arc::new(StaticDecisionPrompt {
            decision: ToolPromptDecision::AllowOnce,
        });

        let mut manager =
            ToolPolicyManager::new_with_config_path_and_prompt(config_path, prompt).unwrap();

        assert!(manager.should_execute_tool("example_tool").unwrap());
        assert_eq!(manager.get_policy("example_tool"), ToolPolicy::Prompt);
    }

    #[test]
    fn custom_prompt_remember_updates_policy() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("tool-policy.json");
        let prompt = Arc::new(StaticDecisionPrompt {
            decision: ToolPromptDecision::Remember(ToolPolicy::Deny),
        });

        let mut manager =
            ToolPolicyManager::new_with_config_path_and_prompt(config_path, prompt).unwrap();

        assert!(!manager.should_execute_tool("deny_tool").unwrap());
        assert_eq!(manager.get_policy("deny_tool"), ToolPolicy::Deny);
    }

    #[test]
    fn audit_sink_records_policy_transitions() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("tool-policy.json");
        let prompt = Arc::new(StaticDecisionPrompt {
            decision: ToolPromptDecision::AllowOnce,
        });

        let mut manager =
            ToolPolicyManager::new_with_config_path_and_prompt(config_path, prompt).unwrap();
        let sink = Arc::new(RecordingAuditSink::default());
        manager.register_audit_sink(Arc::clone(&sink));

        manager.set_policy("example", ToolPolicy::Allow).unwrap();
        manager.reset_all_to_prompt().unwrap();

        let events = sink.events.lock().unwrap().clone();
        assert_eq!(events.len(), 2);

        assert_eq!(
            events[0],
            ToolPolicyAuditEvent {
                tool_name: "example".to_string(),
                scope: ToolPolicyScope::Standard,
                previous_policy: None,
                new_policy: ToolPolicy::Allow,
                source: ToolPolicyChangeSource::Manual,
            }
        );

        assert_eq!(
            events[1],
            ToolPolicyAuditEvent {
                tool_name: "example".to_string(),
                scope: ToolPolicyScope::Standard,
                previous_policy: Some(ToolPolicy::Allow),
                new_policy: ToolPolicy::Prompt,
                source: ToolPolicyChangeSource::BulkReset,
            }
        );
    }
}
