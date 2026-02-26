//! Plugin template and directory structure utilities for VT Code
//!
//! Provides utilities for creating and validating plugin directory structures
//! according to VT Code's plugin system specification.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::plugins::{PluginError, PluginManifest, PluginResult};

/// Plugin template generator
pub struct PluginTemplate;

impl PluginTemplate {
    /// Create a new plugin with the standard directory structure
    pub async fn create_plugin_skeleton(
        plugin_dir: &Path,
        manifest: &PluginManifest,
    ) -> PluginResult<()> {
        // Create the plugin directory
        fs::create_dir_all(plugin_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to create plugin directory: {}", e))
        })?;

        // Create the .vtcode-plugin directory for the manifest
        let vtcode_plugin_dir = plugin_dir.join(".vtcode-plugin");
        fs::create_dir_all(&vtcode_plugin_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to create .vtcode-plugin directory: {}", e))
        })?;

        // Write the plugin manifest
        let manifest_path = vtcode_plugin_dir.join("plugin.json");
        let manifest_json =
            serde_json::to_string_pretty(manifest).map_err(PluginError::JsonError)?;
        fs::write(&manifest_path, &manifest_json)
            .await
            .map_err(|e| {
                PluginError::LoadingError(format!("Failed to write plugin manifest: {}", e))
            })?;

        // Create standard directories if specified in manifest
        Self::create_standard_directories(plugin_dir, manifest).await?;

        // Create example files
        Self::create_example_files(plugin_dir, manifest).await?;

        Ok(())
    }

    /// Create standard plugin directories
    async fn create_standard_directories(
        plugin_dir: &Path,
        manifest: &PluginManifest,
    ) -> PluginResult<()> {
        // Create commands directory
        if manifest.commands.is_some() || Self::should_create_default_commands_dir(manifest) {
            let commands_dir = plugin_dir.join("commands");
            fs::create_dir_all(&commands_dir).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to create commands directory: {}", e))
            })?;
        }

        // Create agents directory
        if manifest.agents.is_some() || Self::should_create_default_agents_dir(manifest) {
            let agents_dir = plugin_dir.join("agents");
            fs::create_dir_all(&agents_dir).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to create agents directory: {}", e))
            })?;
        }

        // Create skills directory
        if manifest.skills.is_some() || Self::should_create_default_skills_dir(manifest) {
            let skills_dir = plugin_dir.join("skills");
            fs::create_dir_all(&skills_dir).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to create skills directory: {}", e))
            })?;
        }

        // Create hooks directory
        if manifest.hooks.is_some() {
            let hooks_dir = plugin_dir.join("hooks");
            fs::create_dir_all(&hooks_dir).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to create hooks directory: {}", e))
            })?;
        }

        Ok(())
    }

    /// Create example files for the plugin
    async fn create_example_files(
        plugin_dir: &Path,
        manifest: &PluginManifest,
    ) -> PluginResult<()> {
        // Create an example command if commands directory exists
        let commands_dir = plugin_dir.join("commands");
        if commands_dir.exists() {
            let example_command = commands_dir.join("example.md");
            if !example_command.exists() {
                let example_content = format!(
                    r#"---
name: {plugin_name}-example
description: Example command for {plugin_name} plugin
parameters:
  - name: input
    type: string
    description: Example input parameter
---

# {plugin_name} Example Command

This is an example command for the {plugin_name} plugin.

## Usage

`/{plugin_name}-example <input>`

## Description

This command demonstrates how to create a plugin command for VT Code.
"#,
                    plugin_name = manifest.name
                );
                fs::write(&example_command, example_content)
                    .await
                    .map_err(|e| {
                        PluginError::LoadingError(format!(
                            "Failed to create example command: {}",
                            e
                        ))
                    })?;
            }
        }

        // Create an example agent if agents directory exists
        let agents_dir = plugin_dir.join("agents");
        if agents_dir.exists() {
            let example_agent = agents_dir.join("example.md");
            if !example_agent.exists() {
                let example_content = format!(
                    r#"---
description: Example agent for {plugin_name} plugin
capabilities: ["example-task", "demo-capability"]
---

# {plugin_name} Example Agent

This is an example agent for the {plugin_name} plugin.

## Capabilities
- Perform example tasks
- Demonstrate agent functionality

## Context and examples
This agent can be used to demonstrate how agents work in VT Code plugins.
"#,
                    plugin_name = manifest.name
                );
                fs::write(&example_agent, example_content)
                    .await
                    .map_err(|e| {
                        PluginError::LoadingError(format!("Failed to create example agent: {}", e))
                    })?;
            }
        }

        // Create an example skill if skills directory exists
        let skills_dir = plugin_dir.join("skills");
        if skills_dir.exists() {
            let example_skill_dir = skills_dir.join("example-skill");
            fs::create_dir_all(&example_skill_dir).await.map_err(|e| {
                PluginError::LoadingError(format!(
                    "Failed to create example skill directory: {}",
                    e
                ))
            })?;

            let skill_md = example_skill_dir.join("SKILL.md");
            if !skill_md.exists() {
                let example_content = format!(
                    r#"---
name: {plugin_name}-example-skill
description: Example skill for {plugin_name} plugin
parameters:
  - name: input
    type: string
    description: Example input parameter
---

# {plugin_name} Example Skill

This is an example skill for the {plugin_name} plugin.

## Purpose

This skill demonstrates how to create a model-invoked capability in VT Code.
"#,
                    plugin_name = manifest.name
                );
                fs::write(&skill_md, example_content).await.map_err(|e| {
                    PluginError::LoadingError(format!("Failed to create example skill: {}", e))
                })?;
            }
        }

        // Create an example hooks config if hooks directory exists
        let hooks_dir = plugin_dir.join("hooks");
        if hooks_dir.exists() {
            let hooks_config = hooks_dir.join("hooks.json");
            if !hooks_config.exists() {
                let example_content = r#"{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "${VTCODE_PLUGIN_ROOT}/scripts/post-edit.sh"
          }
        ]
      }
    ]
  }
}"#;
                fs::write(&hooks_config, example_content)
                    .await
                    .map_err(|e| {
                        PluginError::LoadingError(format!(
                            "Failed to create example hooks config: {}",
                            e
                        ))
                    })?;
            }
        }

        // Create an example MCP config if MCP servers are specified
        if manifest.mcp_servers.is_some() {
            let mcp_config = plugin_dir.join(".mcp.json");
            if !mcp_config.exists() {
                let example_content = r#"{
  "example-server": {
    "command": "node",
    "args": ["${VTCODE_PLUGIN_ROOT}/mcp-server.js"],
    "env": {
      "PLUGIN_ROOT": "${VTCODE_PLUGIN_ROOT}"
    }
  }
}"#;
                fs::write(&mcp_config, example_content).await.map_err(|e| {
                    PluginError::LoadingError(format!("Failed to create example MCP config: {}", e))
                })?;
            }
        }

        // Create an example LSP config if LSP servers are specified
        if manifest.lsp_servers.is_some() {
            let lsp_config = plugin_dir.join(".lsp.json");
            if !lsp_config.exists() {
                let example_content = r#"{
  "example-lsp": {
    "command": "example-lsp-server",
    "args": ["--stdio"],
    "extensionToLanguage": {
      ".example": "example"
    }
  }
}"#;
                fs::write(&lsp_config, example_content).await.map_err(|e| {
                    PluginError::LoadingError(format!("Failed to create example LSP config: {}", e))
                })?;
            }
        }

        Ok(())
    }

    /// Determine if commands directory should be created by default
    fn should_create_default_commands_dir(manifest: &PluginManifest) -> bool {
        // Create default commands directory if no explicit commands are specified
        // but the plugin might benefit from having commands
        manifest.commands.is_none()
    }

    /// Determine if agents directory should be created by default
    fn should_create_default_agents_dir(manifest: &PluginManifest) -> bool {
        // Create default agents directory if no explicit agents are specified
        // but the plugin might benefit from having agents
        manifest.agents.is_none()
    }

    /// Determine if skills directory should be created by default
    fn should_create_default_skills_dir(manifest: &PluginManifest) -> bool {
        // Create default skills directory if no explicit skills are specified
        // but the plugin might benefit from having skills
        manifest.skills.is_none()
    }

    /// Validate plugin directory structure
    pub async fn validate_plugin_structure(plugin_dir: &Path) -> PluginResult<()> {
        // Check if plugin directory exists
        if !plugin_dir.exists() {
            return Err(PluginError::LoadingError(format!(
                "Plugin directory does not exist: {}",
                plugin_dir.display()
            )));
        }

        // Check if manifest exists
        let manifest_path = plugin_dir.join(".vtcode-plugin/plugin.json");
        if !manifest_path.exists() {
            return Err(PluginError::ManifestValidationError(format!(
                "Plugin manifest not found at: {}",
                manifest_path.display()
            )));
        }

        // Validate manifest can be parsed
        let manifest_content = fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| PluginError::LoadingError(format!("Failed to read manifest: {}", e)))?;

        let _manifest: PluginManifest = serde_json::from_str(&manifest_content).map_err(|e| {
            PluginError::ManifestValidationError(format!("Invalid manifest JSON: {}", e))
        })?;

        Ok(())
    }
}

