use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::utils::colors::style;

use super::checkpoints::{snapshot_config, snapshot_manager};

pub async fn handle_snapshots_command(config: &CoreAgentConfig) -> Result<()> {
    println!("{}\n", style("[SNAPSHOTS]").cyan().bold());
    let manager = snapshot_manager(snapshot_config(config))?;
    let snaps = manager.list_snapshots().await?;
    if snaps.is_empty() {
        println!("(none)");
    } else {
        for (i, s) in snaps.iter().enumerate() {
            let created = DateTime::<Utc>::from_timestamp(s.created_at as i64, 0)
                .map(|dt| dt.with_timezone(&Local))
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| s.created_at.to_string());
            println!(
                "  {:>2}. turn {:>3}  messages={:<4} files={:<3} created={}",
                i + 1,
                s.turn_number,
                s.message_count,
                s.file_count,
                created
            );
            if !s.description.is_empty() {
                println!("         note: {}", s.description);
            }
        }
    }
    Ok(())
}

pub async fn handle_cleanup_snapshots_command(
    config: &CoreAgentConfig,
    max: Option<usize>,
) -> Result<()> {
    println!("{}\n", style("[CLEANUP]").cyan().bold());
    let mut cfg = snapshot_config(config);
    if let Some(m) = max {
        cfg.max_snapshots = m;
        println!("Keeping maximum {} snapshots...", m);
    }
    let manager = snapshot_manager(cfg)?;
    manager.cleanup_old_snapshots().await?;
    println!("Cleanup complete.");
    Ok(())
}
