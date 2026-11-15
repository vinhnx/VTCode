//! Runtime-facing [`GoalTracker`](super::GoalTracker) API and persistence.
//!
//! The state machine itself lives in [`super`]; this module holds the small
//! public surface the runloop and frontends use (read accessors, token/elapsed
//! accounting, objective edits) plus `goal.json` save/restore under the
//! session's `goal/` directory. Restoring demotes any in-flight goal to
//! paused (see [`GoalTracker::from_snapshot`](super::GoalTracker::from_snapshot)),
//! so a reloaded goal never resumes driving itself.

use std::path::PathBuf;

use super::{GoalOrchestration, GoalStatus, GoalTracker};
use crate::error::SessionStoreError;

/// Snapshot filename inside the session's `goal/` directory.
const GOAL_SNAPSHOT_FILE: &str = "goal.json";

impl GoalStatus {
    /// Wire value used in persisted snapshots (`"active"`, `"user_paused"`, …).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::UserPaused => "user_paused",
            Self::BackOffPaused => "back_off_paused",
            Self::NoProgressPaused => "no_progress_paused",
            Self::InfraPaused => "infra_paused",
            Self::Blocked => "blocked",
            Self::BudgetLimited => "budget_limited",
            Self::Complete => "complete",
        }
    }

    /// Short human-readable label for status lines.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::UserPaused => "Paused",
            Self::BackOffPaused => "Paused (backing off)",
            Self::NoProgressPaused => "Paused (no progress)",
            Self::InfraPaused => "Paused (infra error)",
            Self::Blocked => "Blocked",
            Self::BudgetLimited => "Budget limited",
            Self::Complete => "Complete",
        }
    }
}

impl GoalTracker {
    /// `true` when a goal snapshot exists, regardless of lifecycle status.
    #[must_use]
    pub fn has_goal(&self) -> bool {
        self.orchestration.is_some()
    }

    /// Stable goal id, if a goal exists.
    #[must_use]
    pub fn goal_id(&self) -> Option<&str> {
        self.orchestration.as_ref().map(|o| o.goal_id.as_str())
    }

    /// User-facing pause message, if the goal paused with one.
    #[must_use]
    pub fn pause_message(&self) -> Option<&str> {
        self.orchestration.as_ref().and_then(|o| o.pause_message.as_deref())
    }