/// Plugin directory utilities
pub struct PluginDirectory;

impl PluginDirectory {
    /// Get the standard plugin directory structure
    pub fn get_standard_structure() -> HashMap<&'static str, &'static str> {
        let mut structure = HashMap::new();
        structure.insert(".vtcode-plugin/", "Plugin manifest directory (required)");
        structure.insert("commands/", "Slash command Markdown files");
        structure.insert("agents/", "Subagent Markdown files");
        structure.insert("skills/", "Agent Skills with SKILL.md files");
        structure.insert("hooks/", "Hook configurations");
        structure.insert("scripts/", "Hook and utility scripts");
        structure.insert("LICENSE", "License file");
        structure.insert("CHANGELOG.md", "Version history");
        structure.insert(".mcp.json", "MCP server definitions");
        structure.insert(".lsp.json", "LSP server configurations");
        structure
    }

    /// Create a plugin from a template
    pub async fn create_from_template(
        base_dir: &Path,
        plugin_name: &str,
        description: &str,
    ) -> PluginResult<PathBuf> {
        let plugin_dir = base_dir.join(plugin_name);

        let manifest = PluginManifest {
            name: plugin_name.to_string(),
            version: Some("1.0.0".to_string()),
            description: Some(description.to_string()),
            author: None,
            homepage: None,
            repository: None,
            license: Some("MIT".to_string()),
            keywords: Some(vec!["vtcode".to_string(), "plugin".to_string()]),
            commands: None,
            agents: None,
            skills: None,
            hooks: None,
            mcp_servers: None,
            output_styles: None,
            lsp_servers: None,
        };

        PluginTemplate::create_plugin_skeleton(&plugin_dir, &manifest).await?;
        Ok(plugin_dir)
    }
}
