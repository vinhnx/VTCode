//! Plugin component handlers for VT Code
//!
//! This module handles the different types of components that plugins can provide:
//! - Commands (slash commands)
//! - Agents (specialized profiles)
//! - Skills (model-invoked capabilities)
//! - Hooks (event handlers)
//! - MCP servers (Model Context Protocol)

use std::path::{Path, PathBuf};

use anyhow::Result;
use tokio::fs;

use crate::plugins::PluginManifest;

/// Handler for plugin commands (slash commands)
pub struct CommandsHandler;

impl CommandsHandler {
    /// Process plugin commands from the plugin directory
    pub async fn process_commands(plugin_path: &Path, manifest_commands: Option<Vec<String>>) -> Result<Vec<PathBuf>> {
        let mut command_files = Vec::new();

        // Add commands from manifest paths
        if let Some(manifest_paths) = manifest_commands {
            for path in manifest_paths {
                let full_path = plugin_path.join(&path);
                if fs::metadata(&full_path)
                    .await
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    command_files.push(full_path);
                }
            }
        }

        // Also look for commands in the default commands/ directory
        let default_commands_dir = plugin_path.join("commands");
        if fs::metadata(&default_commands_dir)
            .await
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            let mut entries = fs::read_dir(&default_commands_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if fs::metadata(&path).await.map(|metadata| metadata.is_file()).unwrap_or(false)
                    && path.extension().is_some_and(|ext| ext == "md")
                {
                    command_files.push(path);
                }
            }
        }

        Ok(command_files)
    }
}

/// Handler for plugin agents.
pub struct AgentsHandler;

impl AgentsHandler {
    /// Process plugin agents from the plugin directory.
    ///
    /// Manifest entries must stay inside the plugin root: absolute paths and
    /// parent-directory escapes are skipped, and symlinks are never followed,
    /// so a crafted manifest cannot smuggle outside files into agent specs.
    pub async fn process_agents(plugin_path: &Path, manifest_agents: Option<Vec<String>>) -> Result<Vec<PathBuf>> {
        let mut agent_files = Vec::new();

        // Add agents from manifest paths
        if let Some(manifest_paths) = manifest_agents {
            for path in manifest_paths {
                let relative = Path::new(&path);
                if relative.is_absolute()
                    || relative
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    continue;
                }
                let full_path = plugin_path.join(relative);
                if fs::symlink_metadata(&full_path)
                    .await
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    agent_files.push(full_path);
                }
            }
        }

        // Also look for agents in the default agents/ directory
        let default_agents_dir = plugin_path.join("agents");
        if fs::symlink_metadata(&default_agents_dir)
            .await
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            let mut entries = fs::read_dir(&default_agents_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if fs::symlink_metadata(&path)
                    .await
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                    && path.extension().is_some_and(|ext| ext == "md")
                {
                    agent_files.push(path);
                }
            }
        }

        Ok(agent_files)
    }
}

/// Handler for plugin skills
pub struct SkillsHandler;

impl SkillsHandler {
    /// Process plugin skills from the plugin directory
    pub async fn process_skills(plugin_path: &Path, manifest_skills: Option<Vec<String>>) -> Result<Vec<PathBuf>> {
        let mut skill_dirs = Vec::new();

        // Add skills from manifest paths
        if let Some(manifest_paths) = manifest_skills {
            for path in manifest_paths {
                let full_path = plugin_path.join(&path);
                if fs::metadata(&full_path)
                    .await
                    .map(|metadata| metadata.is_dir())
                    .unwrap_or(false)
                {
                    skill_dirs.push(full_path);
                }
            }
        }

        // Also look for skills in the default skills/ directory
        let default_skills_dir = plugin_path.join("skills");
        if fs::metadata(&default_skills_dir)
            .await
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            let mut entries = fs::read_dir(&default_skills_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if fs::metadata(&path).await.map(|metadata| metadata.is_dir()).unwrap_or(false) {
                    // Check if it contains a SKILL.md file
                    let skill_md = path.join("SKILL.md");
                    if fs::metadata(&skill_md)
                        .await
                        .map(|metadata| metadata.is_file())
                        .unwrap_or(false)
                    {
                        skill_dirs.push(path);
                    }
                }
            }
        }

