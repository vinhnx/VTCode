//! One-off migration of the legacy, overlapping history stores into the
//! unified per-session store.
//!
//! Legacy inputs:
//! - `.vtcode/history/session-*.memory.json` → `<session>/derived/memory.json`
//! - `.vtcode/logs/trajectory-*.jsonl`  → `<session>/derived/trajectory.jsonl`
//!
//! Legacy `checkpoints/` are intentionally *not* migrated here: they require a
//! lossy `Message` → `ThreadEvent` mapping and `/revert` must be rewired first.

use std::path::{Path, PathBuf};

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
/// When `remove_legacy` is true, successfully imported source files are
/// removed after verifying their contents; other files are preserved.
/// Removal stages each source aside with an atomic rename and re-verifies the
/// moved file, so a concurrent replacement at the original path is never
/// deleted; a mismatch preserves the staged copy and aborts with an error.
/// Existing session directories are never overwritten; exact completed imports
/// are reused, while a collision stops migration and preserves legacy sources.
pub fn migrate_legacy(workspace: &Path, remove_legacy: bool) -> Result<MigrationReport, SessionStoreError> {
    let mut report = MigrationReport::default();
    let mut imported = Vec::new();
    let mut guards = Vec::new();
    let vt = workspace.join(".vtcode");

    let history_dir = vt.join("history");
    if legacy_directory_exists(&history_dir)? {
        for entry in std::fs::read_dir(&history_dir).map_err(|e| SessionStoreError::io(history_dir.clone(), e))? {
            let entry = entry.map_err(|error| SessionStoreError::io(&history_dir, error))?;
            if !entry
                .file_type()
                .map_err(|error| SessionStoreError::io(entry.path(), error))?
                .is_file()
            {
                continue;
            }
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
            let dest = dir.join(crate::DERIVED_DIR).join("memory.json");
            let (guard, exists) = claim_session(workspace, &dir, &session_id, &path, &dest, &bytes)?;
            if !exists {
                crate::ensure_private_directory(&dir)?;
                crate::ensure_private_directory(&dir.join(crate::DERIVED_DIR))?;
                vtcode_commons::VtCodePaths::write_private_file_atomic(&dest, &bytes)
                    .map_err(|error| SessionStoreError::io(&dest, std::io::Error::other(error)))?;
                write_manifest(&dir, &session_id, &path, "completed")?;
                report.sessions_created += 1;
                report.memory_imported += 1;
                report.bytes_migrated += bytes.len() as u64;
            }
            guards.push(guard);
            imported.push((path, dest));
        }
    }

    let logs_dir = vt.join("logs");
    if legacy_directory_exists(&logs_dir)? {
        for entry in std::fs::read_dir(&logs_dir).map_err(|e| SessionStoreError::io(logs_dir.clone(), e))? {
            let entry = entry.map_err(|error| SessionStoreError::io(&logs_dir, error))?;
            if !entry
                .file_type()
                .map_err(|error| SessionStoreError::io(entry.path(), error))?
                .is_file()
            {
                continue;
            }
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
            let dest = dir.join(crate::DERIVED_DIR).join("trajectory.jsonl");
            let (guard, exists) = claim_session(workspace, &dir, &session_id, &path, &dest, &bytes)?;
            if !exists {
                crate::ensure_private_directory(&dir)?;
                crate::ensure_private_directory(&dir.join(crate::DERIVED_DIR))?;
                vtcode_commons::VtCodePaths::write_private_file_atomic(&dest, &bytes)
                    .map_err(|error| SessionStoreError::io(&dest, std::io::Error::other(error)))?;
                write_manifest(&dir, &session_id, &path, "completed")?;
                report.sessions_created += 1;
                report.trajectory_imported += 1;
                report.bytes_migrated += bytes.len() as u64;
            }
            guards.push(guard);
            imported.push((path, dest));
        }
    }

    if remove_legacy {
        remove_imported_sources(&imported)?;
    }
    // Guards stay held through cleanup so retention cannot remove an imported
    // destination between verification and legacy-source removal.
    drop(guards);

    Ok(report)
}

