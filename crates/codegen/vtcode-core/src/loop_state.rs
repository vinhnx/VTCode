//! Loop run state persistence for loop-engineering workflows.
//!
//! A loop is a long-lived scheduler that invokes the vtcode harness repeatedly.
//! `LoopRunState` captures the durable state a loop scheduler reads on resume:
//! current step index, last artifact path, and status.
//!
//! State is persisted as JSON under `{workspace}/.vtcode/state/loop-{id}.json`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(1);

const STATE_DIR_NAME: &str = "state";

// ─── Loop Run State ──────────────────────────────────────────────────────────

/// Durable state for a single loop run. The loop scheduler reads this on
/// resume to know where execution left off.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunState {
    /// Unique identifier for this loop run.
    pub loop_id: String,
    /// Zero-based index of the current step.
    pub step_index: u32,
    /// Path to the last artifact produced by the loop (e.g., a diff, a report).
    pub last_artifact_path: Option<PathBuf>,
    /// Current lifecycle status.
    pub status: LoopStatus,
    /// When the loop run started.
    pub started_at: DateTime<Utc>,
    /// When the loop state was last persisted.
    pub updated_at: DateTime<Utc>,
}

/// Lifecycle status of a loop run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    /// The loop is actively running.
    Running,
    /// The loop completed all steps successfully.
    Completed,
    /// The loop failed and cannot resume.
    Failed,
    /// The loop was paused and can be resumed.
    Paused,
}

impl LoopRunState {
    /// Create a new loop run state with the given identifier.
    pub fn new(loop_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            loop_id: loop_id.into(),
            step_index: 0,
            last_artifact_path: None,
            status: LoopStatus::Running,
            started_at: now,
            updated_at: now,
        }
    }

    /// Advance to the next step and update the timestamp.
    pub fn advance_step(&mut self) {
        self.step_index = self.step_index.saturating_add(1);
        self.updated_at = Utc::now();
    }

    /// Mark the loop as completed.
    pub fn mark_completed(&mut self) {
        self.status = LoopStatus::Completed;
        self.updated_at = Utc::now();
    }

    /// Mark the loop as failed.
    pub fn mark_failed(&mut self) {
        self.status = LoopStatus::Failed;
        self.updated_at = Utc::now();
    }

    /// Mark the loop as paused.
    pub fn mark_paused(&mut self) {
        self.status = LoopStatus::Paused;
        self.updated_at = Utc::now();
    }

    /// Returns true if the loop can be resumed.
    pub fn is_resumable(&self) -> bool {
        matches!(self.status, LoopStatus::Paused | LoopStatus::Running)
    }
}

// ─── Persistence ─────────────────────────────────────────────────────────────

/// Resolve the `.vtcode/state/` directory for a workspace.
pub fn state_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".vtcode").join(STATE_DIR_NAME)
}

/// Resolve the path for a specific loop state file.
pub fn loop_state_path(workspace_root: &Path, loop_id: &str) -> PathBuf {
    state_dir(workspace_root).join(format!("loop-{loop_id}.json"))
}

/// Save loop run state to disk using atomic write (temp file + rename).
pub fn save_loop_state(workspace_root: &Path, state: &LoopRunState) -> Result<PathBuf> {
    let dir = state_dir(workspace_root);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create state directory {}", dir.display()))?;

    let path = loop_state_path(workspace_root, &state.loop_id);
    let serialized =
        serde_json::to_vec_pretty(state).context("Failed to serialize loop run state")?;

    atomic_write(&path, &serialized)?;
    Ok(path)
}

/// Load loop run state from disk.
pub fn load_loop_state(workspace_root: &Path, loop_id: &str) -> Result<Option<LoopRunState>> {
    let path = loop_state_path(workspace_root, loop_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read loop state {}", path.display()))?;
    let state: LoopRunState = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(state))
}

/// Delete a loop state file from disk.
pub fn delete_loop_state(workspace_root: &Path, loop_id: &str) -> Result<bool> {
    let path = loop_state_path(workspace_root, loop_id);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to delete loop state {}", path.display()))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// List all loop state files in the state directory.
pub fn list_loop_states(workspace_root: &Path) -> Result<Vec<LoopRunState>> {
    let dir = state_dir(workspace_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut states = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("Failed to read state directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("loop-") && n.ends_with(".json"))
        {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            match serde_json::from_str::<LoopRunState>(&raw) {
                Ok(state) => states.push(state),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "Skipping malformed loop state file");
                    continue;
                }
            }
        }
    }
    states.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(states)
}

