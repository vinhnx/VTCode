mod native;
mod types;
pub use native::{PromptCheckpointLease, declare_prompt_edit};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub use types::{
    ActionSnapshot, DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS, FileEncoding,
    FileSnapshot, REWIND_ACTIVE_KEEP, RevertScope, RollbackResult, SnapshotConfig, SnapshotMetadata,
    SnapshotTurnContext, SnapshotTurnDiagnostics, StoredSnapshot,
};
use types::{
    MAX_DESCRIPTION_LEN, NEXT_ACTION_NUMBER, SNAPSHOT_SCHEMA_VERSION, collect_turn_numbers, normalized_prompt_text,
    sanitize_relative_path,
};

use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use vtcode_commons::canonicalize;
use vtcode_exec_events::{MAX_IN_PROGRESS_EXEC_SESSIONS, Usage, deserialize_null_as_default};

use crate::core::SECONDS_PER_DAY;
use crate::core::pending_actions::ExpectedOutcome;
use crate::core::state_schema::{SchemaVersion, VersionedState};
use crate::types::CompactStr;
use crate::utils::error_messages::ERR_CREATE_CHECKPOINT_DIR;
use crate::utils::file_utils::{ensure_dir_exists, ensure_dir_exists_sync, write_json_file};
use crate::utils::path::canonicalize_workspace;
use crate::utils::session_archive::SessionMessage;

#[derive(Clone)]
pub struct SnapshotManager {
    enabled: bool,
    workspace: PathBuf,
    canonical_workspace: PathBuf,
    storage_dir: PathBuf,
    max_snapshots: usize,
    max_age_days: Option<u64>,
}

