//! Unified item status for Open Responses state machines.
//!
//! All items in Open Responses follow a state machine model with defined
//! lifecycle states. This module provides the canonical status enum used
//! across all item types.

use serde::{Deserialize, Serialize};

/// Lifecycle status for items in the Open Responses model.
///
/// Per the Open Responses specification, all items are state machines that
/// transition through these defined states during their lifecycle.
///
/// # State Transitions
///
/// ```text
/// ┌─────────────┐
/// │ in_progress │──────────────────────────────────┐
/// └──────┬──────┘                                  │
///        │                                         │
///        ▼                                         ▼
/// ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
/// │ incomplete  │     │  completed  │     │   failed    │
/// └─────────────┘     └─────────────┘     └─────────────┘
///   (terminal)          (terminal)          (terminal)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// The model is currently emitting tokens belonging to this item.
    #[default]
    InProgress,

    /// The model has exhausted its token budget while emitting tokens for this item.
    /// This is a terminal state. If an item ends in this state, it MUST be the last
    /// item emitted, and the containing response MUST also be in an `incomplete` state.
    Incomplete,

    /// The model has finished emitting tokens for this item, and/or a tool call
    /// has completed successfully. This is a terminal state.
    Completed,

    /// The item processing has failed. This is a terminal state.
    Failed,
}

impl ItemStatus {
    /// Returns `true` if this status is a terminal state (no further transitions allowed).
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Incomplete | Self::Completed | Self::Failed)
    }

    /// Returns `true` if this status indicates successful completion.
    #[inline]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Completed)
    }

    /// Returns `true` if this status indicates a failure.
    #[inline]
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed | Self::Incomplete)
    }
}

impl std::fmt::Display for ItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InProgress => write!(f, "in_progress"),
            Self::Incomplete => write!(f, "incomplete"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_states() {
        assert!(!ItemStatus::InProgress.is_terminal());
        assert!(ItemStatus::Incomplete.is_terminal());
        assert!(ItemStatus::Completed.is_terminal());
        assert!(ItemStatus::Failed.is_terminal());
    }

    #[test]
    fn test_success_failure() {
        assert!(!ItemStatus::InProgress.is_success());
        assert!(ItemStatus::Completed.is_success());
        assert!(!ItemStatus::Failed.is_success());

        assert!(!ItemStatus::InProgress.is_failure());
        assert!(!ItemStatus::Completed.is_failure());
        assert!(ItemStatus::Failed.is_failure());
        assert!(ItemStatus::Incomplete.is_failure());
    }

    #[test]
    fn test_serialization() {
        assert_eq!(
            serde_json::to_string(&ItemStatus::InProgress).unwrap(),
            "\"in_progress\""
        );
        assert_eq!(
            serde_json::to_string(&ItemStatus::Completed).unwrap(),
            "\"completed\""
        );
    }
}
