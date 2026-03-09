use anyhow::{Context, Result};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};

pub(super) fn snapshot_config(config: &CoreAgentConfig) -> SnapshotConfig {
    let mut snapshot_cfg = SnapshotConfig::new(config.workspace.clone());
    snapshot_cfg.enabled = true;
    snapshot_cfg.storage_dir = config.checkpointing_storage_dir.clone();
    snapshot_cfg.max_snapshots = config.checkpointing_max_snapshots;
    snapshot_cfg.max_age_days = config.checkpointing_max_age_days;
    snapshot_cfg
}

pub(super) fn snapshot_manager(snapshot_cfg: SnapshotConfig) -> Result<SnapshotManager> {
    SnapshotManager::new(snapshot_cfg).context("failed to initialize checkpoint manager")
}
