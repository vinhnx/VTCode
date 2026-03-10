use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::PathBuf;

use super::super::install_support::{
    cache_is_stale, create_lock_file, load_json_cache, lock_is_active, save_json_cache,
    unix_timestamp_now, vtcode_state_dir_or_default,
};

const INSTALL_LOCK_MAX_AGE_SECS: u64 = 1_800;
const INSTALL_CACHE_STALE_AFTER_SECS: u64 = 86_400;

/// Installation attempt cache to avoid repeated retries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct InstallationCache {
    /// Timestamp of last installation attempt
    pub(super) last_attempt: u64,
    /// Status from last attempt
    pub(super) status: String,
    /// Method that was attempted
    pub(super) method_attempted: Option<String>,
    /// Reason for failure (if applicable)
    pub(super) failure_reason: Option<String>,
}

#[derive(Debug)]
pub(super) struct InstallLockGuard {
    path: PathBuf,
    _file: File,
}

impl InstallationCache {
    fn state_dir() -> PathBuf {
        vtcode_state_dir_or_default()
    }

    fn cache_path() -> PathBuf {
        Self::state_dir().join("ripgrep_install_cache.json")
    }

    pub(super) fn is_stale() -> bool {
        match Self::load() {
            Ok(cache) => cache_is_stale(cache.last_attempt, INSTALL_CACHE_STALE_AFTER_SECS),
            Err(_) => true,
        }
    }

    pub(super) fn load() -> Result<Self> {
        load_json_cache(&Self::cache_path(), "ripgrep installation cache")
    }

    fn save(&self) -> Result<()> {
        save_json_cache(
            &Self::state_dir(),
            &Self::cache_path(),
            self,
            "ripgrep installation cache",
        )
    }

    pub(super) fn mark_failed(method: &str, reason: &str) {
        let cache = InstallationCache {
            last_attempt: unix_timestamp_now(),
            status: "failed".to_string(),
            method_attempted: Some(method.to_string()),
            failure_reason: Some(reason.to_string()),
        };
        let _ = cache.save();
    }

    pub(super) fn mark_success(method: &str) {
        let cache = InstallationCache {
            last_attempt: unix_timestamp_now(),
            status: "success".to_string(),
            method_attempted: Some(method.to_string()),
            failure_reason: None,
        };
        let _ = cache.save();
    }
}

impl InstallLockGuard {
    pub(super) fn acquire() -> Result<Self> {
        let path = lock_path();
        let file = create_lock_file(&path)?;
        Ok(Self { path, _file: file })
    }

    pub(super) fn is_install_in_progress() -> bool {
        lock_is_active(&lock_path(), INSTALL_LOCK_MAX_AGE_SECS)
    }
}

impl Drop for InstallLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_path() -> PathBuf {
    vtcode_state_dir_or_default().join("ripgrep.lock")
}
