//! Checkpoint retention: navigation protection, count budgets, age expiry,
//! and filesnap content reclamation.
//!
//! Retention is best-effort and retryable: every failure path warns and defers
//! rather than failing the turn. Protection is session-aware (see
//! [`SnapshotRecordId`]); the count budget itself stays workspace-global.

use super::*;

impl SnapshotManager {
    /// Checkpoint records still referenced by any *recent* session navigation
    /// (active branch, redo stack, or pending recovery). Rewind/redo must
    /// never lose these. Branch files older than the retention window are
    /// ignored so a completed session cannot pin its turns forever.
    ///
    /// Scoped records are protected per owning branch; legacy (unscoped)
    /// records stay protected by *any* branch referencing the turn, exactly
    /// as before session scoping existed.
    fn protected_turns(&self) -> BTreeSet<SnapshotRecordId> {
        let cutoff = self.retention_cutoff_secs().ok().flatten();
        self.protected_turns_with_cutoff(cutoff)
    }

    fn protected_turns_with_cutoff(&self, cutoff: Option<u64>) -> BTreeSet<SnapshotRecordId> {
        let mut protected = BTreeSet::new();
        let Ok(entries) = fs::read_dir(&self.storage_dir) else {
            return protected;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(branch_key) = stem.strip_prefix("branch_") else {
                continue;
            };
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            // A malformed branch key cannot own scoped records; its turns
            // still protect the legacy layout below.
            let scoped_key = (!branch_key.is_empty()
                && branch_key.len() % 2 == 0
                && branch_key.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| branch_key.to_owned());
            if let Some(cutoff) = cutoff {
                // Stale navigation (completed/abandoned session past retention)
                // must not keep its turn files alive. `cutoff` is a UNIX
                // timestamp: branch files last modified before it are stale.
                let modified_secs = path
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                    .map(|since| since.as_secs());
                if !modified_secs.is_some_and(|secs| secs > cutoff) {
                    continue;
                }
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let mut turns = BTreeSet::new();
            for key in ["active", "redo", "pending"] {
                collect_turn_numbers(&value[key], &mut turns);
            }
            for turn in turns {
                // Legacy records predate session attribution: any live branch
                // keeps them, preserving pre-scoping retention behavior.
                protected.insert(SnapshotRecordId { turn, session_key: None });
                if let Some(key) = scoped_key.clone() {
                    protected.insert(SnapshotRecordId { turn, session_key: Some(key) });
                }
            }
        }
        protected
    }

    /// Cheap retention prune: enforce the snapshot count budget without
    /// reading checkpoint JSON bodies. Safe to call on the per-turn hot path
    /// (`native::begin_prompt`); age-based expiry stays in
    /// [`Self::cleanup_old_snapshots`], which needs each file's `created_at`.
    pub async fn prune_snapshot_budget(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let manager = self.clone();
        let entries = tokio::task::spawn_blocking(move || -> Result<Vec<SnapshotFileEntry>> {
            let protected = manager.protected_turns();
            Ok(manager
                .read_snapshot_files()?
                .into_iter()
                .filter(|entry| !protected.contains(&entry.id))
                .collect())
        })
        .await
        .context("checkpoint retention worker failed")??;
        if self.max_snapshots != 0 && entries.len() > self.max_snapshots {
            let excess = entries.len() - self.max_snapshots;
            for entry in entries.into_iter().take(excess) {
                if let Err(err) = self.retire_snapshot(&entry.path).await {
                    tracing::warn!(
                        path = %entry.path.display(),
                        error = %err,
                        "Failed to remove old checkpoint"
                    );
                }
            }
        }
        self.cleanup_retired_snapshots(false).await;
        Ok(())
    }

    /// Full retention: age-expire checkpoints past `max_age_days`, then apply
    /// the count budget. Age scan reads each candidate's `created_at`, so prefer
    /// [`Self::prune_snapshot_budget`] on hot paths.
    pub async fn cleanup_old_snapshots(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let protected = self.protected_turns();
        if let Some(cutoff) = self.retention_cutoff_secs()? {
            for entry in self.read_snapshot_files()? {
                if protected.contains(&entry.id) {
                    continue;
                }
                let data = match tokio::fs::read(&entry.path).await {
                    Ok(data) => data,
                    Err(err) => {
                        tracing::warn!(
                            path = %entry.path.display(),
                            error = %err,
                            "Failed to read checkpoint"
                        );
                        continue;
                    }
                };
                let stored: StoredSnapshot = match serde_json::from_slice(&data) {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::warn!(
                            path = %entry.path.display(),
                            error = %err,
                            "Failed to parse checkpoint"
                        );
                        continue;
                    }
                };
                if stored.metadata.created_at <= cutoff
                    && let Err(err) = self.retire_snapshot(&entry.path).await
                {
                    tracing::warn!(
                        path = %entry.path.display(),
                        error = %err,
                        "Failed to remove expired checkpoint"
                    );
                }
            }
            // Session lock files outlive crashed sessions; reclaim the ones no
            // live process can still be holding that are older than the
            // retention window. The shared cleanup mutex itself persists, and
            // locks that cannot be statted are conservatively kept.
            if let Ok(entries) = fs::read_dir(&self.storage_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                        continue;
                    };
                    if !name.starts_with("rewind-") || !name.ends_with(".lock") {
                        continue;
                    }
                    let stale = path
                        .metadata()
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                        .map(|since| since.as_secs())
                        .is_some_and(|secs| secs <= cutoff);
                    if stale {
                        remove_lock_file_if_unheld(&path);
                    }
                }
            }
        }

        self.prune_snapshot_budget().await?;
        self.cleanup_retired_snapshots(true).await;
        Ok(())
    }

    pub(super) async fn retire_snapshot(&self, path: &Path) -> Result<()> {
        let retired = path.with_extension(format!("retired-{}", uuid::Uuid::new_v4()));
        tokio::fs::rename(path, retired).await?;
        Ok(())
    }

    pub(super) async fn cleanup_retired_snapshots(&self, full_maintenance: bool) {
        let storage = self.storage_dir.clone();
        let workspace = self.canonical_workspace.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            // A replaced record can still be live if publication failed. Read
            // every live record before deleting anything; corrupt metadata defers
            // cleanup rather than guessing which content can be discarded.
            let mut live_paths = Vec::new();
            let mut retired = Vec::new();
            for entry in fs::read_dir(&storage)? {
                let path = entry?.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.starts_with("turn_") {
                    continue;
                }
                if name.ends_with(".json") {
                    live_paths.push(path);
                } else if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with("retired-"))
                {
                    retired.push(path);
                }
            }
            // The per-prompt path does not need live JSON or a content store
            // when no records were retired. Explicit maintenance still runs GC.
            // The mutex below is acquired only past this point so idle rounds
            // create no lock file on the hot path.
            if retired.is_empty() && !full_maintenance {
                return Ok(());
            }
            // Retention is best-effort and retryable: when another process is
            // already reclaiming checkpoint content, skip this round instead
            // of interleaving filesnap deletes with its scan.
            let _cleanup_guard = match acquire_verified_rewind_lock(&storage.join(WORKSPACE_CLEANUP_LOCK_FILE)) {
                Ok(guard) => guard,
                Err(error) => {
                    tracing::debug!(%error, "Checkpoint cleanup already running elsewhere; deferring round");
                    return Ok(());
                }
            };
            let mut live = BTreeSet::new();
            for path in live_paths {
                let stored: StoredSnapshot = serde_json::from_slice(&fs::read(path)?)?;
                for file in stored.files {
                    if file.encoding == Some(FileEncoding::Filesnap) {
                        live.insert(file.data.context("Missing checkpoint reference")?);
                    }
                }
            }
            let store = filesnap::WorkspaceStore::open(&storage, &workspace)?;
            for path in retired {
                let stored: StoredSnapshot = serde_json::from_slice(&fs::read(&path)?)?;
                let mut sessions = BTreeSet::new();
                for file in &stored.files {
                    if file.encoding == Some(FileEncoding::Filesnap) {
                        let session = file.data.as_deref().context("Missing retired checkpoint reference")?;
                        let id = session.strip_prefix("vt-").context("Invalid retired checkpoint reference")?;
                        uuid::Uuid::parse_str(id)?;
                        if !live.contains(session) {
                            sessions.insert(session.to_owned());
                        }
                    }
                }
                let outcome = store.delete_sessions(&sessions.into_iter().collect::<Vec<_>>());
                anyhow::ensure!(
                    outcome.refused.is_empty() && outcome.incomplete.is_empty(),
                    "Checkpoint cleanup is incomplete: {outcome:?}"
                );
                fs::remove_file(path)?;
            }
            filesnap::collect_garbage(&storage)?;
            Ok(())
        })
        .await;
        if !matches!(&result, Ok(Ok(()))) {
            tracing::warn!(?result, "Checkpoint content cleanup deferred; retired records remain retryable");
        }
    }

    fn retention_cutoff_secs(&self) -> Result<Option<u64>> {
        let Some(days) = self.max_age_days else {
            return Ok(None);
        };

        let now = Self::current_timestamp()?;
        if days == 0 {
            return Ok(Some(now));
        }

        let seconds = days.saturating_mul(SECONDS_PER_DAY);
        let cutoff_instant = SystemTime::now()
            .checked_sub(Duration::from_secs(seconds))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let cutoff = cutoff_instant
            .duration_since(UNIX_EPOCH)
            .context("system clock before UNIX_EPOCH")?
            .as_secs();
        Ok(Some(cutoff))
    }
}
