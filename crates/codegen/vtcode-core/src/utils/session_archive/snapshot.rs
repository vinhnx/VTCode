use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::identifier::validate_session_identifier;
use super::message::SessionMessage;
use super::metadata::SessionArchiveMetadata;
use super::shrink::truncate_preview;
use crate::core::agent::snapshots::SnapshotTurnDiagnostics;
use crate::llm::provider::MessageRole;
use crate::utils::error_log_collector::ErrorLogEntry;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub metadata: SessionArchiveMetadata,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub total_messages: usize,
    pub distinct_tools: Vec<String>,
    pub transcript: Vec<String>,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
    // SessionProgress is heavy and frequently absent in final snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Box<SessionProgress>>,
    /// ERROR-level log entries captured during the session for post-mortem debugging.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub error_logs: Vec<ErrorLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionProgress {
    pub turn_number: usize,
    #[serde(default)]
    pub recent_messages: Vec<SessionMessage>,
    #[serde(default)]
    pub tool_summaries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,
    /// Names of skills loaded at checkpoint time
    #[serde(default)]
    pub loaded_skills: Vec<String>,
    /// Diagnostics for the most recently checkpointed turn, including turns
    /// that ended blocked, cancelled, or aborted before a turn snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_diagnostics: Option<SnapshotTurnDiagnostics>,
}

#[derive(Debug, Clone)]
pub struct SessionListing {
    pub path: PathBuf,
    pub snapshot: SessionSnapshot,
}

/// Minimal projection of [`SessionSnapshot`] used to filter, sort, and limit
/// archive listings without deserializing transcripts, messages, or progress.
///
/// Unknown fields are ignored, so this parses every file a full snapshot
/// parse accepts. Files it rejects are skipped exactly as a failed full parse.
#[derive(Debug, Deserialize)]
pub(crate) struct SessionListHeader {
    pub metadata: SessionListHeaderMetadata,
    pub ended_at: DateTime<Utc>,
}

/// Header-only view of [`SessionArchiveMetadata`](super::metadata::SessionArchiveMetadata).
#[derive(Debug, Deserialize)]
pub(crate) struct SessionListHeaderMetadata {
    #[serde(default)]
    pub workspace_path: String,
}

/// Small archive metadata retained by desktop session-list queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionArchiveSummary {
    /// Stable archive identifier.
    pub identifier: String,
    /// First user prompt preview, when present.
    pub title: Option<String>,
    /// Workspace path recorded in the archive.
    pub workspace_path: String,
    /// Whether the saved archive is terminal rather than in progress.
    pub completed: bool,
    /// Whether saved messages are available to resume.
    pub resumable: bool,
    /// Archive file size in bytes, zero when the file cannot be inspected.
    pub size_bytes: u64,
    /// Snapshot end time as unix seconds, zero when out of range.
    pub updated_at: u64,
}

/// An archive identifier derived from a loaded session listing.
///
/// The inner value is crate-private so callers cannot mark an arbitrary
/// runtime/session ID as resumable. Obtain one from
/// [`super::SessionArchive::verify_persisted_resume_identifier`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSessionArchiveIdentifier(pub(crate) String);

impl VerifiedSessionArchiveIdentifier {
    /// Borrow the verified archive identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct SessionProgressArgs {
    pub total_messages: usize,
    pub distinct_tools: Vec<String>,
    pub messages: Vec<SessionMessage>,
    pub recent_messages: Vec<SessionMessage>,
    pub turn_number: usize,
    pub token_usage: Option<String>,
    pub max_context_tokens: Option<usize>,
    pub loaded_skills: Option<Vec<String>>,
    /// Optional diagnostics for the turn represented by this checkpoint.
    pub turn_diagnostics: Option<SnapshotTurnDiagnostics>,
}

/// Outcome of an asynchronous session-progress checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionProgressPersistenceStatus {
    /// The archive file was written successfully.
    Persisted(PathBuf),
    /// The call was intentionally skipped by the progress throttle.
    Throttled(PathBuf),
    /// History persistence is disabled, so no archive file was written.
    Disabled(PathBuf),
}