    /// Total elapsed wall-clock time in milliseconds, including the live
    /// portion when the goal is currently active.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        let base = self.orchestration.as_ref().map_or(0, |o| o.elapsed_ms);
        if self.orchestration.is_some()
            && let Some(since) = self.active_since
        {
            return base.saturating_add(since.elapsed().as_millis() as u64);
        }
        base
    }

    /// High-water mark of tokens spent on the goal (session total minus the
    /// start baseline, floored at zero). Zero when no goal exists.
    #[must_use]
    pub fn tokens_used(&self) -> i64 {
        self.orchestration.as_ref().map_or(0, |o| o.tokens_used_high_water)
    }

    /// Consumed auto-continue turns this episode. Zero without a goal.
    #[must_use]
    pub fn auto_continue_turns(&self) -> u8 {
        self.orchestration.as_ref().map_or(0, |o| o.auto_continue_turns)
    }

    /// Atomically reserve one auto-continue turn when budget remains,
    /// returning the 1-based turn number. `None` leaves state untouched.
    pub fn claim_auto_continue_turn(&mut self, max_turns: u8) -> Option<u8> {
        let o = self.orchestration.as_mut()?;
        if o.auto_continue_turns >= max_turns {
            return None;
        }
        o.auto_continue_turns = o.auto_continue_turns.saturating_add(1);
        Some(o.auto_continue_turns)
    }

    /// Fold a session token-total observation into the high-water mark.
    /// Monotonic, and a no-op without a goal.
    pub fn record_token_observation(&mut self, session_total: i64) {
        if let Some(o) = self.orchestration.as_mut() {
            o.last_session_tokens_seen = Some(session_total);
            let used = session_total.saturating_sub(o.token_baseline).max(0);
            o.tokens_used_high_water = o.tokens_used_high_water.max(used);
        }
    }

    /// Replace the objective in place, preserving elapsed time and history.
    /// Returns `false` when no goal exists.
    pub fn set_objective(&mut self, objective: &str) -> bool {
        if let Some(o) = self.orchestration.as_mut() {
            o.objective = objective.trim().to_string();
            return true;
        }
        false
    }

    /// Persist the snapshot to `<session_dir>/goal/goal.json`, creating the
    /// directory if needed. With no goal, removes a stale snapshot instead so
    /// a cleared goal never resurrects on restart.
    pub fn persist(&self) -> Result<(), SessionStoreError> {
        let dir = self.goal_dir();
        std::fs::create_dir_all(&dir).map_err(|e| SessionStoreError::CreateDir { path: dir.clone(), source: e })?;
        let path = dir.join(GOAL_SNAPSHOT_FILE);
        match &self.orchestration {
            Some(o) => {
                let bytes = serde_json::to_vec(o)?;
                std::fs::write(&path, bytes).map_err(|e| SessionStoreError::io(path, e))?;
                Ok(())
            }
            None => {
                if path.exists() {
                    std::fs::remove_file(&path).map_err(|e| SessionStoreError::io(path, e))?;
                }
                Ok(())
            }
        }
    }

    /// Restore a tracker from `<session_dir>/goal/goal.json`, or return an
    /// empty tracker when no snapshot exists. In-flight goals restore paused.
    pub fn restore(session_dir: PathBuf) -> Result<Self, SessionStoreError> {
        let path = session_dir.join("goal").join(GOAL_SNAPSHOT_FILE);
        if !path.exists() {
            return Ok(Self::new(session_dir));
        }
        let bytes = std::fs::read(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
        let snapshot: GoalOrchestration = serde_json::from_slice(&bytes)?;
        Ok(Self::from_snapshot(session_dir, snapshot))
    }

    /// Current snapshot for event and RPC payloads; `None` without a goal.
    #[must_use]
    pub fn goal_snapshot(&self) -> Option<vtcode_exec_events::goal::GoalSnapshot> {
        use vtcode_exec_events::goal::GoalSnapshot;
        self.orchestration.as_ref().map(|o| GoalSnapshot {
            goal_id: o.goal_id.clone(),
            state: vtcode_exec_events::goal::GoalState::from(o.status),
            objective: o.objective.clone(),
            elapsed_ms: self.elapsed_ms(),
            tokens_used: o.tokens_used_high_water,
            token_budget: o.token_budget,
            pause_message: o.pause_message.clone(),
        })
    }

    /// Build the `goal.updated` payload for the current state; `goal: None`
    /// when no goal exists.
    #[must_use]
    pub fn to_goal_updated_event(&self) -> vtcode_exec_events::ThreadEvent {
        use vtcode_exec_events::goal::GoalUpdatedEvent;
        let goal = self.goal_snapshot();
        vtcode_exec_events::ThreadEvent::GoalUpdated(Box::new(GoalUpdatedEvent { goal }))
    }
}

impl From<GoalStatus> for vtcode_exec_events::goal::GoalState {
    /// Collapse the tracker's fine-grained pause reasons into the coarse wire
    /// state. Every paused variant maps to `Paused`; terminal states map 1:1.
    fn from(status: GoalStatus) -> Self {
        use vtcode_exec_events::goal::GoalState;
        match status {
            GoalStatus::Active => GoalState::Active,
            GoalStatus::UserPaused
            | GoalStatus::BackOffPaused
            | GoalStatus::NoProgressPaused
            | GoalStatus::InfraPaused => GoalState::Paused,
            GoalStatus::Blocked => GoalState::Blocked,
            GoalStatus::BudgetLimited => GoalState::BudgetLimited,
            GoalStatus::Complete => GoalState::Complete,
        }
    }
}
