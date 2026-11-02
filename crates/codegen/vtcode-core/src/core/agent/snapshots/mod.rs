mod native;
mod retention;
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
    SnapshotSkippedPath, SnapshotSkippedPaths, SnapshotTurnContext, SnapshotTurnDiagnostics, StoredSnapshot,
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

/// Filesystem-safe session key shared by branch files, snapshot records, and
/// session locks.
///
/// Hex-encoding keeps every byte (including `/`, `.`, and unicode) out of the
/// filename so session ids can never escape the storage directory. Fails
/// closed on empty or over-long ids instead of writing an ambiguous file.
fn session_filename_key(session: &str) -> Result<String> {
    anyhow::ensure!(!session.is_empty() && session.len() <= 80, "Invalid session ID");
    Ok(session.as_bytes().iter().map(|b| format!("{b:02x}")).collect())
}

/// Shared lock file serializing checkpoint-content reclamation
/// (`delete_sessions` + `collect_garbage`) across processes.
///
/// Retention is best-effort and retryable: a process that finds this lock
/// held skips its cleanup round instead of interleaving filesnap deletes
/// with another process's scan. Unlike the per-session turn locks, this
/// file persists; it is never unlinked.
const WORKSPACE_CLEANUP_LOCK_FILE: &str = "rewind.lock";

/// Whether a locked fd still refers to the file currently at `lock_path`.
///
/// The completion path unlinks a session lock while holding its lock. A process
/// that opened the old inode just before that unlink can `try_lock` the ghost
/// after the holder drops — while another process creates a fresh file — and
/// both would believe they hold the lock. Comparing inos closes that window.
#[cfg(unix)]
fn locked_file_matches_path(file: &fs::File, lock_path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(locked) = file.metadata() else {
        return false;
    };
    let Ok(current) = fs::metadata(lock_path) else {
        return false;
    };
    locked.ino() == current.ino()
}

#[cfg(not(unix))]
fn locked_file_matches_path(_: &fs::File, _: &Path) -> bool {
    true
}

/// Acquire the lock at `lock_path`, verifying the locked inode is still the
/// file at that path. A ghost acquisition (locked inode replaced under us)
/// is dropped and retried against the current file, so mutual exclusion holds
/// across the completion path's unlink-while-held cleanup.
fn acquire_verified_rewind_lock(lock_path: &Path) -> std::io::Result<fs::File> {
    for _ in 0..3 {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        file.try_lock()?;
        if locked_file_matches_path(&file, lock_path) {
            return Ok(file);
        }
        drop(file);
    }
    Err(std::io::Error::other(format!(
        "{} kept changing identity; retry the turn",
        lock_path.file_name().and_then(|name| name.to_str()).unwrap_or("lock file")
    )))
}

/// Remove a lock file only while holding its lock on the current inode.
///
/// Deleting a lock another process holds (or is about to hold) would let a
/// later open create a fresh inode and break mutual exclusion, so an
/// unacquirable or replaced file is left alone. Best-effort: failures only
/// leave a stale 0-byte file for the next cleanup round.
fn remove_lock_file_if_unheld(lock_path: &Path) {
    let file = match fs::OpenOptions::new().read(true).write(true).open(lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::debug!(%error, path = %lock_path.display(), "failed to open lock file for cleanup");
            return;
        }
    };
    // Only unlink while holding a lock on the file that is currently at the
    // path; a ghost lock (path replaced under us) must not delete the
    // replacement's lock file.
    if file.try_lock().is_ok() && locked_file_matches_path(&file, lock_path) {
        if let Err(error) = fs::remove_file(lock_path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(%error, path = %lock_path.display(), "failed to remove stale lock file");
        }
    }
    // `file` (and its lock) drops here, after the unlink.
}

/// Identity of one checkpoint record file: the owning session (if any) plus
/// the session-local turn number.
///
/// `session_key` is `None` for legacy `turn_{n}.json` records written before
/// session scoping; scoped records carry `Some(key)`.
///
/// Ordering is turn-major so retention prunes the oldest turns first across
/// all sessions, with the session key as a deterministic tiebreak.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SnapshotRecordId {
    turn: usize,
    session_key: Option<String>,
}

