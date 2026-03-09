use anyhow::Result;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{RevertScope, SnapshotManager};
use vtcode_core::utils::colors::style;

use super::checkpoints::{snapshot_config, snapshot_manager};

pub async fn handle_revert_command(
    config: &CoreAgentConfig,
    turn: usize,
    partial: Option<String>,
) -> Result<()> {
    println!("{}\n", style("[REVERT]").cyan().bold());
    let scope = partial
        .as_deref()
        .and_then(SnapshotManager::parse_revert_scope)
        .unwrap_or(RevertScope::Both);
    let manager = snapshot_manager(snapshot_config(config))?;
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
