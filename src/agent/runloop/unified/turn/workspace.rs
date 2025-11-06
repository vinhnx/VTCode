use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use tokio::task;
use vtcode_core::SimpleIndexer;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

use crate::agent::runloop::apply_runtime_overrides;

pub(crate) async fn load_workspace_files(workspace: PathBuf) -> Result<Vec<String>> {
    task::spawn_blocking(move || -> Result<Vec<String>> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;

        // Get all indexed files efficiently without regex overhead
        let files = indexer.all_files();

        Ok(files)
    })
    .await
    .map_err(|err| anyhow!("failed to join file loading task: {}", err))?
}

pub(crate) async fn bootstrap_config_files(workspace: PathBuf, force: bool) -> Result<Vec<String>> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || VTCodeConfig::bootstrap_project(&workspace, force))
        .await
        .map_err(|err| anyhow!("failed to join configuration bootstrap task: {}", err))?;
    result.with_context(|| format!("failed to initialize configuration in {}", label))
}

pub(crate) async fn build_workspace_index(workspace: PathBuf) -> Result<()> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || -> Result<()> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("failed to join workspace indexing task: {}", err))?;
    result.with_context(|| format!("failed to build workspace index in {}", label))
}

async fn load_workspace_config_snapshot(workspace: &Path) -> Result<VTCodeConfig> {
    let workspace_buf = workspace.to_path_buf();
    let label = workspace_buf.display().to_string();
    let result = task::spawn_blocking(move || {
        ConfigManager::load_from_workspace(&workspace_buf).map(|manager| manager.config().clone())
    })
    .await
    .map_err(|err| anyhow!("failed to join workspace config load task: {}", err))?;

    result.with_context(|| format!("failed to load configuration for {}", label))
}

pub(crate) async fn refresh_vt_config(
    workspace: &Path,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
) -> Result<()> {
    let mut snapshot = load_workspace_config_snapshot(workspace).await?;
    apply_runtime_overrides(Some(&mut snapshot), runtime_cfg);
    *vt_cfg = Some(snapshot);
    Ok(())
}
