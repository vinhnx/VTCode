//! Canonical durable contract for thread goal snapshots.
//!
//! Every goal mutation publishes a [`GoalUpdatedEvent`] through
//! [`ThreadEvent::GoalUpdated`](crate::ThreadEvent::GoalUpdated) so timeline
//! consumers (CLI status, desktop goal bar, session archives) replay typed
//! goal state instead of re-parsing rendered output. `goal: None` signals a
//! cleared goal; `Some` carries the user-facing surface only — identity,
//! state, objective, and progress counters.
use serde::{Deserialize, Serialize};

/// Coarse goal lifecycle state for timeline consumers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    /// Goal is actively driving turns.
    Active,
    /// Goal is paused for any reason (user, back-off, no progress, infra).
    Paused,
    /// Goal completed successfully.
    Complete,
    /// Goal was judged unachievable.
    Blocked,
    /// Goal hit its token budget.
    BudgetLimited,
    /// Catch-all for states added in newer schema versions.
    #[serde(other)]
    Unknown,
}

impl GoalState {
    /// Canonical wire string for this state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::Blocked => "blocked",
            Self::BudgetLimited => "budget_limited",
            Self::Unknown => "unknown",
        }
    }
}

/// User-facing goal snapshot: identity, state, objective, and progress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct GoalSnapshot {
    /// Stable goal id.
    pub goal_id: String,
    /// Coarse lifecycle state.
    pub state: GoalState,
    /// Human-readable objective.
    pub objective: String,
    /// Elapsed wall-clock time in milliseconds while active.
    pub elapsed_ms: u64,
    /// High-water mark of tokens spent on the goal.
    pub tokens_used: i64,
    /// Optional token budget cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<i64>,
    /// User-facing pause message, if the goal paused with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_message: Option<String>,
}

/// Goal mutation payload. `goal: None` signals a cleared goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct GoalUpdatedEvent {
    /// Current goal snapshot, or `None` when the goal was cleared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<GoalSnapshot>,
}

impl GoalSnapshot {
    /// Compact elapsed label (`17m 11s`) shared by the CLI and desktop bar.
    #[must_use]
    pub fn elapsed_label(&self) -> String {
        Self::elapsed_label_for(self.elapsed_ms)
    }

    /// Label for an explicit millisecond count, so live-ticking surfaces can
    /// render wall-clock time past the last snapshot without cloning one.
    #[must_use]
    pub fn elapsed_label_for(elapsed_ms: u64) -> String {
        let secs = elapsed_ms / 1000;
        if secs < 60 {
            return format!("{secs}s");
        }
        let mins = secs / 60;
        if mins < 60 {
            return format!("{mins}m {}s", secs % 60);
        }
        let hours = mins / 60;
        if hours < 24 {
            return format!("{hours}h {}m", mins % 60);
        }
        format!("{}d {}h", hours / 24, hours % 24)
    }

    /// Objective truncated to `max_chars` on a char boundary with a `…`
    /// suffix when shortened.
    #[must_use]
    pub fn short_objective(&self, max_chars: usize) -> String {
        if self.objective.chars().count() <= max_chars {
            return self.objective.clone();
        }
        let head: String = self.objective.chars().take(max_chars).collect();
        format!("{}…", head.trim_end())
    }

    /// Token usage label (`250 tokens`, or `250/100000 tokens` with a budget).
    #[must_use]
    pub fn tokens_label(&self) -> String {
        match self.token_budget {
            Some(limit) => format!("{}/{} tokens", self.tokens_used, limit),
            None => format!("{} tokens", self.tokens_used),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThreadEvent;

    fn snapshot() -> GoalSnapshot {
        GoalSnapshot {
            goal_id: "goal-1".to_string(),
            state: GoalState::Active,
            objective: "Ship it".to_string(),
            elapsed_ms: 11_000,
            tokens_used: 250,
            token_budget: Some(100_000),
            pause_message: None,
        }
    }

    #[test]
    fn goal_updated_event_round_trips_some_and_none() {
        let event = ThreadEvent::GoalUpdated(Box::new(GoalUpdatedEvent { goal: Some(snapshot()) }));
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"goal.updated\""), "unexpected tag: {json}");
        let restored: ThreadEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);

        let cleared = ThreadEvent::GoalUpdated(Box::new(GoalUpdatedEvent { goal: None }));
        let json = serde_json::to_string(&cleared).unwrap();
        assert!(!json.contains("pause_message"), "cleared payload must stay minimal: {json}");
        let restored: ThreadEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, cleared);
    }

    #[test]
    fn unknown_goal_state_decodes_to_unknown() {
        let state: GoalState = serde_json::from_str("\"quarantined\"").unwrap();
        assert_eq!(state, GoalState::Unknown);
        assert_eq!(GoalState::Paused.as_str(), "paused");
    }

    #[test]
    fn elapsed_label_for_matches_the_method() {
        for ms in [0, 999, 1000, 59_000, 60_000, 1_031_000, 3_600_000, 90_000_000] {
            let snapshot = GoalSnapshot { elapsed_ms: ms, ..snapshot() };
            assert_eq!(GoalSnapshot::elapsed_label_for(ms), snapshot.elapsed_label(), "{ms}ms");
        }
    }

    #[test]
    fn elapsed_label_matches_both_sides_of_each_boundary() {
        for (ms, expected) in [
            (0, "0s"),
            (59_000, "59s"),
            (60_000, "1m 0s"),
            (1_031_000, "17m 11s"),
            (3_599_000, "59m 59s"),
            (3_600_000, "1h 0m"),
            (86_399_000, "23h 59m"),
            (86_400_000, "1d 0h"),
            (90_000_000, "1d 1h"),
        ] {
            let snapshot = GoalSnapshot { elapsed_ms: ms, ..snapshot() };
            assert_eq!(snapshot.elapsed_label(), expected, "{ms}ms");
        }
    }

    #[test]
    fn short_objective_truncates_on_char_boundaries() {
        let short = GoalSnapshot { objective: "Ship it".into(), ..snapshot() };
        assert_eq!(short.short_objective(60), "Ship it");
        assert_eq!(short.short_objective(7), "Ship it");

        let accented = GoalSnapshot { objective: "héllo wörld".into(), ..snapshot() };
        assert_eq!(accented.short_objective(5), "héllo…");

        let emoji = GoalSnapshot { objective: "a😀b".into(), ..snapshot() };
        assert_eq!(emoji.short_objective(2), "a😀…");
        assert_eq!(emoji.short_objective(3), "a😀b");
    }

    #[test]
    fn tokens_label_shows_budget_only_when_set() {
        let unbounded = GoalSnapshot { tokens_used: 250, token_budget: None, ..snapshot() };
        assert_eq!(unbounded.tokens_label(), "250 tokens");
        let bounded = GoalSnapshot {
            tokens_used: 250,
            token_budget: Some(100_000),
            ..snapshot()
        };
        assert_eq!(bounded.tokens_label(), "250/100000 tokens");
    }
}