        Ok(skill_dirs)
    }
}

/// Handler for plugin hooks
pub struct HooksHandler;

impl HooksHandler {
    /// Process plugin hooks from the plugin directory
    pub async fn process_hooks(
        plugin_path: &Path,
        manifest_hooks: Option<serde_json::Value>,
    ) -> Result<Option<PathBuf>> {
        // Check for hooks in manifest
        if let Some(hooks_config) = manifest_hooks {
            // If hooks config is a string, treat it as a path
            if let Some(path_str) = hooks_config.as_str() {
                let hooks_path = plugin_path.join(path_str);
                if fs::metadata(&hooks_path)
                    .await
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    return Ok(Some(hooks_path));
                }
            }
        }

        // Look for hooks in the default hooks/ directory
        let default_hooks_path = plugin_path.join("hooks/hooks.json");
        if fs::metadata(&default_hooks_path)
            .await
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
        {
            return Ok(Some(default_hooks_path));
        }

        Ok(None)
    }
}

/// Handler for plugin MCP servers
pub struct McpServersHandler;

impl McpServersHandler {
    /// Process plugin MCP servers from the plugin directory
    pub async fn process_mcp_servers(
        plugin_path: &Path,
        manifest_mcp: Option<serde_json::Value>,
    ) -> Result<Option<PathBuf>> {
        // Check for MCP config in manifest
        if let Some(mcp_config) = manifest_mcp {
            // If MCP config is a string, treat it as a path
            if let Some(path_str) = mcp_config.as_str() {
                let mcp_path = plugin_path.join(path_str);
                if fs::metadata(&mcp_path)
                    .await
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    return Ok(Some(mcp_path));
                }
            }
        }

        // Look for MCP config in the default .mcp.json file
        let default_mcp_path = plugin_path.join(".mcp.json");
        if fs::metadata(&default_mcp_path)
            .await
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
        {
            return Ok(Some(default_mcp_path));
        }

        Ok(None)
    }
}

/// Handler for plugin LSP servers
pub struct LspServersHandler;

impl LspServersHandler {
    /// Process plugin LSP servers from the plugin directory
    pub async fn process_lsp_servers(
        plugin_path: &Path,
        manifest_lsp: Option<serde_json::Value>,
    ) -> Result<Option<PathBuf>> {
        // Check for LSP config in manifest
        if let Some(lsp_config) = manifest_lsp {
            // If LSP config is a string, treat it as a path
            if let Some(path_str) = lsp_config.as_str() {
                let lsp_path = plugin_path.join(path_str);
                if fs::metadata(&lsp_path)
                    .await
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    return Ok(Some(lsp_path));
                }
            }
        }

        // Look for LSP config in the default .lsp.json file
        let default_lsp_path = plugin_path.join(".lsp.json");
        if fs::metadata(&default_lsp_path)
            .await
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
        {
            return Ok(Some(default_lsp_path));
        }

        Ok(None)
    }
}

/// A comprehensive handler that processes all plugin components
pub struct PluginComponentsHandler;

