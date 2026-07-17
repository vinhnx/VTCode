use std::fs::{self, File};
use std::path::PathBuf;

use crate::tools::ast_grep_binary::{alias_ast_grep_binary_name, canonical_ast_grep_binary_name};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::super::install_support::vtcode_state_dir_from_home;
use super::super::install_support::{
    acquire_lock_file, cache_is_stale, load_json_cache, lock_is_active, save_json_cache,
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
        save_json_cache(&paths.state_dir, &paths.cache_path, self, "ast-grep install cache")
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
        // Fast-path check to avoid the syscall overhead of attempting a lock
        // acquisition that is very likely to fail. This is only an
        // optimization: correctness under concurrent processes comes from
        // the atomic `create_new` in `acquire_lock_file`, which is the sole
        // arbiter of who wins the race even if two processes both pass this
        // check simultaneously.
        if Self::is_install_in_progress(paths) {
            bail!("ast-grep installation already in progress");
        }

        match acquire_lock_file(&paths.lock_path, INSTALL_LOCK_MAX_AGE_SECS)? {
            Some(file) => Ok(Self { path: paths.lock_path.clone(), _file: file }),
            None => bail!("ast-grep installation already in progress"),
        }
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
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    #[test]
    fn install_paths_live_under_vtcode_home() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        let expected_state_dir = temp_dir.path().join(".vtcode");
        let expected_bin_dir = expected_state_dir.join("bin");
        assert_eq!(paths.state_dir, expected_state_dir);
        assert_eq!(paths.bin_dir, expected_bin_dir);
        assert_eq!(paths.cache_path, temp_dir.path().join(".vtcode/ast_grep_install_cache.json"));
        assert_eq!(paths.lock_path, temp_dir.path().join(".vtcode/ast_grep.lock"));
    }

    #[test]
    fn install_lock_detects_recent_lockfile() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");
        std::fs::write(&paths.lock_path, "lock").expect("lock file");

        assert!(InstallLockGuard::is_install_in_progress(&paths));
    }

    /// Regression test for the check-then-act lock race: spawn several
    /// threads that all race to acquire the install lock at (as close to)
    /// the same instant, and assert that exactly one of them wins. Before
    /// the fix, `create_lock_file` used `create(true).truncate(true)`
    /// instead of `create_new(true)`, so multiple racing callers could each
    /// observe "no lock in progress" and then all successfully "acquire" the
    /// lock by truncating the same file out from under one another.
    ///
    /// A `done_barrier` keeps every acquired guard alive until all threads
    /// have made their attempt, so a fast winner cannot drop its guard (and
    /// thus remove the lock file) before a slower thread's atomic
    /// `create_new` call runs -- which would let more than one thread "win"
    /// and make the test flaky.
    #[test]
    fn install_lock_only_one_concurrent_acquirer_wins() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");

        const THREADS: usize = 8;
        let start_barrier = Arc::new(Barrier::new(THREADS));
        let done_barrier = Arc::new(Barrier::new(THREADS));

        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let paths = paths.clone();
                let start_barrier = Arc::clone(&start_barrier);
                let done_barrier = Arc::clone(&done_barrier);
                std::thread::spawn(move || {
                    start_barrier.wait();
                    let guard = InstallLockGuard::acquire(&paths);
                    let acquired = guard.is_ok();
                    done_barrier.wait();
                    drop(guard);
                    acquired
                })
            })
            .collect();

        let successes = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread panicked"))
            .filter(|acquired| *acquired)
            .count();

        assert_eq!(successes, 1, "exactly one thread should win the install lock");
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
