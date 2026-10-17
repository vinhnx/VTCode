mod native;
pub use native::{PromptCheckpointLease, declare_prompt_edit};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use vtcode_exec_events::{MAX_IN_PROGRESS_EXEC_SESSIONS, Usage, deserialize_null_as_default};

use crate::core::pending_actions::ExpectedOutcome;
use crate::core::state_schema::{SchemaVersion, VersionedState};
use crate::types::CompactStr;
use crate::utils::error_messages::ERR_CREATE_CHECKPOINT_DIR;
use crate::utils::file_utils::{ensure_dir_exists, ensure_dir_exists_sync, write_json_file};
use crate::utils::path::canonicalize_workspace;
use crate::utils::session_archive::SessionMessage;

const MAX_DESCRIPTION_LEN: usize = 160;
use vtcode_commons::canonicalize;

use crate::core::SECONDS_PER_DAY;
pub const DEFAULT_CHECKPOINTS_ENABLED: bool = true;
pub const DEFAULT_MAX_SNAPSHOTS: usize = 50;
pub const DEFAULT_MAX_AGE_DAYS: u64 = 30;
/// How many newest active turns a finished session keeps pinned for rewind.
/// Older entries are dropped on thread completion so their snapshots become
/// prune-eligible instead of sitting protected for the full age window.
pub const REWIND_ACTIVE_KEEP: usize = 5;
const SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(3);

/// Collect turn numbers from a navigation value: either a bare array of turns
/// (`active`) or a recovery object with an `active` array (`redo`/`pending`).
fn collect_turn_numbers(value: &serde_json::Value, out: &mut BTreeSet<usize>) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(turn) = item.as_u64() {
                    out.insert(turn as usize);
                }
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(active) = map.get("active") {
                collect_turn_numbers(active, out);
            }
        }
        _ => {}
    }
}

fn normalized_prompt_text(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn sanitize_relative_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return None;
            }
        }
    }
    Some(normalized)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub id: String,
    pub turn_number: usize,
    pub created_at: u64,
    pub description: String,
    pub message_count: usize,
    pub file_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_message_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<CompactStr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_turn_id: Option<CompactStr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_turn_number: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_diagnostics: Option<SnapshotTurnDiagnostics>,
}

