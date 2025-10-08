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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone)]
pub struct ToolPolicyManager {
    config_path: PathBuf,
    config: ToolPolicyConfig,
}

impl ToolPolicyManager {
    /// Create a new tool policy manager
    pub fn new() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        let config = Self::load_or_create_config(&config_path)?;

        Ok(Self {
            config_path,
            config,
        })
    }

    /// Create a new tool policy manager with workspace-specific config
    pub fn new_with_workspace(workspace_root: &PathBuf) -> Result<Self> {
        let config_path = Self::get_workspace_config_path(workspace_root)?;
        let config = Self::load_or_create_config(&config_path)?;

        Ok(Self {
            config_path,
            config,
        })
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
                    Self::apply_auto_allow_defaults(&mut config);
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
            Self::apply_auto_allow_defaults(&mut config);
            Self::ensure_network_constraints(&mut config);
            Ok(config)
        }
    }

    fn apply_auto_allow_defaults(config: &mut ToolPolicyConfig) {
        for tool in AUTO_ALLOW_TOOLS {
            config
                .policies
                .entry((*tool).to_string())
                .and_modify(|policy| *policy = ToolPolicy::Allow)
                .or_insert(ToolPolicy::Allow);
            if !config.available_tools.contains(&tool.to_string()) {
                config.available_tools.push(tool.to_string());
            }
        }
        Self::ensure_network_constraints(config);
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
        Self::apply_auto_allow_defaults(&mut config);
        config
    }

    fn apply_config_policy(&mut self, tool_name: &str, policy: ConfigToolPolicy) {
        let runtime_policy = match policy {
            ConfigToolPolicy::Allow => ToolPolicy::Allow,
            ConfigToolPolicy::Prompt => ToolPolicy::Prompt,
            ConfigToolPolicy::Deny => ToolPolicy::Deny,
        };

        self.config
            .policies
            .insert(tool_name.to_string(), runtime_policy);
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

        Self::apply_auto_allow_defaults(&mut self.config);
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
            self.config.policies.insert(tool.clone(), default_policy);
            has_changes = true;
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
            let entry = self
                .config
                .mcp
                .providers
                .entry(provider.clone())
                .or_insert_with(McpProviderPolicy::default);

            let existing_tools: HashSet<String> = entry.tools.keys().cloned().collect();
            let advertised: HashSet<String> = tools.iter().cloned().collect();

            // Add new tools with default Prompt policy
            for tool in tools {
                if !existing_tools.contains(tool) {
                    entry.tools.insert(tool.clone(), ToolPolicy::Prompt);
                    has_changes = true;
                }
            }

            // Remove tools no longer advertised
            for stale in existing_tools.difference(&advertised) {
                entry.tools.shift_remove(stale.as_str());
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
        let entry = self
            .config
            .mcp
            .providers
            .entry(provider.to_string())
            .or_insert_with(McpProviderPolicy::default);
        entry.tools.insert(tool.to_string(), policy);
        self.save_config()
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
                        self.set_mcp_tool_policy(&provider, &tool, ToolPolicy::Allow)?;
                        Ok(true)
                    } else {
                        self.prompt_user_for_tool(tool_name)
                    }
                }
            };
        }

        match self.get_policy(tool_name) {
            ToolPolicy::Allow => Ok(true),
            ToolPolicy::Deny => Ok(false),
            ToolPolicy::Prompt => {
                if AUTO_ALLOW_TOOLS.contains(&tool_name) {
                    self.set_policy(tool_name, ToolPolicy::Allow)?;
                    return Ok(true);
                }
                let should_execute = self.prompt_user_for_tool(tool_name)?;
                Ok(should_execute)
            }
        }
    }

    pub fn is_auto_allow_tool(tool_name: &str) -> bool {
        AUTO_ALLOW_TOOLS.contains(&tool_name)
    }

    /// Prompt user for tool execution permission
    fn prompt_user_for_tool(&mut self, tool_name: &str) -> Result<bool> {
        let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        let mut renderer = AnsiRenderer::stdout();
        let banner_style = theme::banner_style();

        if !interactive {
            let message = format!(
                "Non-interactive environment detected. Auto-approving '{}' tool.",
                tool_name
            );
            renderer.line_with_style(banner_style, &message)?;
            self.set_policy(tool_name, ToolPolicy::Allow)?;
            return Ok(true);
        }

        let header = format!("Tool Permission Request: {}", tool_name);
        renderer.line_with_style(banner_style, &header)?;
        renderer.line_with_style(
            banner_style,
            &format!("The agent wants to use the '{}' tool.", tool_name),
        )?;
        renderer.line_with_style(banner_style, "")?;
        renderer.line_with_style(
            banner_style,
            "This decision applies to the current request only.",
        )?;
        renderer.line_with_style(
            banner_style,
            "Update the policy file or use CLI flags to change the default.",
        )?;
        renderer.line_with_style(banner_style, "")?;

        if AUTO_ALLOW_TOOLS.contains(&tool_name) {
            renderer.line_with_style(
                banner_style,
                &format!(
                    "Auto-approving '{}' tool (default trusted tool).",
                    tool_name
                ),
            )?;
            return Ok(true);
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

        let prompt_text = format!("Allow the agent to use '{}'?", tool_name);

        match Confirm::with_theme(&dialog_theme)
            .with_prompt(prompt_text)
            .default(false)
            .interact()
        {
            Ok(confirmed) => {
                let message = if confirmed {
                    format!("✓ Approved: '{}' tool will run now", tool_name)
                } else {
                    format!("✗ Denied: '{}' tool will not run", tool_name)
                };
                let style = if confirmed {
                    MessageStyle::Tool
                } else {
                    MessageStyle::Error
                };
                renderer.line(style, &message)?;
                Ok(confirmed)
            }
            Err(e) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to read confirmation: {}", e),
                )?;
                Ok(false)
            }
        }
    }

    /// Set policy for a specific tool
    pub fn set_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        if let Some((provider, tool)) = parse_mcp_policy_key(tool_name) {
            return self.set_mcp_tool_policy(&provider, &tool, policy);
        }

        self.config.policies.insert(tool_name.to_string(), policy);
        self.save_config()
    }

    /// Reset all tools to prompt
    pub fn reset_all_to_prompt(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Prompt;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Prompt;
            }
        }
        self.save_config()
    }

    /// Allow all tools
    pub fn allow_all_tools(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Allow;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Allow;
            }
        }
        self.save_config()
    }

    /// Deny all tools
    pub fn deny_all_tools(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Deny;
        }
        for provider in self.config.mcp.providers.values_mut() {
            for policy in provider.tools.values_mut() {
                *policy = ToolPolicy::Deny;
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
}
