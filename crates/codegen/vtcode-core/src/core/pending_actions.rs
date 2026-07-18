//! Pending actions tracking: tool calls that have been issued but not yet returned.
//!
//! The agent harness issues tool calls to the LLM and then executes them. Between
//! issue and completion, the actions are "pending." Tracking pending actions enables:
//!
//! - Crash recovery: distinguish "tool never ran" from "tool response didn't arrive."
//! - Timeout detection: identify hung tools and cancel them.
//! - History validation: a pending action is not a "missing output" — it's expected.
//!
//! Following the "state as a first-class citizen" principle (Hitchhiker's Guide
//! to Agentic AI, Section 18.6.3), pending actions are part of the agent's
//! explicit state rather than an implicit property of the message history.

use serde::{Deserialize, Serialize};

/// A tool call that has been dispatched to the execution engine but not yet
/// completed. Tracked by `agent_call_id` matching the LLM's tool_call_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAction {
    /// Matches the `tool_call_id` from the LLM's function call.
    pub action_id: String,
    /// Name of the tool being invoked.
    pub tool_name: String,
    /// JSON arguments passed to the tool.
    pub arguments: serde_json::Value,
    /// Unix timestamp (seconds) when the action was dispatched.
    pub issued_at: u64,
    /// Expected outcome category, enabling targeted rollback.
    pub expected_outcome: ExpectedOutcome,
    /// Current lifecycle status.
    pub status: PendingActionStatus,
}

impl PendingAction {
    /// Create a new pending action with `InFlight` status.
    pub fn new(
        action_id: String,
        tool_name: String,
        arguments: serde_json::Value,
        expected_outcome: ExpectedOutcome,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            action_id,
            tool_name,
            arguments,
            issued_at: now,
            expected_outcome,
            status: PendingActionStatus::InFlight,
        }
    }

    /// Returns `true` if this action is still in flight.
    pub fn is_in_flight(&self) -> bool {
        matches!(self.status, PendingActionStatus::InFlight)
    }

    /// Returns `true` if this action has exceeded the given timeout (in seconds).
    pub fn is_stale(&self, timeout_secs: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        matches!(self.status, PendingActionStatus::InFlight) && now.saturating_sub(self.issued_at) > timeout_secs
    }
}

/// Lifecycle status of a pending action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingActionStatus {
    /// The tool call has been dispatched and is awaiting completion.
    InFlight,
    /// The tool call completed successfully or with an expected error.
    Completed {
        /// Unix timestamp (seconds) when the completion was recorded.
        completed_at: u64,
        /// Whether the tool execution was successful.
        success: bool,
    },
    /// The tool call failed with an unexpected error.
    Failed {
        /// Unix timestamp (seconds) when the failure was recorded.
        completed_at: u64,
        /// Error message describing the failure.
        error: String,
    },
    /// The tool call timed out without completing.
    TimedOut,
}

/// Expected outcome category for a pending action.
///
/// Used by the incremental rollback system to determine which actions to undo
/// and how (e.g., file modifications need file-level rollback, read operations
/// only need conversation-level rollback).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedOutcome {
    /// Expected to modify one or more files on disk.
    FileModification {
        /// Paths of files expected to be modified.
        paths: Vec<String>,
    },
    /// Expected to read data without side effects.
    ReadOperation,
    /// Expected to execute a shell command with potential side effects.
    CommandExecution,
    /// Expected to search the codebase or filesystem.
    SearchOperation,
    /// Catch-all for outcomes not known upfront.
    Generic,
}

impl ExpectedOutcome {
    /// Returns `true` if actions with this outcome have filesystem side effects.
    pub fn has_side_effects(&self) -> bool {
        matches!(self, ExpectedOutcome::FileModification { .. } | ExpectedOutcome::CommandExecution)
    }
}

/// A list of pending actions with helpers for registration, resolution, and
/// stale detection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingActions {
    /// Active and recently-completed actions. Newest first.
    actions: Vec<PendingAction>,
    /// Maximum number of completed/failed/timeout actions to retain for
    /// crash-recovery cross-referencing.
    max_retained: usize,
}

impl PendingActions {
    /// Create a new pending actions store with a retention limit.
    pub fn new(max_retained: usize) -> Self {
        Self {
            actions: Vec::with_capacity(max_retained.saturating_add(16)),
            max_retained,
        }
    }

    /// Register a new pending action as `InFlight`.
    pub fn register(&mut self, action: PendingAction) {
        self.actions.push(action);
    }