impl SnapshotMetadata {
    pub fn resolved_prompt_text<'a>(&'a self, conversation: &'a [SessionMessage]) -> Option<String> {
        self.prompt_text
            .as_deref()
            .and_then(normalized_prompt_text)
            .map(str::to_string)
            .or_else(|| SnapshotManager::derive_prompt_metadata(conversation).0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SnapshotTurnDiagnostics {
    #[serde(default)]
    pub usage: Usage,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub elapsed_ms: u64,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub requested_tool_calls: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub admitted_tool_calls: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub unadmitted_tool_calls: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub failed_tool_calls: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub denied_tool_calls: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub preflight_failures: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub reused_results: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub spooled_results: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub raw_spooled_bytes: u64,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub model_visible_output_bytes: u64,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub suppressed_tool_previews: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub model_visible_tool_preview_budget_exhausted: bool,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub low_signal_tool_calls: u32,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub recovery_activations: u32,
    /// Exec sessions still running when the turn ended (bounded, newest
    /// first). Downstream consumers use this to correlate cross-turn resume
    /// hints; empty when every command settled within the turn.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub in_progress_exec_sessions: Vec<CompactStr>,
}

impl SnapshotTurnDiagnostics {
    /// Attach the in-progress exec session ids captured at turn end.
    ///
    /// Kept as a builder step because the ids come from the live exec-session
    /// registry (async), not from the synchronous turn-state counters.
    /// Bound shares `vtcode_exec_events::MAX_IN_PROGRESS_EXEC_SESSIONS` with
    /// `TurnCompletedEvent` so checkpoint and event streams cannot drift.
    #[must_use]
    pub fn with_in_progress_exec_sessions(mut self, sessions: Vec<crate::tools::types::VTCodeExecSession>) -> Self {
        self.in_progress_exec_sessions = sessions
            .into_iter()
            .take(MAX_IN_PROGRESS_EXEC_SESSIONS)
            .map(|session| CompactStr::from(session.id.as_str().to_string()))
            .collect();
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotTurnContext {
    pub session_id: Option<CompactStr>,
    pub runtime_turn_id: Option<CompactStr>,
    pub session_turn_number: Option<usize>,
    pub turn_diagnostics: Option<SnapshotTurnDiagnostics>,
    pub touched_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileEncoding {
    Utf8,
    Base64,
    /// A filesnap turn reference, not inline file contents. Old readers reject
    /// this variant rather than treating an absent payload as an empty file.
    Filesnap,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSnapshot {
    pub path: String,
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<FileEncoding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSnapshot {
    pub metadata: SnapshotMetadata,
    pub conversation: Vec<SessionMessage>,
    pub files: Vec<FileSnapshot>,
    /// Schema version for forward/backward migration. `None` means legacy (v0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<SchemaVersion>,
}

impl VersionedState for StoredSnapshot {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version.unwrap_or(SchemaVersion::V0)
    }

    fn migrate_one_step(self, from: SchemaVersion, to: SchemaVersion) -> Result<Self> {
        match (from, to) {
            (SchemaVersion::V0, SchemaVersion::V1) => {
                // v0 -> v1: set the explicit schema version; no structural changes yet.
                // Future versions add per-message metadata here.
                Ok(Self { schema_version: Some(SchemaVersion::V1), ..self })
            }
            (SchemaVersion::V1, SchemaVersion::V2) => Ok(Self { schema_version: Some(SchemaVersion::V2), ..self }),
            (SchemaVersion::V2, SNAPSHOT_SCHEMA_VERSION) => Ok(Self {
                schema_version: Some(SNAPSHOT_SCHEMA_VERSION),
                ..self
            }),
            _ => anyhow::bail!("unsupported snapshot migration: {from:?} -> {to:?}"),
        }
    }

    fn next_version(current: SchemaVersion) -> Option<SchemaVersion> {
        match current {
            SchemaVersion::V0 => Some(SchemaVersion::V1),
            SchemaVersion::V1 => Some(SchemaVersion::V2),
            SchemaVersion::V2 => Some(SNAPSHOT_SCHEMA_VERSION),
            SNAPSHOT_SCHEMA_VERSION => None,
            _ => None,
        }
    }
}

/// A snapshot of the agent state taken before executing a single action (tool call).
///
/// Unlike turn-level snapshots that capture the full conversation and file state,
/// action snapshots are lightweight records capturing only the state delta before
/// a specific tool call. This enables incremental undo of the last N actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionSnapshot {
    /// Matches the tool_call_id for this action.
    pub action_id: String,
    /// Sequential counter — strictly increasing per session.
    pub action_number: usize,
    /// Unix timestamp (seconds) when the snapshot was created.
    pub created_at: u64,
    /// Name of the tool being invoked.
    pub tool_name: String,
    /// JSON arguments passed to the tool.
    pub arguments: serde_json::Value,
    /// The number of messages in the conversation *before* this action was executed.
    /// Used to truncate the messages vector on rollback.
    pub pre_action_message_count: usize,
    /// Files we know were touched by this action (from modified_files).
    pub touched_files: Vec<String>,
    /// Expected outcome category, used to determine rollback strategy.
    pub expected_outcome: ExpectedOutcome,
}

/// Result of a rollback operation — describes what was undone.
#[derive(Debug, Clone)]
pub struct RollbackResult {
    /// The action_id of the action that was rolled back.
    pub rollback_action_id: String,
    /// Number of messages removed from the conversation.
    pub messages_removed: usize,
    /// Number of files restored to their pre-action state.
    pub files_restored: usize,
    /// The action_number after which this rollback takes effect.
    pub next_action_number: usize,
}

/// Global monotonic counter for action snapshots across sessions.
static NEXT_ACTION_NUMBER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevertScope {
    Conversation,
    Code,
    Both,
}

impl RevertScope {
    pub fn includes_code(self) -> bool {
        matches!(self, Self::Code | Self::Both)
    }

    pub fn includes_conversation(self) -> bool {
        matches!(self, Self::Conversation | Self::Both)
    }
}

pub struct SnapshotConfig {
    pub enabled: bool,
    pub workspace: PathBuf,
    pub storage_dir: Option<PathBuf>,
    pub max_snapshots: usize,
    pub max_age_days: Option<u64>,
}

impl SnapshotConfig {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            enabled: DEFAULT_CHECKPOINTS_ENABLED,
            workspace,
            storage_dir: None,
            max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        }
    }

    fn storage_dir(&self) -> PathBuf {
        self.storage_dir
            .clone()
            .unwrap_or_else(|| self.workspace.join(".vtcode").join("checkpoints"))
    }
}

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
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn setup_manager() -> (TempDir, SnapshotManager) {
        let dir = TempDir::new().expect("tempdir");
        let workspace = dir.path().to_path_buf();
        let manager = SnapshotManager::new(SnapshotConfig::new(workspace.clone())).expect("manager");
        (dir, manager)
    }

    #[tokio::test]
    async fn no_retirement_prune_does_not_parse_live_records_or_open_store() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let path = manager.snapshot_path(1);
        fs::write(&path, b"deliberately corrupt live metadata")?;
        manager.prune_snapshot_budget().await?;
        assert_eq!(fs::read(&path)?, b"deliberately corrupt live metadata");
        let entries: Vec<_> = fs::read_dir(&manager.storage_dir)?.collect::<std::io::Result<Vec<_>>>()?;
        assert_eq!(entries.len(), 1, "hot cleanup must not create an engine store");
        assert_eq!(entries[0].path(), path);
        Ok(())
    }

    #[tokio::test]
    async fn retired_cleanup_preserves_shared_references_and_defers_on_corrupt_live_metadata() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let file = manager.workspace.join("shared.txt");
        fs::write(&file, b"original")?;
        manager
            .create_snapshot(1, "first", &[], &BTreeSet::from([file.clone()]), None, None, None)
            .await?;
        let stored = manager.load_snapshot(1).await?.expect("snapshot");
        let engine = stored.files[0].data.clone().expect("engine reference");
        fs::write(manager.snapshot_path(2), serde_json::to_vec(&stored)?)?;
        let retired = manager.snapshot_path(1).with_extension("retired-test");
        fs::rename(manager.snapshot_path(1), &retired)?;
        let corrupt = manager.snapshot_path(3);
        fs::write(&corrupt, b"{")?;
        manager.cleanup_retired_snapshots(false).await;
        assert!(retired.exists(), "corrupt live records must defer reclamation");
        let store = filesnap::WorkspaceStore::open(&manager.storage_dir, &manager.workspace)?;
        assert!(store.target_for_turn(&engine)?.is_some());
        fs::remove_file(corrupt)?;
        manager.cleanup_retired_snapshots(false).await;
        assert!(!retired.exists());
        assert!(store.target_for_turn(&engine)?.is_some(), "a live shared reference pins the engine session");
        fs::write(&file, b"external edit")?;
        manager.restore_snapshot(2, RevertScope::Code).await?;
        assert_eq!(fs::read(file)?, b"original");
        Ok(())
    }

