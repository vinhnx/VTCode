use anyhow::{Context, Result};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{RevertScope, SnapshotConfig, SnapshotManager};
use vtcode_core::utils::colors::style;

pub async fn handle_revert_command(
    config: &CoreAgentConfig,
    turn: usize,
    partial: Option<String>,
) -> Result<()> {
    println!("{}", style("Revert Agent State").blue().bold());
    let scope = partial
        .as_deref()
        .and_then(SnapshotManager::parse_revert_scope)
        .unwrap_or(RevertScope::Both);
    let mut snapshot_cfg = SnapshotConfig::new(config.workspace.clone());
    snapshot_cfg.enabled = true;
    snapshot_cfg.storage_dir = config.checkpointing_storage_dir.clone();
    snapshot_cfg.max_snapshots = config.checkpointing_max_snapshots;
    snapshot_cfg.max_age_days = config.checkpointing_max_age_days;

    let manager =
        SnapshotManager::new(snapshot_cfg).context("failed to initialize checkpoint manager")?;
    match manager.restore_snapshot(turn, scope).await? {
        Some(restored) => {
            if scope.includes_code() {
                println!("Applied code changes from checkpoint turn {}.", turn);
            } else {
                println!(
                    "Loaded checkpoint turn {} without applying code changes.",
                    turn
                );
            }
            if scope.includes_conversation() {
                println!(
                    "Conversation history has {} messages. Use /rewind in chat to review.",
                    restored.conversation.len()
                );
            }
        }
        None => {
            println!("Snapshot not found for turn {}", turn);
        }
    }
    Ok(())
}
