use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use tokio::task;
use vtcode_core::SimpleIndexer;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

use crate::agent::agents::apply_runtime_overrides;

async fn blocking_task<F, R>(label: &str, f: F) -> Result<R>
where
    F: FnOnce() -> Result<R> + Send + 'static,
    R: Send + 'static,
{
    task::spawn_blocking(f)
        .await
        .map_err(|err| anyhow!("failed to join {label} task: {err}"))?
}

pub(crate) async fn load_workspace_files(workspace: PathBuf) -> Result<Vec<String>> {
    task::spawn_blocking(move || {
        let indexer = SimpleIndexer::new(workspace.clone());
        indexer.discover_files(&workspace)
    })
    .await
    .map_err(|err| anyhow!("failed to join file loading task: {err}"))
}

pub(crate) async fn bootstrap_config_files(workspace: PathBuf, force: bool) -> Result<Vec<String>> {
    blocking_task("configuration bootstrap", move || {
        VTCodeConfig::bootstrap_project(&workspace, force)
    })
    .await
}

pub(crate) async fn build_workspace_index(workspace: PathBuf) -> Result<()> {
    blocking_task("workspace indexing", move || -> Result<()> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;
        Ok(())
    })
    .await
}

async fn load_workspace_config_snapshot(workspace: &Path) -> Result<VTCodeConfig> {
    let workspace_buf = workspace.to_path_buf();
    blocking_task("workspace config load", move || {
        ConfigManager::load_from_workspace(&workspace_buf).map(|manager| manager.config().clone())
    })
    .await
}

pub(crate) async fn refresh_vt_config(
    workspace: &Path,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
) -> Result<()> {
    let mut snapshot = load_workspace_config_snapshot(workspace).await?;
    apply_runtime_overrides(Some(&mut snapshot), runtime_cfg);
    vtcode_core::llm::factory::register_custom_providers(&snapshot.custom_providers);
    *vt_cfg = Some(snapshot);
    Ok(())
}

pub(crate) fn apply_workspace_config_to_registry(
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
    vt_cfg: &VTCodeConfig,
) -> Result<()> {
    tool_registry.apply_commands_config(&vt_cfg.commands);
    tool_registry.apply_permissions_config(&vt_cfg.permissions);
    tool_registry.apply_sandbox_config(&vt_cfg.sandbox);
    tool_registry.apply_timeout_policy(&vt_cfg.timeouts);
    Ok(())
}