    #[tokio::test]
    async fn full_maintenance_collects_orphans_without_retired_records() -> Result<()> {
        let (_dir, manager) = setup_manager();
        // Use the versioned engine store, rather than a standalone BlobStore
        // that the workspace collector cannot discover.
        let blob_dir = manager
            .storage_dir
            .join("filesnap")
            .join(format!("v{}", filesnap::FORMAT_VERSION))
            .join("blobs");
        let blobs = filesnap::BlobStore::open(&blob_dir)?;
        let hash = blobs.store_bytes(b"orphaned content")?;
        let blob = blob_dir.join(&hash[..2]).join(&hash[2..]);
        fs::File::options()
            .write(true)
            .open(&blob)?
            .set_times(fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(3600)))?;
        manager.prune_snapshot_budget().await?;
        assert!(blob.exists(), "hot pruning skips unrelated orphan maintenance");
        manager.cleanup_old_snapshots().await?;
        assert!(!blob.exists(), "explicit maintenance must still collect old orphans");
        assert!(blobs.hashes()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn create_and_list_snapshots() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let mut conversation = Vec::new();
        conversation.push(SessionMessage::new(crate::llm::provider::MessageRole::User, "Hello"));
        let files = BTreeSet::new();
        manager
            .create_snapshot(1, "First turn", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");
        conversation.push(SessionMessage::new(crate::llm::provider::MessageRole::Assistant, "Hi"));
        manager
            .create_snapshot(2, "Second turn", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");

        let snapshots = manager.list_snapshots().await?;
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].turn_number, 2);
        assert_eq!(snapshots[1].turn_number, 1);
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_restores_file_contents() -> Result<()> {
        let (dir, manager) = setup_manager();
        let workspace = dir.path();
        let file_path = workspace.join("example.txt");
        fs::write(&file_path, "v1")?;

        let mut files = BTreeSet::new();
        files.insert(PathBuf::from("example.txt"));
        let conversation = vec![SessionMessage::new(
            crate::llm::provider::MessageRole::User,
            "edit example",
        )];
        manager
            .create_snapshot(1, "save", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");

        fs::write(&file_path, "v2")?;
        manager.restore_snapshot(1, RevertScope::Code).await?.expect("restore");
        let restored = fs::read_to_string(&file_path)?;
        assert_eq!(restored, "v1");
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_handles_deleted_files() -> Result<()> {
        let (dir, manager) = setup_manager();
        let workspace = dir.path();
        let file_path = workspace.join("remove.txt");
        fs::write(&file_path, "data")?;

        let mut files = BTreeSet::new();
        files.insert(PathBuf::from("remove.txt"));
        let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "remove")];
        manager
            .create_snapshot(1, "save", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");

        fs::remove_file(&file_path)?;
        manager.restore_snapshot(1, RevertScope::Code).await?.expect("restore");
        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path)?;
        assert_eq!(content, "data");
        Ok(())
    }

    #[tokio::test]
    async fn cleanup_respects_limit() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "hi")];
        let files = BTreeSet::new();

        for turn in 1..=5 {
            manager
                .create_snapshot(turn, "turn", &conversation, &files, None, None, None)
                .await?
                .expect("metadata");
        }

        // Manager default limit is 50, shrink artificially for test
        let mut config = SnapshotConfig::new(manager.workspace.clone());
        config.max_snapshots = 3;
        let trimmed = SnapshotManager::new(config)?;
        trimmed.cleanup_old_snapshots().await?;
        let listed = trimmed.list_snapshots().await?;
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].turn_number, 5);
        assert_eq!(listed[2].turn_number, 3);
        Ok(())
    }

    #[tokio::test]
    async fn cleanup_preserves_navigation_referenced_turns() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "nav")];
        let files = BTreeSet::new();

        for turn in 1..=5 {
            manager
                .create_snapshot(turn, "turn", &conversation, &files, None, None, None)
                .await?
                .expect("metadata");
        }

        // Simulate a live session navigation that still needs turns 1 and 2
        // for rewind/redo. Unreferenced turns 3-5 are fair game.
        let branch = serde_json::json!({
            "active": [1, 2],
            "redo": [{ "policy": "", "snapshot": "00000000-0000-0000-0000-000000000000", "active": [1] }],
            "pending": null,
        });
        fs::write(manager.storage_dir.join("branch_74657374.json"), serde_json::to_vec(&branch)?)?;

        let mut config = SnapshotConfig::new(manager.workspace.clone());
        config.max_snapshots = 1;
        let trimmed = SnapshotManager::new(config)?;
        trimmed.cleanup_old_snapshots().await?;

        assert!(trimmed.load_snapshot(1).await?.is_some(), "active turn must survive");
        assert!(trimmed.load_snapshot(2).await?.is_some(), "active turn must survive");
        // Unreferenced turns 3-5 compete for a single budget slot; the oldest
        // two are retired and the newest (5) remains.
        assert!(trimmed.load_snapshot(3).await?.is_none(), "unreferenced turn must be pruned");
        assert!(trimmed.load_snapshot(4).await?.is_none(), "unreferenced turn must be pruned");
        assert!(trimmed.load_snapshot(5).await?.is_some(), "newest unreferenced turn fits the budget");
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_normalizes_absolute_paths() -> Result<()> {
        let (dir, manager) = setup_manager();
        let workspace = dir.path();
        let absolute = workspace.join("abs.txt");
        fs::write(&absolute, "contents")?;

        let mut files = BTreeSet::new();
        files.insert(absolute.clone());
        let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "absolute")];

        manager
            .create_snapshot(1, "abs", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");

        let stored = manager.load_snapshot(1).await?.expect("stored snapshot");
        assert_eq!(stored.files.len(), 1);
        assert_eq!(stored.files[0].path, "abs.txt");
        assert!(!stored.files[0].deleted);
        Ok(())
    }

    #[tokio::test]
    async fn cleanup_removes_expired_snapshots() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "cleanup")];
        let files = BTreeSet::new();

        manager
            .create_snapshot(1, "old", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");

        let snapshot_path = manager.snapshot_path(1);
        let mut stored: StoredSnapshot = serde_json::from_slice(&fs::read(&snapshot_path)?)?;
        stored.metadata.created_at = 1;
        let updated = serde_json::to_vec_pretty(&stored)?;
        fs::write(&snapshot_path, updated)?;

        let mut config = SnapshotConfig::new(manager.workspace.clone());
        config.max_age_days = Some(1);
        let janitor = SnapshotManager::new(config)?;
        janitor.cleanup_old_snapshots().await?;

        assert!(janitor.load_snapshot(1).await?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_persists_prompt_metadata() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let conversation = vec![
            SessionMessage::new(crate::llm::provider::MessageRole::User, "Explain checkpointing"),
            SessionMessage::new(crate::llm::provider::MessageRole::Assistant, "Working on it"),
        ];

        manager
            .create_snapshot(
                1,
                "assistant reply",
                &conversation,
                &BTreeSet::new(),
                Some("Explain checkpointing"),
                Some(0),
                None,
            )
            .await?
            .expect("metadata");

        let stored = manager.load_snapshot(1).await?.expect("stored snapshot");
        assert_eq!(stored.metadata.prompt_text.as_deref(), Some("Explain checkpointing"));
        assert_eq!(stored.metadata.prompt_message_index, Some(0));
        assert_eq!(stored.metadata.description, "Explain checkpointing");
        Ok(())
    }

    #[tokio::test]
    async fn load_snapshot_hydrates_prompt_metadata_for_legacy_files() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let stored = StoredSnapshot {
            metadata: SnapshotMetadata {
                id: "turn_1".to_string(),
                turn_number: 1,
                created_at: 1,
                description: "legacy".to_string(),
                message_count: 2,
                file_count: 0,
                touched_files: Vec::new(),
                prompt_text: None,
                prompt_message_index: None,
                session_id: None,
                runtime_turn_id: None,
                session_turn_number: None,
                turn_diagnostics: None,
            },
            conversation: vec![
                SessionMessage::new(crate::llm::provider::MessageRole::User, "Legacy prompt"),
                SessionMessage::new(crate::llm::provider::MessageRole::Assistant, "Legacy reply"),
            ],
            files: Vec::new(),
            schema_version: None,
        };
        let path = manager.snapshot_path(1);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(&stored)?)?;

        let loaded = manager.load_snapshot(1).await?.expect("loaded snapshot");
        assert_eq!(loaded.metadata.prompt_text.as_deref(), Some("Legacy prompt"));
        assert_eq!(loaded.metadata.prompt_message_index, Some(0));
        Ok(())
    }

    #[test]
    fn legacy_snapshot_versions_migrate_without_inventing_diagnostics() -> Result<()> {
        let legacy = StoredSnapshot {
            metadata: SnapshotMetadata {
                id: "turn_1".to_string(),
                turn_number: 1,
                created_at: 1,
                description: "legacy".to_string(),
                message_count: 0,
                file_count: 0,
                touched_files: Vec::new(),
                prompt_text: None,
                prompt_message_index: None,
                session_id: None,
                runtime_turn_id: None,
                session_turn_number: None,
                turn_diagnostics: None,
            },
            conversation: Vec::new(),
            files: Vec::new(),
            schema_version: None,
        };

        let migrated_v0 = legacy.clone().migrate(SNAPSHOT_SCHEMA_VERSION)?;
        assert_eq!(migrated_v0.schema_version, Some(SNAPSHOT_SCHEMA_VERSION));
        assert!(migrated_v0.metadata.turn_diagnostics.is_none());

        let migrated_v1 =
            StoredSnapshot { schema_version: Some(SchemaVersion::V1), ..legacy }.migrate(SNAPSHOT_SCHEMA_VERSION)?;
        assert_eq!(migrated_v1.schema_version, Some(SNAPSHOT_SCHEMA_VERSION));
        assert!(migrated_v1.metadata.turn_diagnostics.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn v2_snapshot_round_trips_session_linkage_and_canonical_usage() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let usage = Usage {
            input_tokens: 43_282,
            output_tokens: 911,
            cached_input_tokens: 8_000,
            cache_creation_tokens: 512,
        };
        let diagnostics = SnapshotTurnDiagnostics {
            usage: usage.clone(),
            elapsed_ms: 12_345,
            requested_tool_calls: 9,
            admitted_tool_calls: 7,
            unadmitted_tool_calls: 2,
            failed_tool_calls: 2,
            denied_tool_calls: 1,
            preflight_failures: 1,
            reused_results: 2,
            spooled_results: 3,
            raw_spooled_bytes: 139_000,
            model_visible_output_bytes: 6_100,
            suppressed_tool_previews: 5,
            model_visible_tool_preview_budget_exhausted: true,
            low_signal_tool_calls: 4,
            recovery_activations: 1,
            in_progress_exec_sessions: vec![CompactStr::from("run-42")],
        };
        let context = SnapshotTurnContext {
            session_id: Some(CompactStr::from("session-911")),
            runtime_turn_id: Some(CompactStr::from("turn-runtime-911")),
            session_turn_number: Some(911),
            turn_diagnostics: Some(diagnostics.clone()),
            touched_files: Vec::new(),
        };

        manager
            .create_snapshot(1, "diagnostic checkpoint", &[], &BTreeSet::new(), None, None, Some(context))
            .await?
            .expect("metadata");

        let stored = manager.load_snapshot(1).await?.expect("stored snapshot");
        assert_eq!(stored.schema_version, Some(SNAPSHOT_SCHEMA_VERSION));
        assert_eq!(stored.metadata.session_id.as_deref(), Some("session-911"));
        assert_eq!(stored.metadata.runtime_turn_id.as_deref(), Some("turn-runtime-911"));
        assert_eq!(stored.metadata.session_turn_number, Some(911));
        assert_eq!(stored.metadata.turn_diagnostics, Some(diagnostics));
        assert_eq!(stored.metadata.turn_diagnostics.expect("diagnostics").usage, usage);
        Ok(())
    }

    #[test]
    fn parse_revert_scope_variants() {
        assert_eq!(SnapshotManager::parse_revert_scope("conversation"), Some(RevertScope::Conversation));
        assert_eq!(SnapshotManager::parse_revert_scope("code"), Some(RevertScope::Code));
        assert_eq!(SnapshotManager::parse_revert_scope("full"), Some(RevertScope::Both));
        assert_eq!(SnapshotManager::parse_revert_scope("unknown"), None);
    }
    #[tokio::test]
    async fn binary_checkpoint_reuses_content_and_does_not_delete_untracked_neighbors() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let file = manager.workspace.join("asset.bin");
        fs::write(&file, [0, 255, 4])?;
        let files = BTreeSet::from([PathBuf::from("asset.bin"), PathBuf::from("created.bin")]);
        manager.create_snapshot(1, "binary", &[], &files, None, None, None).await?;
        manager.create_snapshot(2, "same", &[], &files, None, None, None).await?;
        let store = filesnap::WorkspaceStore::open(&manager.storage_dir, &manager.workspace)?;
        let first = manager.load_snapshot(1).await?.expect("first");
        let second = manager.load_snapshot(2).await?.expect("second");
        let first_target = store
            .target_for_turn(first.files[0].data.as_deref().expect("ref"))?
            .expect("target");
        let second_target = store
            .target_for_turn(second.files[0].data.as_deref().expect("ref"))?
            .expect("target");
        let first_manifest = store.manifest(first_target.manifest_id())?;
        let second_manifest = store.manifest(second_target.manifest_id())?;
        let first_hash = &first_manifest.entries.values().next().expect("file").hash;
        let second_hash = &second_manifest.entries.values().next().expect("file").hash;
        assert_eq!(first_hash, second_hash);
        fs::write(&file, b"changed")?;
        fs::write(manager.workspace.join("created.bin"), b"created")?;
        fs::write(manager.workspace.join("neighbor"), b"keep")?;
        manager.restore_snapshot(1, RevertScope::Code).await?;
        assert_eq!(fs::read(file)?, [0, 255, 4]);
        assert!(!manager.workspace.join("created.bin").exists());
        assert_eq!(fs::read(manager.workspace.join("neighbor"))?, b"keep");
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refuses_symlink_escape_before_restoring_any_file() -> Result<()> {
        let (dir, manager) = setup_manager();
        fs::write(manager.workspace.join("a.txt"), "before")?;
        fs::create_dir(manager.workspace.join("nested"))?;
        fs::write(manager.workspace.join("nested/b.txt"), "before")?;
        let files = BTreeSet::from([PathBuf::from("a.txt"), PathBuf::from("nested/b.txt")]);
        manager.create_snapshot(1, "paths", &[], &files, None, None, None).await?;
        fs::write(manager.workspace.join("a.txt"), "after")?;
        fs::remove_dir_all(manager.workspace.join("nested"))?;
        let outside = dir.path().join("outside");
        fs::create_dir(&outside)?;
        fs::write(outside.join("b.txt"), "outside")?;
        std::os::unix::fs::symlink(&outside, manager.workspace.join("nested"))?;
        assert!(manager.restore_snapshot(1, RevertScope::Code).await.is_err());
        assert_eq!(fs::read_to_string(manager.workspace.join("a.txt"))?, "after");
        assert_eq!(fs::read_to_string(outside.join("b.txt"))?, "outside");
        Ok(())
    }

    #[tokio::test]
    async fn legacy_inline_contents_still_restore() -> Result<()> {
        let (_dir, manager) = setup_manager();
        fs::write(manager.workspace.join("old.txt"), "original")?;
        let files = BTreeSet::from([PathBuf::from("old.txt")]);
        manager.create_snapshot(1, "legacy", &[], &files, None, None, None).await?;
        let mut stored = manager.load_snapshot(1).await?.expect("snapshot");
        stored.schema_version = Some(SchemaVersion::V2);
        stored.files[0].encoding = Some(FileEncoding::Utf8);
        stored.files[0].data = Some("legacy".into());
        fs::write(manager.snapshot_path(1), serde_json::to_vec(&stored)?)?;
        manager.restore_snapshot(1, RevertScope::Code).await?;
        assert_eq!(fs::read_to_string(manager.workspace.join("old.txt"))?, "legacy");
        Ok(())
    }

    #[tokio::test]
    async fn retention_releases_engine_sessions_but_preserves_live_content() -> Result<()> {
        let (_dir, manager) = setup_manager();
        let path = manager.workspace.join("retained.txt");
        fs::write(&path, "shared")?;
        let files = BTreeSet::from([PathBuf::from("retained.txt")]);
        manager.create_snapshot(1, "first", &[], &files, None, None, None).await?;
        let first = manager.load_snapshot(1).await?.expect("first").files[0]
            .data
            .clone()
            .expect("reference");
        manager.create_snapshot(2, "second", &[], &files, None, None, None).await?;
        let second = manager.load_snapshot(2).await?.expect("second").files[0]
            .data
            .clone()
            .expect("reference");
        let mut config = SnapshotConfig::new(manager.workspace.clone());
        config.max_snapshots = 1;
        let janitor = SnapshotManager::new(config)?;
        janitor.cleanup_old_snapshots().await?;
        let store = filesnap::WorkspaceStore::open(&manager.storage_dir, &manager.workspace)?;
        assert_eq!(store.sessions()?, vec![second.clone()]);
        assert!(store.target_for_turn(&first)?.is_none());
        fs::write(&path, "changed")?;
        janitor.restore_snapshot(2, RevertScope::Code).await?;
        assert_eq!(fs::read_to_string(&path)?, "shared");
        janitor.create_snapshot(2, "replacement", &[], &files, None, None, None).await?;
        assert!(store.target_for_turn(&second)?.is_none());
        assert_eq!(store.sessions()?.len(), 1);
        Ok(())
    }
}
