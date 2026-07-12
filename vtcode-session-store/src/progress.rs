//! Durable, compaction-safe progress ledger.
//!
//! Long-horizon agent capability requires a persistent signal of *goal
//! progress* that survives compaction, fork, and resume. The live conversation
//! is never reloaded into context from disk, but the progress ledger is a tiny
//! derived artifact (like `manifest.json`) that the harness can read on each
//! turn to decide whether work is actually advancing toward completion.
//!
//! The ledger is stored under `<session_dir>/derived/progress.json` and
//! overwritten on each update — it is a single mutable summary, not an
//! append-only log, which keeps reads O(1) and cheap.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::SessionStoreError;
use crate::session_dir;

/// Lifecycle status of a single milestone toward the session goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneStatus {
    /// Not yet started.
    Pending,
    /// Actively being worked on this/last turn.
    InProgress,
    /// Completed and verified.
    Done,
    /// Blocked — cannot proceed without external input or a replan.
    Blocked,
}

impl MilestoneStatus {
    /// Whether this status counts as forward progress toward completion.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, MilestoneStatus::Done)
    }
}

/// A single tracked milestone derived from the task tracker / plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    /// Stable identifier (e.g. tracker item index or plan item id).
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Current status.
    pub status: MilestoneStatus,
}


/// Compact, durable progress signal for one session.
///
/// This is the harness's externalized memory of "are we getting closer to
/// done?" It is intentionally small so it can be loaded every turn without
/// touching the event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressLedger {
    /// Owning session id.
    pub session_id: String,
    /// The objective the agent is pursuing.
    pub goal: String,
    /// Tracked milestones; empty when the agent has no explicit tracker.
    pub milestones: Vec<Milestone>,
    /// Agent's confidence in eventual completion, `0.0..=1.0`.
    pub confidence: f32,
    /// RFC3339 timestamp of the first turn where no forward progress was
    /// detected, or `None` if progress is currently being made.
    pub stalled_since: Option<String>,
    /// RFC3339 timestamp of the last ledger update.
    pub updated_at: String,
}

impl ProgressLedger {
    /// Create a fresh ledger for a session with an initial goal.
    #[must_use]
    pub fn new(session_id: &str, goal: &str) -> Self {
        let ts = Utc::now().to_rfc3339();
        Self {
            session_id: session_id.to_string(),
            goal: goal.to_string(),
            milestones: Vec::new(),
            confidence: 1.0,
            stalled_since: None,
            updated_at: ts,
        }
    }

    /// Fraction of milestones in a terminal (`Done`) state, `0.0..=1.0`.
    /// Returns `1.0` when there are no milestones (nothing tracked yet).
    #[must_use]
    pub fn completion_ratio(&self) -> f32 {
        if self.milestones.is_empty() {
            return 1.0;
        }
        let done = self
            .milestones
            .iter()
            .filter(|m| m.status.is_terminal())
            .count() as f32;
        done / self.milestones.len() as f32
    }

