//! Canonical durable contract for task-tracker checklist snapshots.
//!
//! Every successful `task_tracker` mutation publishes a [`TrackerSnapshot`]
//! through [`ThreadEvent::TrackerUpdated`](crate::ThreadEvent::TrackerUpdated)
//! so timeline consumers (desktop Tasks viewer, session archives) replay typed
//! checklist state instead of re-parsing rendered tool output. The snapshot
//! carries the user-facing surface only — title, progress counts, and the
//! compact item tree — mirroring the CLI transcript/panel split; structured
//! step metadata (`files`/`outcome`/`verify`) stays in the tool payload and
//! the persisted tracker artifacts.
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Lifecycle status of one checklist item, using the canonical `task_tracker`
/// wire strings (`pending`, `in_progress`, `completed`, `blocked`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TrackerItemStatus {
    /// Item is queued and not started.
    #[default]
    Pending,
    /// Item is actively being worked on.
    InProgress,
    /// Item finished successfully.
    Completed,
    /// Item cannot proceed without external input.
    Blocked,
    /// Catch-all for statuses added in newer schema versions.
    #[serde(other)]
    Unknown,
}

impl TrackerItemStatus {
    /// Canonical wire string for this status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

impl FromStr for TrackerItemStatus {
    type Err = ();

    /// Parse a canonical status string; unknown shapes fail closed to `Err`
    /// so producers skip unparseable items instead of mislabeling them.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "blocked" => Ok(Self::Blocked),
            _ => Err(()),
        }
    }
}

/// One checklist row in a tracker snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TrackerItemState {
    /// Dotted (`1.2`) or flat (`1`) position of the item in the checklist.
    pub index_path: String,
    /// Short leading-clause description of the item.
    pub description: String,
    /// Lifecycle status of the item.
    pub status: TrackerItemStatus,
}

/// Bounded checklist snapshot published after a `task_tracker` mutation.
///
/// `completed`/`total` always describe the full checklist even when `items`
/// is empty (explicit counts win over derived ones, mirroring the CLI
/// progress line). Derived views replay the newest snapshot per session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TrackerSnapshot {
    /// Raw checklist title; views humanize generated timestamp slugs.
    pub title: String,
    /// Finished items in the full checklist.
    pub completed: usize,
    /// Renderable items in the full checklist.
    pub total: usize,
    /// Item tree in checklist order. Tool-assigned index paths are unique
    /// within a snapshot, so views may key rows on them.
    pub items: Vec<TrackerItemState>,
}
