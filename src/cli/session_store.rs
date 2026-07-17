//! `vtcode session-store` — operate the unified per-session state store.

use std::path::PathBuf;

use anyhow::{Context, Result};
use vtcode_core::cli::args::SessionStoreCommand;
use vtcode_session_store::{
    RetentionPolicy, apply_retention, migrate_legacy, open, query_facts, recent_sessions,
};

/// Handle the `session-store` CLI subcommand.
pub async fn handle_session_store_command(command: SessionStoreCommand) -> Result<()> {
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match command {
        SessionStoreCommand::Migrate { remove_legacy } => {
            let report = migrate_legacy(&workspace, remove_legacy)
                .context("failed to migrate legacy session stores")?;
            println!(
                "Migrated {} sessions ({} memory envelopes, {} trajectories, {} bytes).",
                report.sessions_created,
                report.memory_imported,
                report.trajectory_imported,
                report.bytes_migrated
            );
            if remove_legacy {
                println!("Removed legacy history/ and logs/ directories.");
            }
        }
        SessionStoreCommand::Gc { max_sessions, max_age_days } => {
            let removed =
                apply_retention(&workspace, RetentionPolicy { max_sessions, max_age_days })
                    .context("failed to apply retention")?;
            println!("Garbage-collected {removed} session(s).");
        }
        SessionStoreCommand::List { limit } => {
            let sessions = recent_sessions(&workspace, limit);
            if sessions.is_empty() {
                println!("No sessions found under .vtcode/sessions/.");
                return Ok(());
            }
            for s in &sessions {
                println!(
                    "{:<28} turns={:<4} events={:<6} {}  {}",
                    s.session_id, s.turn_count, s.event_count, s.status, s.updated_at
                );
            }
            println!("{} session(s).", sessions.len());
        }
        SessionStoreCommand::Inspect { session } => {
            let log = open(&workspace, &session).context("failed to open session")?;
            let manifest = log.manifest();
            println!("session_id:   {}", manifest.session_id);
            println!("status:       {}", manifest.status);
            println!("turns:        {}", manifest.turn_count);
            println!("events:       {}", manifest.event_count);
            println!("created_at:   {}", manifest.created_at);
            println!("updated_at:   {}", manifest.updated_at);
        }
        SessionStoreCommand::Facts { limit } => {
            let facts = query_facts(&workspace, limit).context("failed to query facts")?;
            if facts.is_empty() {
                println!("No grounded facts found across sessions.");
                return Ok(());
            }
            for f in &facts {
                println!("- [{}] {}", f.session_id, f.fact);
            }
            println!("{} fact(s).", facts.len());
        }
    }
    Ok(())
}