    /// Whether every tracked milestone is complete (or none are tracked).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.completion_ratio() >= 1.0
    }

    /// Whether the ledger currently reports a stall.
    #[must_use]
    pub fn is_stalled(&self) -> bool {
        self.stalled_since.is_some()
    }

    /// Record forward progress: clears any stall marker and refreshes the
    /// timestamp. Confidence is nudged upward (bounded at 1.0).
    pub fn note_advance(&mut self) {
        self.stalled_since = None;
        self.confidence = (self.confidence + 0.05).min(1.0);
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Record a stall: sets `stalled_since` on first occurrence and refreshes
    /// the timestamp. Confidence is nudged downward (bounded at `0.0`).
    pub fn note_stall(&mut self) {
        if self.stalled_since.is_none() {
            self.stalled_since = Some(Utc::now().to_rfc3339());
        }
        self.confidence = (self.confidence - 0.1).max(0.0);
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Replace the milestone set and refresh the timestamp.
    pub fn set_milestones(&mut self, milestones: Vec<Milestone>) {
        self.milestones = milestones;
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Set the session goal and refresh the timestamp.
    pub fn set_goal(&mut self, goal: &str) {
        self.goal = goal.to_string();
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Render a compact, human-readable progress summary for durable memory
    /// (e.g. `<workspace>/memories/progress.md`). Survives compaction and gives
    /// a resumed session an accurate picture of what is done.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Session Progress\n\n");
        out.push_str(&format!("**Goal:** {}\n", self.goal));
        out.push_str(&format!(
            "**Completion:** {:.0}%\n",
            (self.completion_ratio() * 100.0).round()
        ));
        out.push_str(&format!("**Confidence:** {:.2}\n", self.confidence));
        if let Some(since) = &self.stalled_since {
            out.push_str(&format!("**Stalled since:** {since}\n"));
        }
        out.push_str(&format!("**Updated:** {}\n\n", self.updated_at));
        if self.milestones.is_empty() {
            out.push_str("_No tracked milestones yet._\n");
        } else {
            out.push_str("## Milestones\n\n");
            for m in &self.milestones {
                let mark = match m.status {
                    MilestoneStatus::Done => "[x]",
                    MilestoneStatus::InProgress => "[~]",
                    MilestoneStatus::Blocked => "[!]",
                    MilestoneStatus::Pending => "[ ]",
                };
                out.push_str(&format!("{} {} — {}\n", mark, m.id, m.description));
            }
        }
        out
    }
}

/// Resolve the on-disk path of the progress ledger for a session.
#[must_use]
pub fn progress_path(workspace: &Path, session_id: &str) -> std::path::PathBuf {
    session_dir(workspace, session_id)
        .join(crate::DERIVED_DIR)
        .join("progress.json")
}

/// Load the progress ledger for a session, if one has been persisted.
///
/// Returns `Ok(None)` when no ledger file exists yet (a fresh or pre-ledger
/// session) rather than an error, so callers can treat absence as "no signal".
pub fn load_progress(
    workspace: &Path,
    session_id: &str,
) -> Result<Option<ProgressLedger>, SessionStoreError> {
    let path = progress_path(workspace, session_id);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
    let ledger: ProgressLedger = serde_json::from_slice(&bytes)?;
    Ok(Some(ledger))
}

/// Persist the progress ledger for a session, creating `derived/` if needed.
pub fn save_progress(
    workspace: &Path,
    session_id: &str,
    ledger: &ProgressLedger,
) -> Result<(), SessionStoreError> {
    let path = progress_path(workspace, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| SessionStoreError::CreateDir {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let bytes = serde_json::to_string_pretty(ledger)?;
    std::fs::write(&path, bytes).map_err(|e| SessionStoreError::io(path, e))?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ledger() -> ProgressLedger {
        let mut l = ProgressLedger::new("s1", "ship the feature");
        l.set_milestones(vec![
            Milestone {
                id: "1".into(),
                description: "design".into(),
                status: MilestoneStatus::Done,
            },
            Milestone {
                id: "2".into(),
                description: "implement".into(),
                status: MilestoneStatus::InProgress,
            },
            Milestone {
                id: "3".into(),
                description: "verify".into(),
                status: MilestoneStatus::Pending,
            },
        ]);
        l
    }

    #[test]
    fn completion_ratio_reflects_terminal_milestones() {
        let l = sample_ledger();
        assert!((l.completion_ratio() - 1.0 / 3.0).abs() < f32::EPSILON);
        assert!(!l.is_complete());
    }

    #[test]
    fn empty_ledger_is_complete() {
        let l = ProgressLedger::new("s", "goal");
        assert!(l.is_complete());
        assert_eq!(l.completion_ratio(), 1.0);
    }

    #[test]
    fn advance_clears_stall_and_bumps_confidence() {
        let mut l = sample_ledger();
        l.note_stall();
        assert!(l.is_stalled());
        let before = l.confidence;
        l.note_advance();
        assert!(!l.is_stalled());
        assert!(l.confidence >= before);
    }

    #[test]
    fn persistence_round_trips() {
        let tmp = std::env::temp_dir().join(format!("vtcode-prog-{}", std::process::id()));
        let ws = tmp.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let mut l = sample_ledger();
        l.note_stall();
        save_progress(&ws, "s1", &l).unwrap();
        let loaded = load_progress(&ws, "s1").unwrap().expect("ledger present");
        assert_eq!(loaded, l);
        assert!(loaded.is_stalled());
        // Absent ledger reads as None, not an error.
        assert!(load_progress(&ws, "absent").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
