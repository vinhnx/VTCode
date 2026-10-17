//! Retention and garbage-collection for the unified session store.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;

use crate::error::SessionStoreError;
use crate::query::SessionSummary;
use crate::sessions_root;

#[derive(Debug)]
struct RetentionCandidate {
    path: std::path::PathBuf,
    summary: SessionSummary,
}

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
pub fn apply_retention(workspace: &Path, policy: RetentionPolicy) -> Result<usize, SessionStoreError> {
    apply_retention_preserving(workspace, policy, None)
}

/// Apply retention while preserving one session directory, even when its
/// existing manifest still says `completed` (for example, a resumed session).
///
/// The preserved path is resolved with the same session-id sanitization as the
/// canonical store and is compared against the direct child discovered on
/// disk. It is never taken from a manifest.
pub fn apply_retention_preserving(
    workspace: &Path,
    policy: RetentionPolicy,
    preserve_session_id: Option<&str>,
) -> Result<usize, SessionStoreError> {
    let root = sessions_root(workspace);
    let root_metadata = match std::fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(SessionStoreError::io(root.clone(), error)),
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Ok(0);
    }
    // Crashed/killed threads never emit thread.completed; surface those
    // abandoned `active` stores as completed so the eviction phases can
    // reclaim them. Live sessions stay `active` and remain unpinned.
    mark_abandoned_active_sessions(workspace, policy.max_age_days, preserve_session_id)?;
    let preserve_path = preserve_session_id.map(|session_id| crate::session_dir(workspace, session_id));
    let sessions = retention_candidates(&root, preserve_path.as_deref())?;
    let mut removed = 0usize;

    // Phase 0: drop empty completed shells immediately. A 0-turn store is only
    // `thread.started` + `thread.completed` noise; it never held work the user
    // can resume. Age/count caps would otherwise keep these around for weeks.
    let (empty, mut sessions): (Vec<_>, Vec<_>) = sessions
        .into_iter()
        .partition(|s| s.summary.turn_count == 0 && s.summary.status != "active");
    for s in &empty {
        remove_session(&root, &s.path)?;
        removed += 1;
    }

    // Phase 1: evict oldest sessions beyond the count cap.
    if sessions.len() > policy.max_sessions {
        sessions.sort_by(|a, b| a.summary.updated_at.cmp(&b.summary.updated_at));
        let to_remove = sessions.len() - policy.max_sessions;
        for s in sessions.iter().take(to_remove) {
            remove_session(&root, &s.path)?;
            removed += 1;
        }
        // Drop evicted entries so phase 2 doesn't double-remove.
        sessions.drain(..to_remove);
    }

    // Phase 2: evict sessions older than max_age_days (regardless of count).
    let cutoff = age_cutoff(policy.max_age_days);
    for s in &sessions {
        if older_than(s.summary.updated_at.as_str(), cutoff) {
            remove_session(&root, &s.path)?;
            removed += 1;
        }
    }

    Ok(removed)
}

/// Sidecar marker written when a session must outlive ordinary retention
/// because an unresolved blocker archive references it.
pub const RETENTION_PIN_FILE: &str = "retention-pin.json";

/// Delete a 0-turn completed session store immediately (close-path hygiene).
///
/// Empty shells (`thread.started` + `thread.completed` only) never held work
/// the user can resume; keeping them pollutes `.vtcode/sessions/` and hides
/// real sessions. No-op when the store has turns, is still active, is pinned,
/// is live in another process, or the id is not a validated direct child.
/// Returns whether the store was removed.
pub fn evict_zero_turn_completed_store(workspace: &Path, session_id: &str) -> Result<bool, SessionStoreError> {
    let root = sessions_root(workspace);
    let dir = crate::session_dir(workspace, session_id);
    if dir.parent() != Some(&root) {
        return Ok(false);
    }
    if session_retention_pinned(&dir) || session_dir_is_live(&dir) {
        return Ok(false);
    }
    let manifest_path = dir.join("manifest.json");
    let Ok(bytes) = std::fs::read(&manifest_path) else {
        return Ok(false);
    };
    let Ok(summary) = serde_json::from_slice::<SessionSummary>(&bytes) else {
        return Ok(false);
    };
    if summary.turn_count > 0 || summary.status == "active" {
        return Ok(false);
    }
    remove_session(&root, &dir)?;
    Ok(true)
}

/// Whether a session directory is pinned against ordinary retention eviction.
#[must_use]
pub fn session_retention_pinned(session_dir: &Path) -> bool {
    session_dir.join(RETENTION_PIN_FILE).is_file()
}

