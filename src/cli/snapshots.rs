use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};
use vtcode_core::utils::colors::style;

pub async fn handle_snapshots_command(config: &CoreAgentConfig) -> Result<()> {
    println!("{}", style("Available Snapshots").blue().bold());
    let mut snapshot_cfg = SnapshotConfig::new(config.workspace.clone());
    snapshot_cfg.enabled = true;
    snapshot_cfg.storage_dir = config.checkpointing_storage_dir.clone();
    snapshot_cfg.max_snapshots = config.checkpointing_max_snapshots;
    snapshot_cfg.max_age_days = config.checkpointing_max_age_days;

    let manager =
        SnapshotManager::new(snapshot_cfg).context("failed to initialize checkpoint manager")?;
    let snaps = manager.list_snapshots().await?;
    if snaps.is_empty() {
        println!("(none)");
    } else {
        for s in snaps {
            let created = DateTime::<Utc>::from_timestamp(s.created_at as i64, 0)
                .map(|dt| dt.with_timezone(&Local))
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| s.created_at.to_string());
            println!(
                "- turn {turn}  messages={messages}  files={files}  created={created}  note={note}",
                turn = s.turn_number,
                messages = s.message_count,
                files = s.file_count,
                created = created,
                note = s.description,
            );
        }
    }
    Ok(())
}

pub async fn handle_cleanup_snapshots_command(
    config: &CoreAgentConfig,
    max: Option<usize>,
) -> Result<()> {
    println!("{}", style("Cleanup Snapshots").blue().bold());
    let mut cfg = SnapshotConfig::new(config.workspace.clone());
    cfg.enabled = true;
    cfg.storage_dir = config.checkpointing_storage_dir.clone();
    cfg.max_snapshots = config.checkpointing_max_snapshots;
    cfg.max_age_days = config.checkpointing_max_age_days;
    if let Some(m) = max {
        cfg.max_snapshots = m;
        println!("Keeping maximum {} snapshots...", m);
    }
    let manager = SnapshotManager::new(cfg).context("failed to initialize checkpoint manager")?;
    manager.cleanup_old_snapshots().await?;
    println!("Cleanup complete.");
    Ok(())
}