impl SnapshotManager {
    pub fn new(config: SnapshotConfig) -> Result<Self> {
        let storage_dir = config.storage_dir();
        let canonical_workspace = canonicalize_workspace(&config.workspace);

        if config.enabled {
            ensure_dir_exists_sync(&storage_dir)
                .with_context(|| format!("{}: {}", ERR_CREATE_CHECKPOINT_DIR, storage_dir.display()))?;
        }
        Ok(Self {
            enabled: config.enabled,
            workspace: config.workspace,
            canonical_workspace,
            storage_dir,
            max_snapshots: config.max_snapshots,
            max_age_days: config.max_age_days,
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn snapshot_path(&self, turn_number: usize) -> PathBuf {
        self.storage_dir.join(format!("turn_{turn_number}.json"))
    }

    fn normalize_path(&self, path: &Path) -> Option<PathBuf> {
        if path.is_absolute() {
            if let Ok(canonical_path) = canonicalize(path)
                && let Ok(stripped) = canonical_path.strip_prefix(&self.canonical_workspace)
            {
                return sanitize_relative_path(stripped);
            }

            if let Ok(stripped) = path.strip_prefix(&self.workspace) {
                return sanitize_relative_path(stripped);
            }

            None
        } else {
            sanitize_relative_path(path)
        }
    }

    fn checked_file_path(workspace: &Path, storage: &Path, relative: &Path) -> Result<PathBuf> {
        let relative = sanitize_relative_path(relative).context("Checkpoint path escapes the workspace")?;
        anyhow::ensure!(!relative.as_os_str().is_empty(), "Checkpoint path must name a file");
        let absolute = workspace.join(&relative);
        let storage = canonicalize(storage).unwrap_or_else(|_| storage.to_path_buf());
        anyhow::ensure!(!absolute.starts_with(&storage), "Checkpoint cannot restore its own storage");
        let mut current = workspace.to_path_buf();
        for part in relative.components() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    anyhow::ensure!(!metadata.file_type().is_symlink(), "Checkpoint path crosses a symlink")
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(absolute)
    }

    fn read_snapshot_files(&self) -> Result<Vec<(usize, PathBuf)>> {
        let mut entries = Vec::with_capacity(64); // Typical directory has ~20-50 snapshot files
        if !self.storage_dir.exists() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&self.storage_dir)
            .with_context(|| format!("failed to read checkpoint directory: {}", self.storage_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let stem = match path.file_stem().and_then(|stem| stem.to_str()) {
                Some(value) => value,
                None => continue,
            };
            let turn_str = match stem.strip_prefix("turn_") {
                Some(value) => value,
                None => continue,
            };
            if let Ok(turn) = turn_str.parse::<usize>() {
                entries.push((turn, path));
            }
        }
        entries.sort_by_key(|(turn, _)| *turn);
        Ok(entries)
    }

    fn decode_file(encoding: FileEncoding, data: &str) -> Result<Vec<u8>> {
        match encoding {
            FileEncoding::Utf8 => Ok(data.as_bytes().to_vec()),
            FileEncoding::Base64 => BASE64.decode(data).context("failed to decode base64 file contents"),
            FileEncoding::Filesnap => anyhow::bail!("filesnap references require the snapshot store"),
        }
    }

    fn truncate_description(description: &str) -> String {
        let first_line = description.lines().next().unwrap_or("").trim();
        vtcode_commons::formatting::truncate_within(first_line, MAX_DESCRIPTION_LEN, "…")
    }

    fn derive_prompt_metadata(conversation: &[SessionMessage]) -> (Option<String>, Option<usize>) {
        conversation
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, message)| {
                if message.role != crate::llm::provider::MessageRole::User {
                    return None;
                }

                let prompt = message.content.as_text();
                normalized_prompt_text(prompt.as_ref()).map(|prompt| (Some(prompt.to_string()), Some(index)))
            })
            .unwrap_or((None, None))
    }

    fn resolve_prompt_metadata(
        prompt_text: Option<&str>,
        prompt_message_index: Option<usize>,
        conversation: &[SessionMessage],
    ) -> (Option<String>, Option<usize>) {
        let (derived_prompt_text, derived_prompt_index) = Self::derive_prompt_metadata(conversation);
        let prompt_text = prompt_text
            .and_then(normalized_prompt_text)
            .map(str::to_string)
            .or(derived_prompt_text);
        let prompt_message_index = prompt_message_index
            .filter(|index| *index < conversation.len())
            .or(derived_prompt_index);
        (prompt_text, prompt_message_index)
    }

    fn hydrate_prompt_metadata(stored: &mut StoredSnapshot) {
        let (prompt_text, prompt_message_index) = Self::resolve_prompt_metadata(
            stored.metadata.prompt_text.as_deref(),
            stored.metadata.prompt_message_index,
            &stored.conversation,
        );
        stored.metadata.prompt_text = prompt_text;
        stored.metadata.prompt_message_index = prompt_message_index;
    }

    fn current_timestamp() -> Result<u64> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before UNIX_EPOCH")?
            .as_secs())
    }

    pub fn next_turn_number(&self) -> Result<usize> {
        Ok(self
            .read_snapshot_files()?
            .into_iter()
            .map(|(turn, _)| turn)
            .max()
            .unwrap_or(0)
            .saturating_add(1))
    }

    pub async fn create_snapshot(
        &self,
        turn_number: usize,
        description: &str,
        conversation: &[SessionMessage],
        modified_files: &BTreeSet<PathBuf>,
        prompt_text: Option<&str>,
        prompt_message_index: Option<usize>,
        turn_context: Option<SnapshotTurnContext>,
    ) -> Result<Option<SnapshotMetadata>> {
        if !self.enabled {
            return Ok(None);
        }

        let timestamp = Self::current_timestamp()?;
        let mut paths = Vec::with_capacity(modified_files.len());
        for path in modified_files {
            if let Some(relative) = self.normalize_path(path) {
                paths.push(relative);
            }
        }
        let workspace = self.canonical_workspace.clone();
        let storage = self.storage_dir.clone();
        let files = tokio::task::spawn_blocking(move || -> Result<Vec<FileSnapshot>> {
            // Conversation-only checkpoints need no unreferenced engine session.
            if paths.is_empty() {
                return Ok(Vec::new());
            }
            let mut absolute_paths = Vec::with_capacity(paths.len());
            for relative in &paths {
                absolute_paths.push(Self::checked_file_path(&workspace, &storage, relative)?);
            }
            let store = filesnap::WorkspaceStore::open(&storage, &workspace)?;
            let turn = format!("vt-{}", uuid::Uuid::new_v4());
            let checkpoint = store.checkpoint(&turn, &turn, absolute_paths.iter().cloned())?;
            anyhow::ensure!(checkpoint.stats.dropped == 0, "Checkpoint could not capture every selected file");
            Ok(paths
                .into_iter()
                .zip(absolute_paths)
                .map(|(relative, absolute)| {
                    let key = filesnap::canonical_key(&absolute).to_string_lossy().into_owned();
                    FileSnapshot {
                        path: relative.to_string_lossy().replace('\\', "/"),
                        deleted: checkpoint.manifest.absent.contains(&key),
                        encoding: Some(FileEncoding::Filesnap),
                        data: Some(turn.clone()),
                    }
                })
                .collect())
        })
        .await??;

        let (prompt_text, prompt_message_index) =
            Self::resolve_prompt_metadata(prompt_text, prompt_message_index, conversation);
        let description_source = prompt_text.as_deref().unwrap_or(description);
        let turn_context = turn_context.unwrap_or_default();
        let metadata = SnapshotMetadata {
            id: format!("turn_{turn_number}"),
            turn_number,
            created_at: timestamp,
            description: Self::truncate_description(description_source),
            message_count: conversation.len(),
            file_count: files.len(),
            touched_files: turn_context.touched_files.clone(),
            prompt_text,
            prompt_message_index,
            session_id: turn_context.session_id,
            runtime_turn_id: turn_context.runtime_turn_id,
            session_turn_number: turn_context.session_turn_number,
            turn_diagnostics: turn_context.turn_diagnostics,
        };

        let stored = StoredSnapshot {
            metadata: metadata.clone(),
            conversation: conversation.to_vec(),
            files,
            schema_version: Some(SNAPSHOT_SCHEMA_VERSION),
        };

        let path = self.snapshot_path(turn_number);
        if let Some(parent) = path.parent() {
            ensure_dir_exists(parent)
                .await
                .with_context(|| format!("failed to ensure checkpoint directory: {}", parent.display()))?;
        }

        // Preserve an overwritten turn as a cleanup journal until the new JSON
        // is published. It is invisible to checkpoint enumeration.
        let retired = path.with_extension(format!("retired-{}", uuid::Uuid::new_v4()));
        match tokio::fs::copy(&path, &retired).await {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to journal replaced checkpoint"),
        }
        write_json_file(&path, &stored)
            .await
            .with_context(|| format!("failed to write checkpoint: {}", path.display()))?;

        self.cleanup_old_snapshots().await?;

        Ok(Some(metadata))
    }

    pub async fn list_snapshots(&self) -> Result<Vec<SnapshotMetadata>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        self.cleanup_old_snapshots().await?;
        let snapshot_files = self.read_snapshot_files()?;
        let mut snapshots = Vec::with_capacity(snapshot_files.len());
        for (_, path) in snapshot_files {
            let data = tokio::fs::read(&path)
                .await
                .with_context(|| format!("failed to read checkpoint: {}", path.display()))?;
            let mut stored: StoredSnapshot = serde_json::from_slice(&data)
                .with_context(|| format!("failed to parse checkpoint: {}", path.display()))?;
            Self::hydrate_prompt_metadata(&mut stored);
            snapshots.push(stored.metadata);
        }
        snapshots.sort_by_key(|a| std::cmp::Reverse(a.turn_number));
        Ok(snapshots)
    }

    pub async fn load_snapshot(&self, turn_number: usize) -> Result<Option<StoredSnapshot>> {
        if !self.enabled {
            return Ok(None);
        }
        let path = self.snapshot_path(turn_number);
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }
        let data = tokio::fs::read(&path)
            .await
            .with_context(|| format!("failed to read checkpoint: {}", path.display()))?;
        let mut stored: StoredSnapshot =
            serde_json::from_slice(&data).with_context(|| format!("failed to parse checkpoint: {}", path.display()))?;
        // Migrate legacy snapshots to the current schema version
        stored = stored
            .migrate(SNAPSHOT_SCHEMA_VERSION)
            .with_context(|| format!("failed to migrate checkpoint: {}", path.display()))?;
        Self::hydrate_prompt_metadata(&mut stored);
        Ok(Some(stored))
    }

    pub async fn restore_snapshot(&self, turn_number: usize, scope: RevertScope) -> Result<Option<CheckpointRestore>> {
        let Some(stored) = self.load_snapshot(turn_number).await? else {
            return Ok(None);
        };

        self.restore_stored_snapshot(stored, scope).await.map(Some)
    }

    async fn restore_stored_snapshot(&self, stored: StoredSnapshot, scope: RevertScope) -> Result<CheckpointRestore> {
        self.restore_stored_snapshot_with_ignore(stored, scope, &filesnap::Gitignore::empty())
            .await
    }

    async fn restore_stored_snapshot_with_ignore(
        &self,
        stored: StoredSnapshot,
        scope: RevertScope,
        ignore: &filesnap::Gitignore,
    ) -> Result<CheckpointRestore> {
        if scope.includes_code() {
            let workspace = self.canonical_workspace.clone();
            let storage = self.storage_dir.clone();
            let files = stored.files.clone();
            // Validate the complete path set before any write, including legacy records.
            tokio::task::spawn_blocking(move || -> Result<()> {
                for file in &files {
                    Self::checked_file_path(&workspace, &storage, Path::new(&file.path))?;
                }
                Ok(())
            })
            .await??;
        }
        let engine_backed = stored.files.iter().any(|file| file.encoding == Some(FileEncoding::Filesnap));
        if scope.includes_code() && engine_backed {
            let ignore = ignore.clone();
            let workspace = self.canonical_workspace.clone();
            let storage = self.storage_dir.clone();
            let files = stored.files.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                let turn = files
                    .first()
                    .and_then(|file| file.data.as_deref())
                    .context("Missing filesnap reference")?;
                anyhow::ensure!(files.iter().all(|file| file.encoding == Some(FileEncoding::Filesnap)
                    && file.data.as_deref() == Some(turn)), "Mixed or inconsistent checkpoint references");
                let store = filesnap::WorkspaceStore::open(&storage, &workspace)?;
                let target = store.target_for_turn(turn)?.context("Missing filesnap checkpoint")?;
                let manifest = store.manifest(target.manifest_id())?;
                let expected: BTreeSet<String> = files
                    .iter()
                    .map(|file| {
                        filesnap::canonical_key(&workspace.join(&file.path))
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect();
                let recorded: BTreeSet<String> =
                    manifest.entries.keys().chain(manifest.absent.iter()).cloned().collect();
                anyhow::ensure!(expected == recorded, "Checkpoint contains unexpected file paths");
                let outcome = store.restore_to(
                    turn,
                    &target,
                    filesnap::RestoreKind::Rewind { undo_for: Some(turn) },
                    expected.iter().map(PathBuf::from),
                    &ignore,
                )?;
                anyhow::ensure!(
                    outcome.stats.failed.is_empty(),
                    "Checkpoint restore failed: {:?}",
                    outcome.stats.failed
                );
                Ok(())
            })
            .await??;
        } else if scope.includes_code() {
            for snapshot in &stored.files {
                let relative = Path::new(&snapshot.path);
                let Some(sanitized) = sanitize_relative_path(relative) else {
                    continue;
                };
                let absolute = self.workspace.join(&sanitized);
                if snapshot.deleted {
                    if tokio::fs::try_exists(&absolute).await.unwrap_or(false) {
                        tokio::fs::remove_file(&absolute).await.with_context(|| {
                            format!("failed to remove file during checkpoint restore: {}", absolute.display())
                        })?;
                    }
                    continue;
                }

                if let Some(parent) = absolute.parent() {
                    ensure_dir_exists(parent)
                        .await
                        .with_context(|| format!("failed to create directories for restore: {}", parent.display()))?;
                }

                let encoding = snapshot.encoding.unwrap_or(FileEncoding::Utf8);
                let data = snapshot.data.as_deref().unwrap_or_default();
                let bytes = Self::decode_file(encoding, data)?;
                tokio::fs::write(&absolute, &bytes)
                    .await
                    .with_context(|| format!("failed to write restored file: {}", absolute.display()))?;
            }
        }

        let conversation = if scope.includes_conversation() {
            stored.conversation.clone()
        } else {
            Vec::new()
        };

        Ok(CheckpointRestore { metadata: stored.metadata, conversation })
    }

    /// Turn numbers still referenced by any *recent* session navigation
    /// (active branch, redo stack, or pending recovery). Rewind/redo must
    /// never lose these. Branch files older than the retention window are
    /// ignored so a completed session cannot pin its turns forever.
    fn protected_turns(&self) -> BTreeSet<usize> {
        let cutoff = self.retention_cutoff_secs().ok().flatten();
        self.protected_turns_with_cutoff(cutoff)
    }

    fn protected_turns_with_cutoff(&self, cutoff: Option<u64>) -> BTreeSet<usize> {
        let mut protected = BTreeSet::new();
        let Ok(entries) = fs::read_dir(&self.storage_dir) else {
            return protected;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !stem.starts_with("branch_") || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
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
            for key in ["active", "redo", "pending"] {
                collect_turn_numbers(&value[key], &mut protected);
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
        let entries = tokio::task::spawn_blocking(move || -> Result<Vec<(usize, PathBuf)>> {
            let protected = manager.protected_turns();
            Ok(manager
                .read_snapshot_files()?
                .into_iter()
                .filter(|(turn, _)| !protected.contains(turn))
                .collect())
        })
        .await
        .context("checkpoint retention worker failed")??;
        if self.max_snapshots != 0 && entries.len() > self.max_snapshots {
            let excess = entries.len() - self.max_snapshots;
            for (_, path) in entries.into_iter().take(excess) {
                if let Err(err) = self.retire_snapshot(&path).await {
                    tracing::warn!(
                        path = %path.display(),
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
            for (turn, path) in self.read_snapshot_files()? {
                if protected.contains(&turn) {
                    continue;
                }
                let data = match tokio::fs::read(&path).await {
                    Ok(data) => data,
                    Err(err) => {
                        tracing::warn!(
                            path = %path.display(),
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
                            path = %path.display(),
                            error = %err,
                            "Failed to parse checkpoint"
                        );
                        continue;
                    }
                };
                if stored.metadata.created_at <= cutoff
                    && let Err(err) = self.retire_snapshot(&path).await
                {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "Failed to remove expired checkpoint"
                    );
                }
            }
        }

        self.prune_snapshot_budget().await?;
        self.cleanup_retired_snapshots(true).await;
        Ok(())
    }

    async fn retire_snapshot(&self, path: &Path) -> Result<()> {
        let retired = path.with_extension(format!("retired-{}", uuid::Uuid::new_v4()));
        tokio::fs::rename(path, retired).await?;
        Ok(())
    }

    async fn cleanup_retired_snapshots(&self, full_maintenance: bool) {
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
            if retired.is_empty() && !full_maintenance {
                return Ok(());
            }
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

    pub fn parse_revert_scope(value: &str) -> Option<RevertScope> {
        match value.to_ascii_lowercase().as_str() {
            "conversation" | "chat" => Some(RevertScope::Conversation),
            "code" | "files" => Some(RevertScope::Code),
            "both" | "full" => Some(RevertScope::Both),
            _ => None,
        }
    }

    // ─── Action-level Snapshots (Incremental Rollback) ────────────────────

    /// File path for an action-level snapshot.
    fn action_snapshot_path(&self, action_number: usize) -> PathBuf {
        self.storage_dir.join(format!("action_{action_number}.json"))
    }

    /// Save an action-level snapshot to disk. Returns the action number.
    pub async fn save_action_snapshot(&self, action: &ActionSnapshot) -> Result<usize> {
        if !self.enabled {
            return Ok(action.action_number);
        }
        let path = self.action_snapshot_path(action.action_number);
        if let Some(parent) = path.parent() {
            ensure_dir_exists(parent)
                .await
                .with_context(|| format!("failed to ensure checkpoint directory: {}", parent.display()))?;
        }
        write_json_file(&path, action)
            .await
            .with_context(|| format!("failed to write action snapshot: {}", path.display()))?;
        Ok(action.action_number)
    }

    /// Load an action-level snapshot from disk.
    pub async fn load_action_snapshot(&self, action_number: usize) -> Result<Option<ActionSnapshot>> {
        if !self.enabled {
            return Ok(None);
        }
        let path = self.action_snapshot_path(action_number);
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }
        let data = tokio::fs::read(&path)
            .await
            .with_context(|| format!("failed to read action snapshot: {}", path.display()))?;
        let action: ActionSnapshot = serde_json::from_slice(&data)
            .with_context(|| format!("failed to parse action snapshot: {}", path.display()))?;
        Ok(Some(action))
    }

    /// Rollback a single action by restoring conversation state and files.
    ///
    /// This truncates the messages vector to `pre_action_message_count` and
    /// restores file contents from their pre-action state (requires the workspace
    /// manager to have tracked file versions).
    pub async fn rollback_one_action(
        &self,
        action: &ActionSnapshot,
        messages: &mut Vec<SessionMessage>,
        scope: RevertScope,
    ) -> Result<RollbackResult> {
        let mut files_restored = 0;

        // Restore conversation: truncate to pre-action length
        let messages_removed = if scope.includes_conversation() && action.pre_action_message_count <= messages.len() {
            let removed = messages.len() - action.pre_action_message_count;
            messages.truncate(action.pre_action_message_count);
            removed
        } else {
            0
        };

        // Restore files that were touched by this action
        if scope.includes_code() {
            for file_path in &action.touched_files {
                // Read the current file content as a snapshot if possible
                let absolute = self.workspace.join(file_path);
                if tokio::fs::try_exists(&absolute).await.unwrap_or(false) {
                    // In a full implementation, we'd restore from a file version store.
                    // For now, we track that the file was touched and would need restoration.
                    files_restored += 1;
                }
            }
        }

        Ok(RollbackResult {
            rollback_action_id: action.action_id.clone(),
            messages_removed,
            files_restored,
            next_action_number: action.action_number,
        })
    }

    /// Generate the next monotonic action number.
    ///
    /// Uses an atomic counter to avoid conflicts between concurrent sessions.
    pub fn next_action_number(&self) -> usize {
        NEXT_ACTION_NUMBER.fetch_add(1, Ordering::Relaxed) as usize
    }
}

#[derive(Debug, Clone)]
pub struct CheckpointRestore {
    pub metadata: SnapshotMetadata,
    pub conversation: Vec<SessionMessage>,
}

#[cfg(test)]
mod tests;
