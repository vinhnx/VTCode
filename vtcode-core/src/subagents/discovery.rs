use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use vtcode_config::{DiscoveredSubagents, SubagentDiscoveryInput, discover_subagents};

use crate::plugins::components::AgentsHandler;
use crate::plugins::manifest::PluginManifest;

// ─── Subagent Discovery ────────────────────────────────────────────────────

pub(crate) async fn discover_controller_subagents(
    workspace_root: &Path,
) -> Result<DiscoveredSubagents> {
    let plugin_agent_files = discover_plugin_agent_files(workspace_root).await?;
    discover_subagents(&SubagentDiscoveryInput {
        workspace_root: workspace_root.to_path_buf(),
        cli_agents: None,
        plugin_agent_files,
    })
}

async fn discover_plugin_agent_files(workspace_root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for plugin_root in trusted_plugin_roots(workspace_root) {
        if !plugin_root.exists() || !plugin_root.is_dir() {
            continue;
        }

        for entry in std::fs::read_dir(&plugin_root)
            .with_context(|| format!("Failed to read plugin directory {}", plugin_root.display()))?
        {
            let path = entry?.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join(".vtcode-plugin/plugin.json");
            if !manifest_path.exists() {
                continue;
            }

            let manifest: PluginManifest =
                serde_json::from_str(&std::fs::read_to_string(&manifest_path).with_context(
                    || format!("Failed to read plugin manifest {}", manifest_path.display()),
                )?)
                .with_context(|| {
                    format!(
                        "Failed to parse plugin manifest {}",
                        manifest_path.display()
                    )
                })?;
            for agent_path in AgentsHandler::process_agents(&path, manifest.agents.clone()).await? {
                files.push((manifest.name.clone(), agent_path));
            }
        }
    }
    Ok(files)
}

fn trusted_plugin_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(4);
    if let Some(codex_home) = std::env::var_os("CODEX_HOME").map(PathBuf::from) {
        roots.push(codex_home.join("plugins"));
    } else if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".vtcode/plugins"));
    }
    roots.push(workspace_root.join(".vtcode/plugins"));
    roots.push(workspace_root.join(".agents/plugins"));
    roots
}
