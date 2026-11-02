//! Storage accounting and cleanup DTOs for the desktop Storage settings.

use serde::{Deserialize, Serialize};

/// On-disk usage broken down by owning store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSummary {
    /// State directory holding session archives and sibling stores.
    pub state_dir: String,
    /// Workspace whose `.vtcode` stores were measured.
    pub workspace: String,
    /// Bytes used by session archives.
    pub sessions_bytes: u64,
    /// Number of session archives counted.
    pub sessions_count: u64,
    /// Bytes used by managed loop-isolation worktrees.
    pub worktrees_bytes: u64,
    /// Number of managed worktrees counted.
    pub worktrees_count: u64,
    /// Bytes used by per-workspace `.vtcode/sessions` memory stores.
    pub memory_bytes: u64,
    /// Bytes used by the rest of the state directory.
    pub other_bytes: u64,
    /// Sum of every category above.
    pub total_bytes: u64,
}

/// One managed worktree available for review or removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeEntry {
    /// Directory name under `.vtcode/worktrees/`.
    pub name: String,
    /// HEAD commit hash when known.
    pub head: Option<String>,
    /// Whether the worktree has uncommitted changes.
    pub is_dirty: bool,
}

/// Parameters for removing one managed worktree by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRemoveParams {
    /// Directory name under `.vtcode/worktrees/`.
    pub name: String,
}

/// Outcome of deleting one session archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeleteResult {
    /// Deleted archive identifier, echoed from the request.
    pub session_id: String,
    /// Whether an archive file was removed.
    pub deleted: bool,
}

/// Outcome of removing one managed worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRemoveResult {
    /// Removed worktree name, echoed from the request.
    pub name: String,
    /// Whether a worktree was removed.
    pub removed: bool,
}
