//! Tool policy management system
//!
//! This module manages user preferences for tool usage, storing choices in
//! ~/.vtcode/tool-policy.json to minimize repeated prompts while maintaining
//! user control overwhich tools the agent can use.

use anyhow::{Context, Result};
use console::{Color as ConsoleColor, Style as ConsoleStyle, style};
use dialoguer::{Confirm, theme::ColorfulTheme};
use indexmap::IndexMap;
use is_terminal::IsTerminal;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::theme;
use crate::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::config::constants::tools;
use crate::config::core::tools::{ToolPolicy as ConfigToolPolicy, ToolsConfig};

const AUTO_ALLOW_TOOLS: &[&str] = &["run_terminal_cmd", "bash"];
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
}

impl Default for ToolPolicyConfig {
    fn default() -> Self {
        Self {
            version: 1,
            available_tools: Vec::new(),
            policies: IndexMap::new(),
            constraints: IndexMap::new(),
        }
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
        let current_tools: std::collections::HashSet<_> =
            self.config.policies.keys().cloned().collect();
        let new_tools: std::collections::HashSet<_> = tools.iter().cloned().collect();

        // Add new tools with appropriate defaults
        for tool in tools.iter().filter(|tool| !current_tools.contains(*tool)) {
            let default_policy = if AUTO_ALLOW_TOOLS.contains(&tool.as_str()) {
                ToolPolicy::Allow
            } else {
                ToolPolicy::Prompt
            };
            self.config.policies.insert(tool.clone(), default_policy);
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
        }

        // Update available tools list
        self.config.available_tools = tools;

        Self::ensure_network_constraints(&mut self.config);

        self.save_config()
    }

    /// Get policy for a specific tool
    pub fn get_policy(&self, tool_name: &str) -> ToolPolicy {
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
        self.config.policies.insert(tool_name.to_string(), policy);
        self.save_config()
    }

    /// Reset all tools to prompt
    pub fn reset_all_to_prompt(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Prompt;
        }
        self.save_config()
    }

    /// Allow all tools
    pub fn allow_all_tools(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Allow;
        }
        self.save_config()
    }

    /// Deny all tools
    pub fn deny_all_tools(&mut self) -> Result<()> {
        for policy in self.config.policies.values_mut() {
            *policy = ToolPolicy::Deny;
        }
        self.save_config()
    }

    /// Get summary of current policies
    pub fn get_policy_summary(&self) -> IndexMap<String, ToolPolicy> {
        self.config.policies.clone()
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

        if self.config.policies.is_empty() {
            println!("No tools configured yet.");
            return;
        }

        let mut allow_count = 0;
        let mut prompt_count = 0;
        let mut deny_count = 0;

        for (tool, policy) in &self.config.policies {
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