    /// Resolve an in-flight action by `action_id`, updating its status.
    /// Returns `true` if the action was found and resolved.
    pub fn resolve(&mut self, action_id: &str, success: bool, error: Option<String>) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Some(action) = self
            .actions
            .iter_mut()
            .find(|a| a.action_id == action_id && matches!(a.status, PendingActionStatus::InFlight))
        {
            action.status = match error {
                Some(err) => PendingActionStatus::Failed { completed_at: now, error: err },
                None => PendingActionStatus::Completed { completed_at: now, success },
            };
            self.prune();
            true
        } else {
            false
        }
    }

    /// Mark an in-flight action as timed out. Returns `true` if found.
    pub fn mark_timed_out(&mut self, action_id: &str) -> bool {
        if let Some(action) = self
            .actions
            .iter_mut()
            .find(|a| a.action_id == action_id && matches!(a.status, PendingActionStatus::InFlight))
        {
            action.status = PendingActionStatus::TimedOut;
            self.prune();
            true
        } else {
            false
        }
    }

    /// Returns an iterator over actions that are still `InFlight`.
    pub fn in_flight(&self) -> impl Iterator<Item = &PendingAction> {
        self.actions.iter().filter(|a| a.is_in_flight())
    }

    /// Returns an iterator over in-flight actions that have exceeded the timeout.
    pub fn stale(&self, timeout_secs: u64) -> impl Iterator<Item = &PendingAction> {
        self.actions.iter().filter(move |a| a.is_stale(timeout_secs))
    }

    /// Returns the number of in-flight actions.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight().count()
    }

    /// Check whether a specific action_id is in the pending list (any status).
    /// Used by history validation to distinguish missing outputs from pending actions.
    pub fn contains(&self, action_id: &str) -> bool {
        self.actions.iter().any(|a| a.action_id == action_id)
    }

    /// Remove completed/failed/timeout actions beyond the retention limit.
    fn prune(&mut self) {
        if self.max_retained == 0 {
            return;
        }
        // Collect indices of terminal (non-in-flight) actions, oldest first
        let terminal_indices: Vec<usize> = self
            .actions
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.is_in_flight())
            .map(|(i, _)| i)
            .collect();
        if terminal_indices.len() > self.max_retained {
            let to_remove = terminal_indices.len() - self.max_retained;
            // Remove oldest terminal actions by iterating in reverse to preserve indices
            for idx in terminal_indices.into_iter().take(to_remove).rev() {
                self.actions.remove(idx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_action(action_id: &str) -> PendingAction {
        PendingAction::new(
            action_id.to_string(),
            "test_tool".to_string(),
            serde_json::json!({}),
            ExpectedOutcome::Generic,
        )
    }

    #[test]
    fn test_register_and_resolve() {
        let mut store = PendingActions::new(10);
        store.register(make_action("call_1"));
        assert_eq!(store.in_flight_count(), 1);
        assert!(store.contains("call_1"));

        assert!(store.resolve("call_1", true, None));
        assert_eq!(store.in_flight_count(), 0);
        assert!(store.contains("call_1"));
    }

    #[test]
    fn test_resolve_with_error() {
        let mut store = PendingActions::new(10);
        store.register(make_action("call_1"));

        assert!(store.resolve("call_1", false, Some("timeout".to_string())));
        let actions: Vec<_> = store.in_flight().collect();
        assert!(actions.is_empty());
    }

    #[test]
    fn test_resolve_unknown_action() {
        let mut store = PendingActions::new(10);
        assert!(!store.resolve("unknown", true, None));
    }

    #[test]
    fn test_stale_detection() {
        let mut store = PendingActions::new(10);
        store.register(PendingAction {
            action_id: "stale_call".to_string(),
            tool_name: "slow_tool".to_string(),
            arguments: serde_json::json!({}),
            issued_at: 0, // Unix epoch — definitely stale
            expected_outcome: ExpectedOutcome::Generic,
            status: PendingActionStatus::InFlight,
        });

        let stale: Vec<_> = store.stale(1).collect(); // 1 second timeout
        assert_eq!(stale.len(), 1);
    }

    #[test]
    fn test_in_flight_count() {
        let mut store = PendingActions::new(10);
        store.register(make_action("call_1"));
        store.register(make_action("call_2"));
        assert_eq!(store.in_flight_count(), 2);

        store.resolve("call_1", true, None);
        assert_eq!(store.in_flight_count(), 1);
    }

    #[test]
    fn test_expected_outcome_side_effects() {
        assert!(ExpectedOutcome::FileModification { paths: vec!["a.txt".to_string()] }.has_side_effects());
        assert!(ExpectedOutcome::CommandExecution.has_side_effects());
        assert!(!ExpectedOutcome::ReadOperation.has_side_effects());
        assert!(!ExpectedOutcome::SearchOperation.has_side_effects());
        assert!(!ExpectedOutcome::Generic.has_side_effects());
    }

    #[test]
    fn test_pending_action_serde_roundtrip() {
        let action = make_action("call_roundtrip");
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: PendingAction = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.action_id, "call_roundtrip");
        assert!(deserialized.is_in_flight());
    }

    #[test]
    fn test_prune_retains_in_flight() {
        let mut store = PendingActions::new(1); // retain only 1 terminal
        store.register(make_action("keep"));
        store.register(make_action("prune_me"));
        store.resolve("prune_me", true, None);
        store.resolve("keep", true, None);

        // Both resolved, so oldest one should be pruned (keep is oldest, prune_me is newest)
        assert_eq!(store.in_flight_count(), 0);
        // The newer terminal action should be retained
        assert!(store.contains("prune_me"));
        // The oldest terminal action should be pruned
        assert!(!store.contains("keep"));
    }
}
