//! Quiet `/goal` execution for desktop clients.
//!
//! Typed slash commands submitted as turns would wedge (a non-turn slash
//! produces no terminal turn event), so the desktop client executes goal
//! commands through `goal/execute` instead. The service parses `args` with
//! the shared [`parse_goal_slash_command`](crate::slash_command::parse_goal_slash_command)
//! grammar, applies the shared runloop core, and returns render lines plus
//! the fresh snapshot; a `submit` prompt starts a real turn when set.

use serde::{Deserialize, Serialize};
use vtcode_exec_events::goal::GoalSnapshot;

/// Parameters for executing a `/goal` command against a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalExecuteParams {
    /// Session owning the goal loop.
    pub session_id: String,
    /// Raw arguments after `/goal` (empty shows status).
    pub args: String,
}

/// Result of a `/goal` execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalExecuteResult {
    /// Status lines to show the user, in order.
    pub lines: Vec<String>,
    /// Whether the tracker changed (persisted; bar refreshes from `goal`).
    pub mutated: bool,
    /// Turn prompt to submit after rendering (`Set` starts work on the
    /// objective; successful `Resume` continues it; otherwise `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit: Option<String>,
    /// Fresh snapshot after applying, or `None` when no goal exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<GoalSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_exec_events::goal::GoalState;

    #[test]
    fn goal_execute_params_use_camel_case_keys() {
        let params = GoalExecuteParams { session_id: "s".into(), args: "pause".into() };
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["sessionId"], "s");
        assert_eq!(json["args"], "pause");
    }

    #[test]
    fn goal_execute_result_round_trips_and_omits_empty_fields() {
        let full = GoalExecuteResult {
            lines: vec!["Goal set: Ship it".into()],
            mutated: true,
            submit: Some("Ship it".into()),
            goal: Some(GoalSnapshot {
                goal_id: "goal-1".into(),
                state: GoalState::Active,
                objective: "Ship it".into(),
                elapsed_ms: 0,
                tokens_used: 0,
                token_budget: None,
                pause_message: None,
            }),
        };
        let json = serde_json::to_string(&full).unwrap();
        let restored: GoalExecuteResult = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, full);

        let minimal = GoalExecuteResult {
            lines: vec![],
            mutated: false,
            submit: None,
            goal: None,
        };
        let json = serde_json::to_value(&minimal).unwrap();
        assert!(json.get("submit").is_none());
        assert!(json.get("goal").is_none());
    }
}
