//! Retention and garbage-collection for the unified session store.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;

use crate::error::SessionStoreError;
use crate::query::SessionSummary;
use crate::sessions_root;

/// Retention policy applied to the set of per-session stores.
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    /// Maximum number of sessions to keep (oldest evicted first).
    pub max_sessions: usize,
    /// Maximum age of a session in days before eviction.
    pub max_age_days: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self { max_sessions: 50, max_age_days: 30 }
    }
}

/// Apply the retention policy, removing the oldest / stale sessions.
///
/// Returns the number of sessions removed. This bounds the otherwise
/// unbounded growth of `.vtcode/sessions/` so overhead does not accumulate
/// on disk across a long-lived agent.
pub fn apply_retention(
    workspace: &Path,
    policy: RetentionPolicy,
) -> Result<usize, SessionStoreError> {
    let root = sessions_root(workspace);
    if !root.exists() {
        return Ok(0);
    }
    let mut sessions: Vec<SessionSummary> = crate::query::recent_sessions(workspace, usize::MAX);
    let mut removed = 0usize;

    // Phase 1: evict oldest sessions beyond the count cap.
    if sessions.len() > policy.max_sessions {
        sessions.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        let to_remove = sessions.len() - policy.max_sessions;
        for s in sessions.iter().take(to_remove) {
            remove_session(&root.join(&s.session_id))?;
            removed += 1;
        }
        // Drop evicted entries so phase 2 doesn't double-remove.
        sessions.drain(..to_remove);
    }

    // Phase 2: evict sessions older than max_age_days (regardless of count).
    let cutoff = age_cutoff(policy.max_age_days);
    for s in &sessions {
        if older_than(s.updated_at.as_str(), cutoff) {
            remove_session(&root.join(&s.session_id))?;
            removed += 1;
        }
    }

    Ok(removed)
}

/// Remove the legacy `history/` and `logs/` directories after they have been
/// imported into the unified store by [`crate::migrate_legacy`].
///
/// Returns the number of bytes freed. The legacy `checkpoints/` directory is
/// intentionally left in place until `/revert` is rewired to the unified
/// store; callers should confirm revert behavior before deleting it manually.
pub fn gc_legacy(workspace: &Path) -> Result<u64, SessionStoreError> {
    let vt = workspace.join(".vtcode");
    let mut freed = 0u64;
    for name in ["history", "logs"] {
        let dir = vt.join(name);
        if dir.exists() {
            freed += dir_size(&dir);
            std::fs::remove_dir_all(&dir).map_err(|e| SessionStoreError::io(dir.clone(), e))?;
        }
    }
    Ok(freed)
}

fn remove_session(dir: &Path) -> Result<(), SessionStoreError> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| SessionStoreError::io(dir.to_path_buf(), e))?;
    }
    Ok(())
}

fn dir_size(dir: &Path) -> u64 {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn age_cutoff(max_age_days: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(max_age_days * 24 * 3600)
}

fn older_than(rfc3339: &str, cutoff: SystemTime) -> bool {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339) else {
        return false;
    };
    let cutoff_secs = cutoff
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(i64::MAX);
    dt.timestamp() < cutoff_secs
}
