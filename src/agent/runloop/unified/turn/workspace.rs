use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use tokio::task;
use vtcode_core::SimpleIndexer;

pub(super) async fn load_workspace_files(workspace: PathBuf) -> Result<Vec<String>> {
    task::spawn_blocking(move || -> Result<Vec<String>> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;

        Ok(indexer.all_files())
    })
    .await
    .map_err(|err| anyhow!("failed to join file loading task: {}", err))?
}

pub(super) async fn build_workspace_index(workspace: PathBuf) -> Result<()> {
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
