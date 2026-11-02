use super::*;

pub(super) const MAX_DESCRIPTION_LEN: usize = 160;

pub const DEFAULT_CHECKPOINTS_ENABLED: bool = true;
pub const DEFAULT_MAX_SNAPSHOTS: usize = 50;
pub const DEFAULT_MAX_AGE_DAYS: u64 = 30;
/// How many newest active turns a finished session keeps pinned for rewind.
/// Older entries are dropped on thread completion so their snapshots become
/// prune-eligible instead of sitting protected for the full age window.
pub const REWIND_ACTIVE_KEEP: usize = 5;
pub(super) const SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(3);

/// Collect turn numbers from a navigation value: either a bare array of turns
/// (`active`) or a recovery object with an `active` array (`redo`/`pending`).
pub(super) fn collect_turn_numbers(value: &serde_json::Value, out: &mut BTreeSet<usize>) {
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

pub(super) fn normalized_prompt_text(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(super) fn sanitize_relative_path(path: &Path) -> Option<PathBuf> {
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

/// One workspace path a checkpoint could not snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotSkippedPath {
    /// Workspace-relative path with forward slashes (matches [`FileSnapshot::path`]).
    pub path: String,
    /// Human reason phrase; free text so old readers never choke on new reasons.
    pub reason: String,
}

/// Paths a checkpoint left out. The turn proceeds anyway; rewind surfaces
/// this so a restore never silently claims completeness it does not have.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotSkippedPaths {
    /// Total dropped paths; `sample` is bounded by filesnap's per-turn report.
    #[serde(default)]
    pub total: usize,
    /// Up to a few dropped paths with reasons (bounded sample, not exhaustive).
    #[serde(default)]
    pub sample: Vec<SnapshotSkippedPath>,
}

impl SnapshotSkippedPaths {
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// Short data summary every warning site shares:
    /// `2 paths (a.mov (larger than …), and 1 more)`.
    pub fn summary(&self) -> String {
        let noun = if self.total == 1 { "path" } else { "paths" };
        if self.sample.is_empty() {
            return format!("{} {noun}", self.total);
        }
        let sample = self
            .sample
            .iter()
            .map(|skipped| format!("{} ({})", skipped.path, skipped.reason))
            .collect::<Vec<_>>()
            .join(", ");
        if self.sample.len() < self.total {
            format!("{} {noun} ({sample}, and {} more)", self.total, self.total - self.sample.len())
        } else {
            format!("{} {noun} ({sample})", self.total)
        }
    }
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
    /// Paths the capture left out; the turn proceeded anyway. `None` (and
    /// absent in old records) means a complete capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_paths: Option<SnapshotSkippedPaths>,
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
pub(super) static NEXT_ACTION_NUMBER: AtomicU64 = AtomicU64::new(1);

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

    pub(super) fn storage_dir(&self) -> PathBuf {
        self.storage_dir
            .clone()
            .unwrap_or_else(|| self.workspace.join(".vtcode").join("checkpoints"))
    }
}