/// Atomic write using temp file + rename, matching the pattern from
/// `scheduler/mod.rs:1381`.
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let temp_name = format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("loop-state"),
        WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp_path = path.with_file_name(temp_name);
    fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn loop_run_state_new_has_correct_defaults() {
        let state = LoopRunState::new("test-loop");
        assert_eq!(state.loop_id, "test-loop");
        assert_eq!(state.step_index, 0);
        assert_eq!(state.status, LoopStatus::Running);
        assert!(state.last_artifact_path.is_none());
    }

    #[test]
    fn loop_run_state_advance_step_increments() {
        let mut state = LoopRunState::new("test");
        assert_eq!(state.step_index, 0);
        state.advance_step();
        assert_eq!(state.step_index, 1);
        state.advance_step();
        assert_eq!(state.step_index, 2);
    }

    #[test]
    fn loop_run_state_status_transitions() {
        let mut state = LoopRunState::new("test");
        assert_eq!(state.status, LoopStatus::Running);
        assert!(state.is_resumable());

        state.mark_paused();
        assert_eq!(state.status, LoopStatus::Paused);
        assert!(state.is_resumable());

        state.mark_completed();
        assert_eq!(state.status, LoopStatus::Completed);
        assert!(!state.is_resumable());

        let mut state2 = LoopRunState::new("test2");
        state2.mark_failed();
        assert_eq!(state2.status, LoopStatus::Failed);
        assert!(!state2.is_resumable());
    }

    #[test]
    fn loop_state_round_trip_persistence() {
        let tmp = TempDir::new().expect("temp dir");
        let mut state = LoopRunState::new("round-trip-test");
        state.advance_step();
        state.last_artifact_path = Some(PathBuf::from("/tmp/artifact.txt"));

        let path = save_loop_state(tmp.path(), &state).expect("save");
        assert!(path.exists());

        let loaded = load_loop_state(tmp.path(), "round-trip-test")
            .expect("load")
            .expect("should exist");
        assert_eq!(loaded.loop_id, "round-trip-test");
        assert_eq!(loaded.step_index, 1);
        assert!(loaded.last_artifact_path.is_some());
        assert_eq!(loaded.status, LoopStatus::Running);
    }

    #[test]
    fn load_loop_state_returns_none_for_missing() {
        let tmp = TempDir::new().expect("temp dir");
        let result = load_loop_state(tmp.path(), "nonexistent").expect("ok");
        assert!(result.is_none());
    }

    #[test]
    fn delete_loop_state_removes_file() {
        let tmp = TempDir::new().expect("temp dir");
        let state = LoopRunState::new("delete-me");
        save_loop_state(tmp.path(), &state).expect("save");

        let deleted = delete_loop_state(tmp.path(), "delete-me").expect("delete");
        assert!(deleted);

        let loaded = load_loop_state(tmp.path(), "delete-me").expect("load");
        assert!(loaded.is_none());
    }

    #[test]
    fn list_loop_states_returns_sorted_by_updated_at() {
        let tmp = TempDir::new().expect("temp dir");

        let mut state1 = LoopRunState::new("loop-1");
        state1.updated_at = Utc::now() - chrono::Duration::hours(1);
        save_loop_state(tmp.path(), &state1).expect("save");

        let mut state2 = LoopRunState::new("loop-2");
        state2.updated_at = Utc::now();
        save_loop_state(tmp.path(), &state2).expect("save");

        let states = list_loop_states(tmp.path()).expect("list");
        assert_eq!(states.len(), 2);
        // Most recent first
        assert_eq!(states[0].loop_id, "loop-2");
        assert_eq!(states[1].loop_id, "loop-1");
    }

    #[test]
    fn loop_state_serializes_status_variants() {
        for status in [
            LoopStatus::Running,
            LoopStatus::Completed,
            LoopStatus::Failed,
            LoopStatus::Paused,
        ] {
            let state = LoopRunState {
                loop_id: "serde-test".to_string(),
                step_index: 0,
                last_artifact_path: None,
                status: status.clone(),
                started_at: Utc::now(),
                updated_at: Utc::now(),
            };
            let json = serde_json::to_string(&state).expect("serialize");
            let deserialized: LoopRunState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(deserialized.status, status);
        }
    }
}