/// Whether a live process still holds the session's event-log handles open.
///
/// `session.lock` is flock-held for as long as any event-log handle to the
/// session exists (see `event_log::acquire_liveness_lock`). `WouldBlock`
/// proves a live holder; a missing lock file (older or crashed sessions) or
/// an acquirable lock means nothing alive keeps the session open. Unreadable
/// lock files count as live: deletion paths must not evict what they cannot
/// inspect.
pub(crate) fn session_dir_is_live(session_dir: &Path) -> bool {
    use crate::event_log::SESSION_LOCK_FILE;

    let lock_path = session_dir.join(SESSION_LOCK_FILE);
    let file = match std::fs::OpenOptions::new().read(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    file.try_lock()
        .map_err(std::io::Error::from)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
}

/// Session ids whose stores are retention-pinned against ordinary eviction.
///
/// Companion to [`session_retention_pinned`]: a blocked session pins its store
/// so ordinary retention cannot erase its evidence. Callers pruning other
/// session-derived data (e.g. legacy history envelopes) must exclude these ids
/// or they would erase through a different path what the pin protects.
#[must_use]
pub fn retention_pinned_session_ids(workspace: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(sessions_root(workspace)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().map(|file_type| file_type.is_dir()).unwrap_or(false))
        .filter(|entry| session_retention_pinned(&entry.path()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// Write a retention pin for a session (best-effort path check by caller).
pub fn pin_session_retention(session_dir: &Path, reason: &str) -> Result<(), SessionStoreError> {
    let path = session_dir.join(RETENTION_PIN_FILE);
    let body = serde_json::json!({
        "reason": reason,
        "pinned_at": chrono::Utc::now().to_rfc3339(),
    });
    std::fs::write(&path, body.to_string()).map_err(|e| SessionStoreError::io(path, e))
}

/// Remove a retention pin if present.
pub fn unpin_session_retention(session_dir: &Path) -> Result<bool, SessionStoreError> {
    let path = session_dir.join(RETENTION_PIN_FILE);
    if !path.is_file() {
        return Ok(false);
    }
    std::fs::remove_file(&path).map_err(|e| SessionStoreError::io(path, e))?;
    Ok(true)
}

/// Enumerate session stores from their filesystem entries, never from the
/// session ID contained in a manifest. This keeps retention confined to
/// validated direct children of the sessions root.
fn retention_candidates(
    root: &Path,
    preserve_path: Option<&Path>,
) -> Result<Vec<RetentionCandidate>, SessionStoreError> {
    let entries = std::fs::read_dir(root).map_err(|e| SessionStoreError::io(root.to_path_buf(), e))?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| SessionStoreError::io(root.to_path_buf(), e))?;
        let file_type = entry.file_type().map_err(|e| SessionStoreError::io(entry.path(), e))?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if preserve_path.is_some_and(|preserve_path| preserve_path == path) {
            continue;
        }
        // Unresolved blocker forensics must survive count/age eviction.
        if session_retention_pinned(&path) {
            continue;
        }
        // A session still open in a live process must not be evicted even
        // when its manifest says completed: the user may resume or keep
        // reading it.
        if session_dir_is_live(&path) {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        let Ok(bytes) = std::fs::read(&manifest_path) else {
            continue;
        };
        let Ok(summary) = serde_json::from_slice::<SessionSummary>(&bytes) else {
            continue;
        };
        if summary.status == "active" {
            continue;
        }
        candidates.push(RetentionCandidate { path, summary });
    }
    Ok(candidates)
}

/// Flip `active` manifests that have been idle past `max_age_days` to
/// `completed` so ordinary retention can evict them.
///
/// A crashed or killed thread never emits `thread.completed`, so its manifest
/// stays `active` forever and would otherwise pin the store. Live sessions are
/// younger than the cutoff and are left untouched. `max_age_days == 0` disables
/// the sweep (the hard "never evict active" contract used by force-evict tests).
/// Returns how many manifests were marked abandoned.
pub fn mark_abandoned_active_sessions(
    workspace: &Path,
    max_age_days: u64,
    preserve_session_id: Option<&str>,
) -> Result<usize, SessionStoreError> {
    if max_age_days == 0 {
        return Ok(0);
    }
    let root = sessions_root(workspace);
    let preserve_path = preserve_session_id.map(|session_id| crate::session_dir(workspace, session_id));
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(0);
    };
    let cutoff = age_cutoff(max_age_days);
    let mut marked = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        // Same symlink-safe enumeration as `retention_candidates`: never
        // rewrite a manifest through a planted symlink.
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if preserve_path.as_deref() == Some(path.as_path()) {
            continue;
        }
        if session_retention_pinned(&path) {
            continue;
        }
        // A live process still holds this session open (open-but-idle):
        // marking it completed would let phase-2 evict a session the user
        // may still resume.
        if session_dir_is_live(&path) {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        let Ok(bytes) = std::fs::read(&manifest_path) else {
            continue;
        };
        let Ok(mut summary) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let is_active = summary.get("status").and_then(serde_json::Value::as_str) == Some("active");
        if !is_active {
            continue;
        }
        let updated_at = summary
            .get("updated_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !older_than(updated_at, cutoff) {
            continue;
        }
        if let Some(object) = summary.as_object_mut() {
            object.insert("status".to_string(), serde_json::Value::String("completed".to_string()));
        } else {
            continue;
        }
        let body = serde_json::to_vec_pretty(&summary)
            .map_err(|error| SessionStoreError::io(manifest_path.clone(), std::io::Error::other(error)))?;
        // Same primitive as ManifestStore: 0600 private temp + fsync + rename,
        // so a crash cannot leave a truncated manifest, the rewritten manifest
        // keeps session-file permissions, and a planted symlink cannot be
        // followed to an outside destination.
        vtcode_commons::VtCodePaths::write_private_file_atomic(&manifest_path, &body)
            .map_err(|error| SessionStoreError::io(manifest_path.clone(), std::io::Error::other(error)))?;
        marked += 1;
    }
    Ok(marked)
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

fn remove_session(root: &Path, dir: &Path) -> Result<(), SessionStoreError> {
    // Retention is allowed to remove only one validated child of the sessions
    // root. Never trust a manifest-controlled identifier or follow a symlink.
    if dir.parent() != Some(root) {
        return Ok(());
    }
    let metadata = match std::fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SessionStoreError::io(dir.to_path_buf(), error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(());
    }
    std::fs::remove_dir_all(dir).map_err(|e| SessionStoreError::io(dir.to_path_buf(), e))?;
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
    let seconds = max_age_days.saturating_mul(24 * 3600);
    SystemTime::now() - Duration::from_secs(seconds)
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
