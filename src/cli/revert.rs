use anyhow::Result;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{RevertScope, SnapshotManager};
use vtcode_core::utils::colors::style;

use super::checkpoints::{snapshot_config, snapshot_manager};

pub async fn handle_revert_command(
    config: &CoreAgentConfig,
    turn: usize,
    partial: Option<String>,
    session: Option<String>,
) -> Result<()> {
    println!("{}\n", style("[REVERT]").cyan().bold());
    let scope = partial
        .as_deref()
        .and_then(SnapshotManager::parse_revert_scope)
        .unwrap_or(RevertScope::Both);
    let manager = snapshot_manager(snapshot_config(config))?;
    let restored = match session.as_deref() {
        Some(session) => manager.restore_snapshot_for_session(session, turn, scope).await?,
        None => manager.restore_snapshot(turn, scope).await?,
    };
    match restored {
        Some(restored) => {
            if let Some(owner) = restored.metadata.session_id.as_deref() {
                println!("Checkpoint turn {turn} belongs to session {owner}.");
            }
            if scope.includes_code() {
                println!("Applied code changes from checkpoint turn {turn}.");
                if let Some(skipped) = restored.metadata.skipped_paths.as_ref().filter(|s| !s.is_empty()) {
                    println!("Warning: this checkpoint skipped {} — the restore may be incomplete.", skipped.summary());
                }
            } else {
                println!("Loaded checkpoint turn {turn} without applying code changes.");
            }
            if scope.includes_conversation() {
                println!(
                    "Conversation history has {} messages. Use /rewind in chat to review.",
                    restored.conversation.len()
                );
            }
        }
        None => {
            println!("Snapshot not found for turn {turn}");
        }
    }
    Ok(())
}
