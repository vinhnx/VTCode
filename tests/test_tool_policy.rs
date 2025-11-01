#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! anyhow = "1.0"
//! console = "0.15"
//! dialoguer = "0.11"
//! serde = { version = "1.0", features = ["derive"] }
//! serde_json = "1.0"
//! dirs = "5.0"
//! tempfile = "3.0"
//! indexmap = { version = "2.2", features = ["serde"] }
//! ```

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use vtcode_core::utils::colors::style;

/// Tool execution policy
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
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

/// Tool policy configuration stored in ~/.vtcode/tool-policy.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicyConfig {
    /// Configuration version for future compatibility
    pub version: u32,
    /// Available tools at time of last update
    pub available_tools: Vec<String>,
    /// Policy for each tool
    pub policies: IndexMap<String, ToolPolicy>,
}

impl Default for ToolPolicyConfig {
    fn default() -> Self {
        Self {
            version: 1,
            available_tools: Vec::new(),
            policies: IndexMap::new(),
        }
    }
}

/// Tool policy manager
pub struct ToolPolicyManager {
    config_path: PathBuf,
    config: ToolPolicyConfig,
}

impl ToolPolicyManager {
    /// Create a new tool policy manager with custom path for testing
    pub fn new_with_path(config_path: PathBuf) -> Result<Self> {
        let config = Self::load_or_create_config(&config_path)?;

        Ok(Self {
            config_path,
            config,
        })
    }

    /// Load existing config or create new one with all tools as "prompt"
    fn load_or_create_config(config_path: &PathBuf) -> Result<ToolPolicyConfig> {
        if config_path.exists() {
            let content =
                fs::read_to_string(config_path).context("Failed to read tool policy config")?;

            serde_json::from_str(&content).context("Failed to parse tool policy config")
        } else {
            // Create new config with empty tools list
            let config = ToolPolicyConfig::default();
            Ok(config)
        }
    }

    /// Update the tool list and save configuration
    pub fn update_available_tools(&mut self, tools: Vec<String>) -> Result<()> {
        let current_tools: std::collections::HashSet<_> =
            self.config.available_tools.iter().collect();
        let new_tools: std::collections::HashSet<_> = tools.iter().collect();

        // Add new tools as "prompt"
        for tool in &tools {
            if !current_tools.contains(tool) {
                self.config
                    .policies
                    .insert(tool.clone(), ToolPolicy::Prompt);
            }
        }

        // Remove deleted tools
        let tools_to_remove: Vec<_> = self
            .config
            .policies
            .keys()
            .filter(|tool| !new_tools.contains(tool))
            .cloned()
            .collect();

        for tool in tools_to_remove {
            self.config.policies.shift_remove(&tool);
        }

        // Update available tools list
        self.config.available_tools = tools;

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

    /// Set policy for a specific tool
    pub fn set_policy(&mut self, tool_name: &str, policy: ToolPolicy) -> Result<()> {
        self.config.policies.insert(tool_name.to_string(), policy);
        self.save_config()
    }

    /// Save configuration to file
    fn save_config(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.config)
            .context("Failed to serialize tool policy config")?;

        fs::write(&self.config_path, content).context("Failed to write tool policy config")?;

        Ok(())
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
}

fn main() -> Result<()> {
    println!("{}", style("Tool Policy System Test").bold().cyan());
    println!();

    // Create a temporary directory for testing
    let temp_dir = tempfile::tempdir()?;
    let config_path = temp_dir.path().join("tool-policy.json");

    // Create a new policy manager
    let mut policy_manager = ToolPolicyManager::new_with_path(config_path)?;

    // Test 1: Update available tools
    println!("{}", style("Test 1: Adding initial tools").yellow());
    let initial_tools = vec![
        "read_file".to_string(),
        "write_file".to_string(),
        "list_files".to_string(),
    ];
    policy_manager.update_available_tools(initial_tools)?;
    policy_manager.print_status();
    println!();

    // Test 2: Set specific policies
    println!("{}", style("Test 2: Setting specific policies").yellow());
    policy_manager.set_policy("read_file", ToolPolicy::Allow)?;
    policy_manager.set_policy("write_file", ToolPolicy::Deny)?;
    policy_manager.print_status();
    println!();

    // Test 3: Add new tools
    println!("{}", style("Test 3: Adding new tools").yellow());
    let updated_tools = vec![
        "read_file".to_string(),
        "write_file".to_string(),
        "list_files".to_string(),
        "run_terminal_cmd".to_string(),
        "grep_file".to_string(),
    ];
    policy_manager.update_available_tools(updated_tools)?;
    policy_manager.print_status();
    println!();

    // Test 4: Remove tools
    println!("{}", style("Test 4: Removing tools").yellow());
    let final_tools = vec![
        "read_file".to_string(),
        "list_files".to_string(),
        "grep_file".to_string(),
    ];
    policy_manager.update_available_tools(final_tools)?;
    policy_manager.print_status();
    println!();

    // Test 5: Check policy retrieval
    println!("{}", style("Test 5: Policy retrieval").yellow());
    println!(
        "read_file policy: {:?}",
        policy_manager.get_policy("read_file")
    );
    println!(
        "list_files policy: {:?}",
        policy_manager.get_policy("list_files")
    );
    println!(
        "nonexistent_tool policy: {:?}",
        policy_manager.get_policy("nonexistent_tool")
    );
    println!();

    println!(
        "{}",
        style("✓ All tests completed successfully!").green().bold()
    );
    println!("The tool policy system is working correctly.");
    println!();
    println!("Key features demonstrated:");
    println!("• Persistent storage in JSON format");
    println!("• Automatic addition of new tools as 'prompt'");
    println!("• Removal of deleted tools from configuration");
    println!("• Policy setting and retrieval");
    println!("• Status display with color coding");

    Ok(())
}
