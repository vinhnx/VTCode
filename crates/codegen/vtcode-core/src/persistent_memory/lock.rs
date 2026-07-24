use super::*;

const LOCK_RETRY_ATTEMPTS: usize = 40;
const LOCK_RETRY_DELAY_MS: u64 = 50;
/// A held `MemoryLock` file is considered abandoned once it is older than this
/// many seconds and may be forcibly removed so a new holder can acquire it.
///
/// This must stay comfortably above the longest legitimate hold time (the
/// lock spans LLM summarize/classify calls, which can take several seconds),
/// so a live lock is never mistakenly stolen. It exists purely to recover
/// locks orphaned by a process that was killed (SIGKILL/OOM/power loss)
/// before its `Drop` impl could remove the lock file.
pub(crate) const LOCK_STALE_AFTER_SECS: u64 = 300;

pub(crate) struct MemoryLock {
    pub(super) path: PathBuf,
}

impl MemoryLock {
    pub(crate) async fn acquire(path: &Path) -> Result<Self> {
        Self::acquire_with_stale_after(path, Duration::from_secs(LOCK_STALE_AFTER_SECS)).await
    }

    /// Same as [`Self::acquire`] but with a caller-supplied staleness threshold,
    /// so the stale-lock recovery path can be exercised in tests without
    /// waiting for the full `LOCK_STALE_AFTER_SECS` duration.
    pub(crate) async fn acquire_with_stale_after(path: &Path, stale_after: Duration) -> Result<Self> {
        for _ in 0..LOCK_RETRY_ATTEMPTS {
            match tokio::fs::OpenOptions::new().create_new(true).write(true).open(path).await {
                Ok(_) => {
                    return Ok(Self { path: path.to_path_buf() });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(age) = lock_age(path).await {
                        if age >= stale_after {
                            // Best-effort removal of an orphaned lock file. If another
                            // process concurrently removes or recreates the file, the
                            // atomic `create_new` on the next loop iteration is what
                            // actually decides ownership, so this stays race-safe.
                            let _ = tokio::fs::remove_file(path).await;
                        }
                    }
                    sleep(Duration::from_millis(LOCK_RETRY_DELAY_MS)).await
                }
                Err(err) => {
                    return Err(err).with_context(|| format!("Failed to acquire {}", path.display()));
                }
            }
        }
        Err(anyhow::anyhow!("Timed out waiting for persistent memory lock {}", path.display()))
    }
}

/// Returns how long ago the file at `path` was last modified, or `None` if
/// its metadata or modification time cannot be determined (treated as "not
/// stale" by callers).
pub(crate) async fn lock_age(path: &Path) -> Option<Duration> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    meta.modified().ok()?.elapsed().ok()
}

impl Drop for MemoryLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
