//! Unified per-session state store for VT Code.
//!
//! This crate is the single source of truth for an agent session's state,
//! context, and history. Each session is persisted under
//! `.vtcode/sessions/<session_id>/` as:
//!
//! - `events.jsonl` — the canonical append-only [`ThreadEvent`](vtcode_exec_events::ThreadEvent)
//!   log (schema-versioned). Everything else is derived from this.
//! - `manifest.json` — session metadata and counters.
//! - `index/turns.json` — byte-offset index enabling O(1) turn reconstruction.
//! - `derived/` — regenerated views (`trajectory.jsonl`, `memory.json`, …).
//!
//! The store is intentionally append-only and off the agent's hot path: the
//! live conversation stays in memory and is never reloaded from disk into
//! context. Reads happen only for revert, compaction, analytics, and
//! long-term-learning queries.

pub mod error;
pub mod event_log;
pub mod migration;
pub mod progress;
pub mod query;
pub mod retention;

pub use error::SessionStoreError;
pub use event_log::{SessionEventLog, SessionManifest, TurnIndex, TurnIndexEntry};
pub use migration::{MigrationReport, migrate_legacy};
pub use progress::{
    Milestone, MilestoneStatus, ProgressLedger, load_progress, progress_path, save_progress,
};
pub use query::{FactRecord, SessionSummary, query_facts, recent_sessions};
pub use retention::{RetentionPolicy, apply_retention, gc_legacy};

use std::path::{Path, PathBuf};

/// Directory (relative to the workspace) holding all per-session stores.
pub const SESSIONS_DIR: &str = ".vtcode/sessions";

/// Sub-directory inside a session holding regenerated views.
pub const DERIVED_DIR: &str = "derived";

/// Schema version for the on-disk session store layout.
pub const SESSION_STORE_SCHEMA_VERSION: u32 = 1;

/// Resolve the sessions root directory for a workspace.
#[must_use]
pub fn sessions_root(workspace: &Path) -> PathBuf {
    workspace.join(SESSIONS_DIR)
}

/// Resolve the directory for a single session.
#[must_use]
pub fn session_dir(workspace: &Path, session_id: &str) -> PathBuf {
    sessions_root(workspace).join(sanitize_id(session_id))
}

/// Open (creating if necessary) the event log for a session.
///
/// This is the canonical entry point for recording a session's events. The
/// returned [`SessionEventLog`] is cheap to clone (internally `Arc`-free but the
/// file handle is shared via an internal mutex) and supports concurrent
/// `append` calls from the runloop's event sink.
pub fn open(workspace: &Path, session_id: &str) -> Result<SessionEventLog, SessionStoreError> {
    SessionEventLog::open(workspace, session_id)
}

/// Sanitize a session id so it is safe to use as a directory name.
fn sanitize_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for c in id.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    // Strip leading dots to avoid creating hidden directories.
    let out = out.trim_start_matches('.').to_string();
    if out.is_empty() {
        "session".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests;