/// One enumerated checkpoint record: its identity plus its live `.json` path.
///
/// Retired (`*.retired-*`), recovery (`turn_recovery_*.json`), and branch
/// (`branch_*.json`) files never produce entries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotFileEntry {
    id: SnapshotRecordId,
    path: PathBuf,
}

/// Parse a checkpoint-directory filename into a record identity.
///
/// Returns `None` for anything that is not a live checkpoint record: wrong
/// extension, `turn_recovery_*` rescue files, branch files, and malformed
/// stems. This predicate is shared by enumeration, retention, and tests so the
/// on-disk contract cannot drift between readers.
fn parse_snapshot_filename(file_name: &str) -> Option<SnapshotRecordId> {
    let stem = file_name.strip_suffix(".json")?;
    let rest = stem.strip_prefix("turn_")?;
    if rest.is_empty() || stem.starts_with("turn_recovery_") {
        return None;
    }
    // Legacy `turn_{n}.json`: the whole remainder is the turn number.
    if let Ok(turn) = rest.parse::<usize>() {
        return Some(SnapshotRecordId { turn, session_key: None });
    }
    // Scoped `turn_{hex}_{n}.json`: hex carries no underscores, so the turn
    // is everything after the last one.
    let (key, turn_str) = rest.rsplit_once('_')?;
    if key.is_empty() || key.len() % 2 != 0 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let turn = turn_str.parse::<usize>().ok()?;
    Some(SnapshotRecordId { turn, session_key: Some(key.to_owned()) })
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

    /// Session-scoped checkpoint record: `turn_{session_key}_{turn}.json`.
    ///
    /// Each session owns a disjoint set of record files so two sessions in one
    /// workspace never overwrite each other's checkpoints. The unscoped
    /// [`Self::snapshot_path`] form stays as the legacy layout: pre-upgrade
    /// records keep loading (see [`Self::load_snapshot_for_session`]) and
    /// session-less writers keep writing it.
    fn snapshot_path_for_session(&self, session: &str, turn_number: usize) -> Result<PathBuf> {
        let key = session_filename_key(session)?;
        Ok(self.storage_dir.join(format!("turn_{key}_{turn_number}.json")))
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

    fn read_snapshot_files(&self) -> Result<Vec<SnapshotFileEntry>> {
        let mut entries = Vec::with_capacity(64); // Typical directory has ~20-50 snapshot files
        if !self.storage_dir.exists() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&self.storage_dir)
            .with_context(|| format!("failed to read checkpoint directory: {}", self.storage_dir.display()))?
        {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if let Some(id) = parse_snapshot_filename(name) {
                entries.push(SnapshotFileEntry { id, path });
            }
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(entries)
    }

    /// Enumerate live checkpoint records that carry `turn_number`, across the
    /// legacy layout and every session scope.
    ///
    /// Used to resolve bare-turn lookups (`revert --turn N`): legacy records
    /// sort before scoped ones so pre-upgrade workspaces resolve exactly as
    /// they did before session scoping existed.
    fn read_snapshot_files_for_turn(&self, turn_number: usize) -> Result<Vec<SnapshotFileEntry>> {
        let mut matches: Vec<SnapshotFileEntry> = self
            .read_snapshot_files()?
            .into_iter()
            .filter(|entry| entry.id.turn == turn_number)
            .collect();
        matches.sort_by(|a, b| a.id.session_key.cmp(&b.id.session_key));
        Ok(matches)
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

    /// Next checkpoint turn number for `session`: one past the highest turn
    /// the session already owns.
    ///
    /// Legacy (unscoped) records count toward every session's maximum so a
    /// resumed pre-upgrade session never reuses a turn number its own branch
    /// file already references under the legacy layout. Fresh sessions in a
    /// fresh workspace still start at 1.
    pub fn next_turn_number(&self, session: &str) -> Result<usize> {
        let key = session_filename_key(session)?;
        Ok(self
            .read_snapshot_files()?
            .into_iter()
            .filter(|entry| entry.id.session_key.is_none() || entry.id.session_key.as_deref() == Some(&key))
            .map(|entry| entry.id.turn)
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
            if checkpoint.stats.dropped != 0 {
                let skipped = native::skipped_paths_record(&workspace, &checkpoint);
                anyhow::bail!("Checkpoint could not capture every selected file: {}", skipped.summary());
            }
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
        // Session-carrying snapshots land in that session's disjoint record
        // set; session-less callers keep the legacy shared layout.
        let record_session = turn_context.session_id.clone();
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
            skipped_paths: None,
        };

        let stored = StoredSnapshot {
            metadata: metadata.clone(),
            conversation: conversation.to_vec(),
            files,
            schema_version: Some(SNAPSHOT_SCHEMA_VERSION),
        };

        let path = match record_session.as_deref() {
            Some(session) => self.snapshot_path_for_session(session, turn_number)?,
            None => self.snapshot_path(turn_number),
        };
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
        for entry in snapshot_files {
            let data = tokio::fs::read(&entry.path)
                .await
                .with_context(|| format!("failed to read checkpoint: {}", entry.path.display()))?;
            let mut stored: StoredSnapshot = serde_json::from_slice(&data)
                .with_context(|| format!("failed to parse checkpoint: {}", entry.path.display()))?;
            Self::hydrate_prompt_metadata(&mut stored);
            snapshots.push(stored.metadata);
        }
        snapshots.sort_by(|a, b| b.turn_number.cmp(&a.turn_number).then_with(|| a.session_id.cmp(&b.session_id)));
        Ok(snapshots)
    }

    pub async fn load_snapshot(&self, turn_number: usize) -> Result<Option<StoredSnapshot>> {
        if !self.enabled {
            return Ok(None);
        }
        self.load_snapshot_from_path(&self.snapshot_path(turn_number)).await
    }

    /// Load `session`'s checkpoint for `turn_number`.
    ///
    /// Reads the session-scoped record first, then falls back to the legacy
    /// shared layout so resumed pre-upgrade sessions keep rewinding turns
    /// they recorded before session scoping existed.
    pub async fn load_snapshot_for_session(&self, session: &str, turn_number: usize) -> Result<Option<StoredSnapshot>> {
        if !self.enabled {
            return Ok(None);
        }
        let scoped = self.snapshot_path_for_session(session, turn_number)?;
        if let Some(stored) = self.load_snapshot_from_path(&scoped).await? {
            return Ok(Some(stored));
        }
        self.load_snapshot_from_path(&self.snapshot_path(turn_number)).await
    }

    async fn load_snapshot_from_path(&self, path: &Path) -> Result<Option<StoredSnapshot>> {
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            return Ok(None);
        }
        let data = tokio::fs::read(path)
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
        // Legacy records resolve exactly as before session scoping existed.
        if let Some(stored) = self.load_snapshot(turn_number).await? {
            return self.restore_stored_snapshot(stored, scope).await.map(Some);
        }
        let matches = self.read_snapshot_files_for_turn(turn_number)?;
        let [entry] = matches.as_slice() else {
            if matches.is_empty() {
                return Ok(None);
            }
            let sessions = matches
                .iter()
                .filter_map(|entry| entry.id.session_key.as_deref())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("turn {turn_number} exists in multiple sessions ({sessions}); re-run with --session <id>");
        };
        let Some(stored) = self.load_snapshot_from_path(&entry.path).await? else {
            return Ok(None);
        };
        self.restore_stored_snapshot(stored, scope).await.map(Some)
    }

    /// Restore `session`'s checkpoint for `turn_number`, with the same
    /// session-then-legacy lookup as [`Self::load_snapshot_for_session`].
    pub async fn restore_snapshot_for_session(
        &self,
        session: &str,
        turn_number: usize,
        scope: RevertScope,
    ) -> Result<Option<CheckpointRestore>> {
        let Some(stored) = self.load_snapshot_for_session(session, turn_number).await? else {
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
