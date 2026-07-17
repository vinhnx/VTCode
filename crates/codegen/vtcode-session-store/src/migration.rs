//! One-off migration of the legacy, overlapping history stores into the
//! unified per-session store.
//!
//! Legacy inputs:
//! - `.vtcode/history/session-*.memory.json` → `<session>/derived/memory.json`
//! - `.vtcode/logs/trajectory-*.jsonl`  → `<session>/derived/trajectory.jsonl`
//!
//! Legacy `checkpoints/` are intentionally *not* migrated here: they require a
//! lossy `Message` → `ThreadEvent` mapping and `/revert` must be rewired first.

use std::path::Path;

use chrono::Utc;

use crate::error::SessionStoreError;
use crate::{SessionManifest, session_dir};

/// Outcome of a legacy migration run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Number of session directories created.
    pub sessions_created: usize,
    /// Number of session memory envelopes imported.
    pub memory_imported: usize,
    /// Number of trajectory logs imported.
    pub trajectory_imported: usize,
    /// Total bytes copied into the unified store.
    pub bytes_migrated: u64,
}

/// Migrate legacy history/trajectory stores into the unified session store.
///
/// When `remove_legacy` is true, the now-imported `history/` and `logs/`
/// directories are deleted (the `checkpoints/` directory is preserved).
pub fn migrate_legacy(
    workspace: &Path,
    remove_legacy: bool,
) -> Result<MigrationReport, SessionStoreError> {
    let mut report = MigrationReport::default();
    let vt = workspace.join(".vtcode");

    let history_dir = vt.join("history");
    if history_dir.is_dir() {
        for entry in std::fs::read_dir(&history_dir)
            .map_err(|e| SessionStoreError::io(history_dir.clone(), e))?
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let session_id = name.to_string();
            let bytes = std::fs::read(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
            let dir = session_dir(workspace, &session_id);
            std::fs::create_dir_all(dir.join(crate::DERIVED_DIR)).map_err(|e| {
                SessionStoreError::CreateDir {
                    path: dir.clone(),
                    source: e,
                }
            })?;
            let dest = dir.join(crate::DERIVED_DIR).join("memory.json");
            std::fs::write(&dest, &bytes).map_err(|e| SessionStoreError::io(dest, e))?;
            write_manifest(&dir, &session_id, &path, "completed")?;
            report.sessions_created += 1;
            report.memory_imported += 1;
            report.bytes_migrated += bytes.len() as u64;
        }
    }

    let logs_dir = vt.join("logs");
    if logs_dir.is_dir() {
        for entry in std::fs::read_dir(&logs_dir)
            .map_err(|e| SessionStoreError::io(logs_dir.clone(), e))?
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // `trajectory-<ts>` → session id `traj-<ts>` to avoid colliding with
            // history session ids.
            let session_id = format!("traj-{}", name.trim_start_matches("trajectory-"));
            let bytes = std::fs::read(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
            let dir = session_dir(workspace, &session_id);
            std::fs::create_dir_all(dir.join(crate::DERIVED_DIR)).map_err(|e| {
                SessionStoreError::CreateDir {
                    path: dir.clone(),
                    source: e,
                }
            })?;
            let dest = dir.join(crate::DERIVED_DIR).join("trajectory.jsonl");
            std::fs::write(&dest, &bytes).map_err(|e| SessionStoreError::io(dest, e))?;
            write_manifest(&dir, &session_id, &path, "completed")?;
            report.sessions_created += 1;
            report.trajectory_imported += 1;
            report.bytes_migrated += bytes.len() as u64;
        }
    }

    if remove_legacy {
        let freed = crate::retention::gc_legacy(workspace)?;
        let _ = freed;
    }

    Ok(report)
}

fn write_manifest(
    dir: &Path,
    session_id: &str,
    source: &Path,
    status: &str,
) -> Result<(), SessionStoreError> {
    let ts = source
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| chrono::DateTime::<Utc>::from(t).to_rfc3339())
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let mut manifest = SessionManifest::new(session_id);
    manifest.created_at = ts.clone();
    manifest.updated_at = ts;
    manifest.status = status.to_string();
    let path = dir.join("manifest.json");
    let bytes = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&path, bytes).map_err(|e| SessionStoreError::io(path, e))?;
    Ok(())
}
