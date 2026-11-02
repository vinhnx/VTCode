//! `vtcode session-store` — operate the unified per-session state store.

use std::path::PathBuf;

use anyhow::{Context, Result};
use vtcode_core::cli::args::SessionStoreCommand;
use vtcode_memory::{
    DEFAULT_MAX_EVENTS, RetentionPolicy, SessionSummary, apply_retention, live_sessions, migrate_legacy, open,
    query_facts, read_audit_pack, recent_sessions, verify_audit_pack, write_audit_pack,
};

/// One `session-store list` row. Live sessions (a process holds the session
/// lock) carry the `live` marker so users can see messaging peers at a glance.
fn format_session_list_row(summary: &SessionSummary, live: bool) -> String {
    format!(
        "{:<28} turns={:<4} events={:<6} {}  {}  {}",
        summary.session_id,
        summary.turn_count,
        summary.event_count,
        summary.status,
        summary.updated_at,
        if live { "live" } else { "" },
    )
}

/// Handle the `session-store` CLI subcommand.
pub async fn handle_session_store_command(command: SessionStoreCommand) -> Result<()> {
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match command {
        SessionStoreCommand::Migrate { remove_legacy } => {
            let report =
                migrate_legacy(&workspace, remove_legacy).context("failed to migrate legacy session stores")?;
            println!(
                "Migrated {} sessions ({} memory envelopes, {} trajectories, {} bytes).",
                report.sessions_created, report.memory_imported, report.trajectory_imported, report.bytes_migrated
            );
            if remove_legacy {
                println!("Removed legacy history/ and logs/ directories.");
            }
        }
        SessionStoreCommand::Gc { max_sessions, max_age_days } => {
            let removed = apply_retention(&workspace, RetentionPolicy { max_sessions, max_age_days })
                .context("failed to apply retention")?;
            println!("Garbage-collected {removed} session(s).");
        }
        SessionStoreCommand::List { limit } => {
            let sessions = recent_sessions(&workspace, limit);
            if sessions.is_empty() {
                println!("No sessions found under .vtcode/sessions/.");
                return Ok(());
            }
            let live_summaries = live_sessions(&workspace);
            let live: std::collections::HashSet<&str> = live_summaries.iter().map(|s| s.session_id.as_str()).collect();
            for s in &sessions {
                println!("{}", format_session_list_row(s, live.contains(s.session_id.as_str())));
            }
            println!("{} session(s).", sessions.len());
        }
        SessionStoreCommand::Inspect { session } => {
            let log = open(&workspace, &session, DEFAULT_MAX_EVENTS).context("failed to open session")?;
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
        SessionStoreCommand::Pack { session, output, verify } => {
            if let Some(pack_path) = verify {
                let pack = read_audit_pack(&pack_path)
                    .with_context(|| format!("failed to read audit pack {}", pack_path.display()))?;
                let report = verify_audit_pack(&workspace, &session, &pack).context("failed to verify audit pack")?;
                if report.verified {
                    println!("VERIFIED: {} files match the audit pack.", pack.entries.len());
                } else {
                    println!("FAILED: audit pack does not match the current session contents.");
                }
                for m in &report.mismatches {
                    println!("  MODIFIED: {m}");
                }
                for m in &report.missing {
                    println!("  MISSING:  {m}");
                }
                for u in &report.unaccounted {
                    println!("  NEW:      {u}");
                }
                if !report.verified {
                    anyhow::bail!("audit pack verification failed");
                }
                return Ok(());
            }
            let (pack, path) =
                write_audit_pack(&workspace, &session, output.as_deref()).context("failed to create audit pack")?;
            println!(
                "Wrote audit pack to {} ({} files, session status: {}, turns: {}, events: {}).",
                path.display(),
                pack.entries.len(),
                pack.status,
                pack.turn_count,
                pack.event_count
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_summary() -> SessionSummary {
        SessionSummary {
            session_id: "peer-a".to_string(),
            turn_count: 3,
            event_count: 42,
            status: "active".to_string(),
            updated_at: "2026-11-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn list_row_marks_live_sessions_only() {
        let live_row = format_session_list_row(&sample_summary(), true);
        assert!(live_row.contains("peer-a"), "session id present: {live_row}");
        assert!(live_row.contains("turns=3"), "turn count present: {live_row}");
        assert!(live_row.ends_with("live"), "live marker present: {live_row}");

        let dead_row = format_session_list_row(&sample_summary(), false);
        assert!(!dead_row.contains("live"), "no marker for dead sessions: {dead_row}");
    }
}
