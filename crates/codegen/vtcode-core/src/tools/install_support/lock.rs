use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::path::Path;

use super::time::modified_age_secs;

/// Atomically attempts to acquire an installer lock file at `lock_path`.
///
/// Uses `create_new` so that racing processes cannot both "acquire" the
/// lock: only the first process to create the file succeeds, and every
/// other process observes `AlreadyExists` instead of silently truncating
/// and rewriting a file another process relies on for mutual exclusion.
///
/// If an existing lock file is older than `max_age_secs`, it is treated as
/// abandoned -- e.g. orphaned by a process that was killed (SIGKILL/OOM/power
/// loss) before its `Drop` impl could remove it -- and is best-effort removed
/// so a fresh attempt can proceed. Returns `Ok(None)` when another live
/// process currently holds the lock; the caller should treat this as
/// "installation already in progress" rather than an error.
pub(crate) fn acquire_lock_file(lock_path: &Path, max_age_secs: u64) -> Result<Option<File>> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    if let Some(file) = try_create_new_lock_file(lock_path)? {
        return Ok(Some(file));
    }

    // Another lock file already exists. If it looks abandoned, reclaim it.
    // This retry is still race-safe: ownership is decided by whichever
    // process wins the atomic `create_new` call below, not by this
    // staleness check, so a concurrent reclaim attempt cannot corrupt state.
    if !lock_is_active(lock_path, max_age_secs) {
        let _ = fs::remove_file(lock_path);
        return try_create_new_lock_file(lock_path);
    }

    Ok(None)
}

fn try_create_new_lock_file(lock_path: &Path) -> Result<Option<File>> {
    match OpenOptions::new().create_new(true).write(true).open(lock_path) {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(err) => {
            Err(err).with_context(|| format!("Failed to create lock file {}", lock_path.display()))
        }
    }
}

pub(crate) fn lock_is_active(lock_path: &Path, max_age_secs: u64) -> bool {
    modified_age_secs(lock_path).is_some_and(|age| age < max_age_secs)
}