fn legacy_directory_exists(dir: &Path) -> Result<bool, SessionStoreError> {
    match std::fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(SessionStoreError::io(dir, std::io::Error::other("legacy path is not a directory"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SessionStoreError::io(dir, error)),
    }
}

fn remove_imported_sources(imported: &[(PathBuf, PathBuf)]) -> Result<(), SessionStoreError> {
    for (source, dest) in imported {
        verify_imported(source, dest)?;
    }
    for (source, dest) in imported {
        let staged = staged_removal_path(source)?;
        // Best-effort collision check: planting a staged file here needs
        // directory write access, which already permits deleting sources.
        if std::fs::symlink_metadata(&staged).is_ok() {
            return Err(SessionStoreError::io(
                &staged,
                std::io::Error::other("staged removal target already exists; refusing to overwrite"),
            ));
        }
        std::fs::rename(source, &staged).map_err(|error| SessionStoreError::io(source, error))?;
        if read_no_follow(&staged)? != read_no_follow(dest)? {
            return Err(staged_mismatch_error(&staged));
        }
        std::fs::remove_file(&staged).map_err(|error| SessionStoreError::io(&staged, error))?;
    }
    Ok(())
}

fn verify_imported(source: &Path, dest: &Path) -> Result<(), SessionStoreError> {
    if read_no_follow(source)? != read_no_follow(dest)? {
        return Err(SessionStoreError::io(source, std::io::Error::other("legacy source changed after import")));
    }
    Ok(())
}

fn read_no_follow(path: &Path) -> Result<Vec<u8>, SessionStoreError> {
    vtcode_commons::VtCodePaths::read_file_no_follow(path)
        .map_err(|error| SessionStoreError::io(path, std::io::Error::other(error)))
}

/// Staging path for fail-closed legacy cleanup: same directory (so the rename
/// stays on one filesystem), hidden, and never matching the importer's
/// `.json`/`.jsonl` extension filter.
fn staged_removal_path(source: &Path) -> Result<PathBuf, SessionStoreError> {
    let missing = || SessionStoreError::io(source, std::io::Error::other("legacy source has no file name"));
    let name = source.file_name().ok_or_else(missing)?;
    let mut staged = std::ffi::OsString::from(".");
    staged.push(name);
    staged.push(format!(".migrated-{}", std::process::id()));
    source.parent().ok_or_else(missing).map(|parent| parent.join(staged))
}

fn staged_mismatch_error(staged: &Path) -> SessionStoreError {
    SessionStoreError::io(
        staged,
        std::io::Error::other("legacy source changed after import; staged copy preserved for manual recovery"),
    )
}

fn claim_session(
    workspace: &Path,
    dir: &Path,
    session_id: &str,
    source: &Path,
    dest: &Path,
    bytes: &[u8],
) -> Result<(std::fs::File, bool), SessionStoreError> {
    let name = dir
        .file_name()
        .ok_or_else(|| SessionStoreError::io(dir, std::io::Error::other("missing session name")))?;
    let guard = crate::session_guard::try_exclusive(workspace, name)?
        .ok_or_else(|| SessionStoreError::SessionBusy { session: session_id.to_owned() })?;
    match std::fs::symlink_metadata(dir) {
        Ok(metadata)
            if metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && vtcode_commons::VtCodePaths::read_file_no_follow(dir.join("manifest.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<SessionManifest>(&bytes).ok())
                    .is_some_and(|manifest| manifest == migrated_manifest(session_id, source, "completed"))
                && vtcode_commons::VtCodePaths::read_file_no_follow(dest).is_ok_and(|stored| stored == bytes) =>
        {
            Ok((guard, true))
        }
        Ok(_) => Err(SessionStoreError::io(
            dir,
            std::io::Error::new(std::io::ErrorKind::AlreadyExists, "migration would overwrite an existing session"),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((guard, false)),
        Err(error) => Err(SessionStoreError::io(dir, error)),
    }
}

fn write_manifest(dir: &Path, session_id: &str, source: &Path, status: &str) -> Result<(), SessionStoreError> {
    let manifest = migrated_manifest(session_id, source, status);
    let path = dir.join("manifest.json");
    let bytes = serde_json::to_vec(&manifest)?;
    vtcode_commons::VtCodePaths::write_private_file_atomic(&path, &bytes)
        .map_err(|error| SessionStoreError::io(path, std::io::Error::other(error)))?;
    Ok(())
}

fn migrated_manifest(session_id: &str, source: &Path, status: &str) -> SessionManifest {
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
    manifest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_preserves_all_sources_when_one_changed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        let first_copy = dir.path().join("first-copy");
        let second_copy = dir.path().join("second-copy");
        std::fs::write(&first, b"same").expect("first");
        std::fs::write(&first_copy, b"same").expect("first copy");
        std::fs::write(&second, b"new").expect("changed source");
        std::fs::write(&second_copy, b"old").expect("second copy");
        let error = remove_imported_sources(&[(first.clone(), first_copy), (second.clone(), second_copy)])
            .expect_err("changed source must stop cleanup");
        assert!(error.to_string().contains("changed after import"));
        assert_eq!(std::fs::read(first).expect("first retained"), b"same");
        assert_eq!(std::fs::read(second).expect("second retained"), b"new");
    }

    #[test]
    fn cleanup_refuses_to_overwrite_existing_staged_copy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        std::fs::write(&source, b"same").expect("source");
        std::fs::write(&dest, b"same").expect("dest");
        let staged = staged_removal_path(&source).expect("staged path");
        std::fs::write(&staged, b"stale").expect("stale staged");
        let error = remove_imported_sources(&[(source.clone(), dest)]).expect_err("staged collision must stop cleanup");
        assert!(error.to_string().contains("already exists"));
        assert_eq!(std::fs::read(&source).expect("source retained"), b"same");
        assert_eq!(std::fs::read(&staged).expect("staged retained"), b"stale");
    }

    #[test]
    fn cleanup_removes_source_without_leaving_a_staged_copy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        std::fs::write(&source, b"same").expect("source");
        std::fs::write(&dest, b"same").expect("dest");
        let staged = staged_removal_path(&source).expect("staged path");
        remove_imported_sources(&[(source.clone(), dest)]).expect("cleanup");
        assert!(!source.exists());
        assert!(!staged.exists());
    }

    #[test]
    fn staged_removal_names_are_never_imported() {
        for name in ["session.memory.json", "trajectory-1.jsonl"] {
            let staged = staged_removal_path(Path::new(name)).expect("staged path");
            let ext = staged.extension().and_then(|e| e.to_str());
            assert_ne!(ext, Some("json"), "{staged:?}");
            assert_ne!(ext, Some("jsonl"), "{staged:?}");
        }
    }

    #[test]
    fn staged_mismatch_error_names_the_preserved_copy() {
        let error = staged_mismatch_error(Path::new("/tmp/.source.migrated-1"));
        assert!(error.to_string().contains("changed after import"));
        assert!(error.to_string().contains("staged copy preserved"));
    }
}
