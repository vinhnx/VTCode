//! Plugin validation and debugging tools for VT Code
//!
//! Provides utilities for validating plugin manifests, structures, and debugging
//! plugin functionality.

use std::path::Path;

use anyhow::Result;

use crate::command_safety::command_might_be_dangerous;
use crate::plugins::{PluginError, PluginManifest, PluginResult};
use crate::utils::file_utils::read_file_with_context;

/// Plugin validator
pub struct PluginValidator;

impl PluginValidator {
    /// Validate a plugin manifest
    pub fn validate_manifest(manifest: &PluginManifest) -> PluginResult<()> {
        // Validate required fields
        if manifest.name.is_empty() {
            return Err(PluginError::ManifestValidationError(
                "Plugin name is required".to_string(),
            ));
        }

        // Validate name format (kebab-case)
        if !Self::is_valid_plugin_name(&manifest.name) {
            return Err(PluginError::ManifestValidationError(
                "Plugin name must be in kebab-case (lowercase with hyphens)".to_string(),
            ));
        }

        // Validate version format if present
        if let Some(version) = &manifest.version
            && !Self::is_valid_version(version)
        {
            return Err(PluginError::ManifestValidationError(
                "Plugin version must follow semantic versioning (e.g., 1.0.0)".to_string(),
            ));
        }

        // Validate author if present
        if let Some(author) = &manifest.author
            && author.name.is_empty()
        {
            return Err(PluginError::ManifestValidationError(
                "Plugin author name is required when author is specified".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate a plugin directory structure
    pub async fn validate_plugin_structure(plugin_path: &Path) -> PluginResult<()> {
        // Call the function from PluginTemplate
        crate::plugins::PluginTemplate::validate_plugin_structure(plugin_path).await
    }

    /// Check if a plugin name is valid (kebab-case)
    fn is_valid_plugin_name(name: &str) -> bool {
        // Check if name contains only lowercase letters, numbers, and hyphens
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.is_empty()
    }

    /// Check if a version string is valid semantic version
    fn is_valid_version(version: &str) -> bool {
        // Basic semantic version check (X.Y.Z format)
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() < 3 {
            return false;
        }

        // Check that each part is numeric
        parts.iter().all(|part| {
            // Handle pre-release versions (e.g., 1.0.0-beta.1)
            let clean_part = part.split('-').next().unwrap_or(part);
            clean_part.chars().all(|c| c.is_ascii_digit())
        })
    }

    /// Validate plugin security (check for dangerous patterns)
    pub fn validate_plugin_security(manifest: &PluginManifest) -> PluginResult<()> {
        // Check for potentially dangerous configurations
        if let Some(mcp_servers) = &manifest.mcp_servers {
            // If it's an inline configuration, validate the commands
            match mcp_servers {
                crate::plugins::manifest::McpServerConfig::Inline(servers) => {
                    for (name, server) in servers {
                        if Self::is_dangerous_command(&server.command) {
                            return Err(PluginError::LoadingError(format!(
                                "MCP server '{}' uses potentially dangerous command: {}",
                                name, server.command
                            )));
                        }
                    }
                }
                crate::plugins::manifest::McpServerConfig::Path(_) => {
                    // For path-based configs, we can't validate until loaded
                }
            }
        }

        Ok(())
    }

    /// Check if a command is potentially dangerous
    fn is_dangerous_command(command: &str) -> bool {
        command_might_be_dangerous(&[command.to_string()])
    }
}

/// Plugin debugging utilities
pub struct PluginDebugger;

impl PluginDebugger {
    /// Print detailed information about a plugin manifest
    pub fn debug_manifest(manifest: &PluginManifest) -> String {
        let mut output = String::new();

        output.push_str(&format!("Plugin: {}\n", manifest.name));
        output.push_str(&format!(
            "  Version: {}\n",
            manifest.version.as_deref().unwrap_or("not specified")
        ));
        output.push_str(&format!(
            "  Description: {}\n",
            manifest.description.as_deref().unwrap_or("not specified")
        ));

        if let Some(author) = &manifest.author {
            output.push_str(&format!("  Author: {}\n", author.name));
            if let Some(email) = &author.email {
                output.push_str(&format!("    Email: {}\n", email));
            }
            if let Some(url) = &author.url {
                output.push_str(&format!("    URL: {}\n", url));
            }
        }

        if let Some(homepage) = &manifest.homepage {
            output.push_str(&format!("  Homepage: {}\n", homepage));
        }

        if let Some(repository) = &manifest.repository {
            output.push_str(&format!("  Repository: {}\n", repository));
        }

        if let Some(license) = &manifest.license {
            output.push_str(&format!("  License: {}\n", license));
        }

        if let Some(keywords) = &manifest.keywords {
            output.push_str(&format!("  Keywords: {}\n", keywords.join(", ")));
        }

        // Component counts
        let commands_count = manifest.commands.as_ref().map_or(0, |c| c.len());
        let agents_count = manifest.agents.as_ref().map_or(0, |a| a.len());
        let skills_count = manifest.skills.as_ref().map_or(0, |s| s.len());

        output.push_str("  Components:\n");
        output.push_str(&format!("    Commands: {}\n", commands_count));
        output.push_str(&format!("    Agents: {}\n", agents_count));
        output.push_str(&format!("    Skills: {}\n", skills_count));
        output.push_str(&format!(
            "    Hooks: {}\n",
            if manifest.hooks.is_some() {
                "yes"
            } else {
                "no"
            }
        ));
        output.push_str(&format!(
            "    MCP Servers: {}\n",
            if manifest.mcp_servers.is_some() {
                "yes"
            } else {
                "no"
            }
        ));
        output.push_str(&format!(
            "    LSP Servers: {}\n",
            if manifest.lsp_servers.is_some() {
                "yes"
            } else {
                "no"
            }
        ));

        output
    }

    /// Validate and debug a plugin manifest with detailed output
    pub fn validate_and_debug_manifest(manifest: &PluginManifest) -> Result<String> {
        let mut issues = Vec::new();

        // Run validation checks
        if let Err(e) = PluginValidator::validate_manifest(manifest) {
            issues.push(format!("Validation error: {}", e));
        }

        if let Err(e) = PluginValidator::validate_plugin_security(manifest) {
            issues.push(format!("Security warning: {}", e));
        }

        // Create debug output
        let mut output = Self::debug_manifest(manifest);

        if !issues.is_empty() {
            output.push_str("\nIssues found:\n");
            for issue in issues {
                output.push_str(&format!("  - {}\n", issue));
            }
        } else {
            output.push_str("\nNo issues found.\n");
        }

        Ok(output)
    }
}

/// Plugin validation CLI command handler
pub async fn handle_plugin_validate(path: &std::path::Path) -> Result<()> {
    // Check if path exists
    if !path.exists() {
        anyhow::bail!("Plugin path does not exist: {}", path.display());
    }

    // Try to load the manifest
    let manifest_path = path.join(".vtcode-plugin/plugin.json");
    if !manifest_path.exists() {
        anyhow::bail!("Plugin manifest not found at: {}", manifest_path.display());
    }

    let manifest_content = read_file_with_context(&manifest_path, "plugin manifest").await?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_content)?;

    // Validate the manifest
    let validation_output = PluginDebugger::validate_and_debug_manifest(&manifest)?;
    println!("{}", validation_output);

    Ok(())
}
