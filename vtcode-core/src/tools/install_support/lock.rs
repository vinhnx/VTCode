use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::path::Path;

use super::time::modified_age_secs;

pub(crate) fn create_lock_file(lock_path: &Path) -> Result<File> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(lock_path)
        .with_context(|| format!("Failed to create lock file {}", lock_path.display()))
}

pub(crate) fn lock_is_active(lock_path: &Path, max_age_secs: u64) -> bool {
    modified_age_secs(lock_path).is_some_and(|age| age < max_age_secs)
}