impl SessionProgressPersistenceStatus {
    /// Return the archive path associated with this checkpoint attempt.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Persisted(path) | Self::Throttled(path) | Self::Disabled(path) => path,
        }
    }

    /// Return whether this outcome guarantees that the current snapshot was
    /// written to the archive.
    #[must_use]
    pub fn is_persisted(&self) -> bool {
        matches!(self, Self::Persisted(_))
    }
}

impl SessionListing {
    pub fn identifier(&self) -> String {
        self.path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Return an identifier that is safe to advertise as an archive resume target.
    pub(crate) fn verified_identifier(&self) -> Result<VerifiedSessionArchiveIdentifier> {
        let identifier = self
            .path
            .file_stem()
            .and_then(|value| value.to_str())
            .with_context(|| format!("session archive path has no valid identifier: {}", self.path.display()))?;
        validate_session_identifier(identifier)?;
        Ok(VerifiedSessionArchiveIdentifier(identifier.to_owned()))
    }

    pub fn first_prompt_preview(&self) -> Option<String> {
        self.preview_for_role(MessageRole::User)
    }

    pub fn first_reply_preview(&self) -> Option<String> {
        self.preview_for_role(MessageRole::Assistant)
    }

    /// Archive file size in bytes, zero when the file cannot be inspected.
    pub fn size_bytes(&self) -> u64 {
        std::fs::metadata(&self.path).map(|metadata| metadata.len()).unwrap_or(0)
    }

    /// Snapshot end time as unix seconds, zero when out of range.
    pub fn updated_at(&self) -> u64 {
        u64::try_from(self.snapshot.ended_at.timestamp()).unwrap_or(0)
    }

    /// Project the fields needed for recent-session navigation and release the full transcript.
    pub(crate) fn into_summary(self) -> SessionArchiveSummary {
        let size_bytes = self.size_bytes();
        let updated_at = self.updated_at();
        let identifier = self.identifier();
        let title = self.first_prompt_preview();
        let completed = self.snapshot.progress.is_none();
        let resumable = !self.snapshot.messages.is_empty();
        let workspace_path = self.snapshot.metadata.workspace_path;
        SessionArchiveSummary {
            identifier,
            title,
            workspace_path,
            completed,
            resumable,
            size_bytes,
            updated_at,
        }
    }

    fn preview_for_role(&self, role: MessageRole) -> Option<String> {
        self.snapshot.messages.iter().find_map(|message| {
            if message.role != role {
                return None;
            }

            let text_projection = message.content.as_text();
            if text_projection.trim().is_empty() {
                return None;
            }

            text_projection.lines().find_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(truncate_preview(trimmed, 80))
                }
            })
        })
    }
}

fn normalize_workspace_for_match(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };

    crate::utils::path::normalize_path(&absolute)
}

pub fn session_workspace_path(listing: &SessionListing) -> Option<PathBuf> {
    let raw = listing.snapshot.metadata.workspace_path.trim();
    if raw.is_empty() { None } else { Some(PathBuf::from(raw)) }
}

pub fn session_listing_matches_workspace(listing: &SessionListing, workspace: &Path) -> bool {
    raw_workspace_path_matches_query(
        &listing.snapshot.metadata.workspace_path,
        &normalize_workspace_for_match(workspace),
    )
}

/// Normalize a list-query workspace once so per-file matching avoids a
/// repeated `current_dir` + normalization per archive.
pub(crate) fn normalize_list_query_workspace(path: &Path) -> PathBuf {
    normalize_workspace_for_match(path)
}

/// Match a raw `metadata.workspace_path` against a pre-normalized query.
///
/// This is the single matching core behind [`session_listing_matches_workspace`];
/// header scans call it directly with an already-normalized query.
pub(crate) fn raw_workspace_path_matches_query(workspace_path: &str, normalized_query: &Path) -> bool {
    let raw = workspace_path.trim();
    if raw.is_empty() {
        return false;
    }

    normalize_workspace_for_match(Path::new(raw)).as_path() == normalized_query
}