impl PluginComponentsHandler {
    /// Process all components for a plugin
    pub async fn process_all_components<P: AsRef<Path>>(
        plugin_path: P,
        manifest: &PluginManifest,
    ) -> Result<PluginComponents> {
        let path_buf = plugin_path.as_ref().to_path_buf();
        let commands = CommandsHandler::process_commands(&path_buf, manifest.commands.clone()).await?;

        let agents = AgentsHandler::process_agents(&path_buf, manifest.agents.clone()).await?;

        let skills = SkillsHandler::process_skills(&path_buf, manifest.skills.clone()).await?;

        let hooks = HooksHandler::process_hooks(
            &path_buf,
            manifest.hooks.as_ref().map(|h| match h {
                crate::plugins::manifest::HookConfig::Path(path) => serde_json::Value::String(path.clone()),
                crate::plugins::manifest::HookConfig::Inline(_) => serde_json::Value::Null, // For inline, we'll handle separately
            }),
        )
        .await?;

        let mcp_servers = McpServersHandler::process_mcp_servers(
            &path_buf,
            manifest.mcp_servers.as_ref().map(|m| match m {
                crate::plugins::manifest::McpServerConfig::Path(path) => serde_json::Value::String(path.clone()),
                crate::plugins::manifest::McpServerConfig::Inline(_) => serde_json::Value::Null, // For inline, we'll handle separately
            }),
        )
        .await?;

        let lsp_servers = LspServersHandler::process_lsp_servers(
            &path_buf,
            manifest.lsp_servers.as_ref().map(|l| match l {
                crate::plugins::manifest::LspServerConfig::Path(path) => serde_json::Value::String(path.clone()),
                crate::plugins::manifest::LspServerConfig::Inline(_) => serde_json::Value::Null, // For inline, we'll handle separately
            }),
        )
        .await?;

        Ok(PluginComponents {
            commands,
            agents,
            skills,
            hooks,
            mcp_servers,
            lsp_servers,
        })
    }
}

/// Structure containing all plugin components
pub struct PluginComponents {
    pub commands: Vec<PathBuf>,
    pub agents: Vec<PathBuf>,
    pub skills: Vec<PathBuf>,
    pub hooks: Option<PathBuf>,
    pub mcp_servers: Option<PathBuf>,
    pub lsp_servers: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Plugin root at `<dir>/plugin` with a sibling file outside it, so
    /// `../sibling.md` escapes the root while staying inside the temp dir.
    async fn plugin_fixture() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().join("plugin");
        fs::create_dir_all(root.join("agents")).await.expect("agents dir");
        fs::create_dir_all(root.join("nested")).await.expect("nested dir");
        fs::write(root.join("agents").join("ok.md"), "ok").await.expect("write ok");
        fs::write(root.join("agents").join("notes.txt"), "not markdown")
            .await
            .expect("write txt");
        fs::write(root.join("nested").join("custom.md"), "custom")
            .await
            .expect("write custom");
        fs::write(dir.path().join("sibling.md"), "outside")
            .await
            .expect("write sibling");
        (dir, root)
    }

    #[tokio::test]
    async fn manifest_agent_paths_stay_contained() {
        let (_dir, root) = plugin_fixture().await;
        let outside = _dir.path().join("sibling.md");

        let files = AgentsHandler::process_agents(
            &root,
            Some(vec![
                "nested/custom.md".to_string(),
                "../sibling.md".to_string(),
                outside.to_string_lossy().into_owned(),
                "missing.md".to_string(),
            ]),
        )
        .await
        .expect("process");

        assert!(files.iter().all(|p| p.starts_with(&root)), "all results stay under the plugin root: {files:?}");
        assert!(files.contains(&root.join("nested/custom.md")), "legit nested entry kept: {files:?}");
        assert!(files.contains(&root.join("agents/ok.md")), "default dir scan kept: {files:?}");
        assert!(
            !files.iter().any(|p| p.ends_with("sibling.md")),
            "traversal and absolute entries dropped: {files:?}"
        );
        assert!(!files.iter().any(|p| p.ends_with("notes.txt")), "non-markdown default files skipped: {files:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn manifest_agent_symlinks_are_skipped() {
        let (_dir, root) = plugin_fixture().await;
        let outside = _dir.path().join("sibling.md");
        std::os::unix::fs::symlink(&outside, root.join("agents").join("link.md")).expect("symlink");

        let files = AgentsHandler::process_agents(&root, Some(vec!["agents/link.md".to_string()]))
            .await
            .expect("process");

        assert!(
            !files.iter().any(|p| p.ends_with("link.md")),
            "symlinked entries must not escape via following: {files:?}"
        );
        assert!(files.contains(&root.join("agents/ok.md")), "regular files still collected: {files:?}");
    }
}
