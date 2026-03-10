use std::fs::{self, File};
use std::path::PathBuf;

use crate::tools::ast_grep_binary::{alias_ast_grep_binary_name, canonical_ast_grep_binary_name};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::super::install_support::vtcode_state_dir_from_home;
use super::super::install_support::{
    cache_is_stale, create_lock_file, load_json_cache, lock_is_active, save_json_cache,
    unix_timestamp_now, vtcode_state_dir,
};
#[cfg(test)]
use std::path::Path;

const INSTALL_LOCK_MAX_AGE_SECS: u64 = 1_800;
const INSTALL_CACHE_STALE_AFTER_SECS: u64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct InstallationCache {
    pub(super) last_attempt: u64,
    pub(super) status: String,
    pub(super) release_tag: Option<String>,
    pub(super) failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct InstallPaths {
    pub(super) state_dir: PathBuf,
    pub(super) bin_dir: PathBuf,
    pub(super) cache_path: PathBuf,
    pub(super) lock_path: PathBuf,
    pub(super) binary_path: PathBuf,
    pub(super) alias_path: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) struct InstallLockGuard {
    path: PathBuf,
    _file: File,
}

impl InstallationCache {
    pub(super) fn load(paths: &InstallPaths) -> Result<Self> {
        load_json_cache(&paths.cache_path, "ast-grep install cache")
    }

    fn save(&self, paths: &InstallPaths) -> Result<()> {
        save_json_cache(
            &paths.state_dir,
            &paths.cache_path,
            self,
            "ast-grep install cache",
        )
    }

    pub(super) fn is_stale(paths: &InstallPaths) -> bool {
        let Ok(cache) = Self::load(paths) else {
            return true;
        };
        cache_is_stale(cache.last_attempt, INSTALL_CACHE_STALE_AFTER_SECS)
    }

    pub(super) fn mark_success(paths: &InstallPaths, release_tag: &str) {
        let cache = Self {
            last_attempt: unix_timestamp_now(),
            status: "success".to_string(),
            release_tag: Some(release_tag.to_string()),
            failure_reason: None,
        };
        let _ = cache.save(paths);
    }

    pub(super) fn mark_failure(paths: &InstallPaths, reason: &str) {
        let cache = Self {
            last_attempt: unix_timestamp_now(),
            status: "failed".to_string(),
            release_tag: None,
            failure_reason: Some(reason.to_string()),
        };
        let _ = cache.save(paths);
    }
}

impl InstallPaths {
    pub(super) fn discover() -> Result<Self> {
        let state_dir = vtcode_state_dir()
            .context("Cannot determine home directory for VT Code-managed ast-grep install")?;
        Ok(Self::from_state_dir(state_dir))
    }

    #[cfg(test)]
    fn from_home(home: &Path) -> Self {
        Self::from_state_dir(vtcode_state_dir_from_home(home))
    }

    fn from_state_dir(state_dir: PathBuf) -> Self {
        let bin_dir = state_dir.join("bin");
        Self {
            cache_path: state_dir.join("ast_grep_install_cache.json"),
            lock_path: state_dir.join("ast_grep.lock"),
            binary_path: bin_dir.join(canonical_ast_grep_binary_name()),
            alias_path: alias_ast_grep_binary_name().map(|name| bin_dir.join(name)),
            state_dir,
            bin_dir,
        }
    }
}

impl InstallLockGuard {
    pub(super) fn acquire(paths: &InstallPaths) -> Result<Self> {
        if Self::is_install_in_progress(paths) {
            bail!("ast-grep installation already in progress");
        }

        let file = create_lock_file(&paths.lock_path)?;
        Ok(Self {
            path: paths.lock_path.clone(),
            _file: file,
        })
    }

    fn is_install_in_progress(paths: &InstallPaths) -> bool {
        lock_is_active(&paths.lock_path, INSTALL_LOCK_MAX_AGE_SECS)
    }
}

impl Drop for InstallLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallLockGuard, InstallPaths, InstallationCache};
    use tempfile::TempDir;

    #[test]
    fn install_paths_live_under_vtcode_home() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        let expected_state_dir = temp_dir.path().join(".vtcode");
        let expected_bin_dir = expected_state_dir.join("bin");
        assert_eq!(paths.state_dir, expected_state_dir);
        assert_eq!(paths.bin_dir, expected_bin_dir);
        assert_eq!(
            paths.cache_path,
            temp_dir.path().join(".vtcode/ast_grep_install_cache.json")
        );
        assert_eq!(
            paths.lock_path,
            temp_dir.path().join(".vtcode/ast_grep.lock")
        );
    }

    #[test]
    fn install_lock_detects_recent_lockfile() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");
        std::fs::write(&paths.lock_path, "lock").expect("lock file");

        assert!(InstallLockGuard::is_install_in_progress(&paths));
    }

    #[test]
    fn installation_cache_round_trips() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        let cache = InstallationCache {
            last_attempt: 42,
            status: "failed".to_string(),
            release_tag: None,
            failure_reason: Some("boom".to_string()),
        };

        cache.save(&paths).expect("save cache");
        let loaded = InstallationCache::load(&paths).expect("load cache");
        assert_eq!(loaded.last_attempt, 42);
        assert_eq!(loaded.status, "failed");
        assert_eq!(loaded.failure_reason.as_deref(), Some("boom"));
    }
}
