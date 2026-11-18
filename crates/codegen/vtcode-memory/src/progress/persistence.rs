//! On-disk persistence for the session progress ledger.
//!
//! The ledger is a tiny derived artifact under `<session_dir>/derived/`, not
//! part of the canonical append-only event log. These helpers are grouped so
//! `progress/mod.rs` can focus on the ledger's data types and the goal tracker
//! state machine. All three are re-exported from the parent, so the public
//! paths (`vtcode_memory::progress::*`) are unchanged.

use super::*;

/// Resolve the on-disk path of the progress ledger for a session.
#[must_use]
pub fn progress_path(workspace: &Path, session_id: &str) -> PathBuf {
    session_dir(workspace, session_id)
        .join(crate::DERIVED_DIR)
        .join("progress.json")
}

/// Load the progress ledger for a session, if one has been persisted.
///
/// Returns `Ok(None)` when no ledger file exists yet (a fresh or pre-ledger
/// session) rather than an error, so callers can treat absence as "no signal".
pub fn load_progress(workspace: &Path, session_id: &str) -> Result<Option<ProgressLedger>, SessionStoreError> {
    let path = progress_path(workspace, session_id);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
    let ledger: ProgressLedger = serde_json::from_slice(&bytes)?;
    Ok(Some(ledger))
}

/// Persist the progress ledger for a session, creating `derived/` if needed.
pub fn save_progress(workspace: &Path, session_id: &str, ledger: &ProgressLedger) -> Result<(), SessionStoreError> {
    let path = progress_path(workspace, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SessionStoreError::CreateDir { path: parent.to_path_buf(), source: e })?;
    }
    let bytes = serde_json::to_vec(ledger)?;
    std::fs::write(&path, bytes).map_err(|e| SessionStoreError::io(path, e))?;
    Ok(())
}
