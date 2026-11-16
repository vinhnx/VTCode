//! JSON schemas for the thread goal loop tools.
//!
//! The runloop registers `get_goal`/`update_goal` per session with the live
//! tracker, so the model can read goal state and declare completion, an
//! external blocker, or a user-requested pause. Descriptions carry the full
//! usage contract; no prompt-guideline rule is needed.

use serde_json::{Value, json};

pub fn get_goal_description() -> &'static str {
    "Read the current thread goal loop state. Returns the goal snapshot (status, objective, elapsed time, token usage) or null when no goal is set. Use it to check what you are pursuing before a long task, and to verify status before completing."
}

#[must_use]
pub fn get_goal_parameters() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {}
    })
}

pub fn update_goal_description() -> &'static str {
    "Update the active thread goal loop. Actions: complete (ONLY after verifying every part of the objective against current evidence — the loop stops and only the user can set a new goal), block (ONLY for a genuine external blocker you cannot resolve: missing credentials, unavailable service, or a required user decision — name it in reason), pause (ONLY when the user explicitly asked to pause). Requires a non-empty reason. Fails when no goal is active or the transition is invalid."
}

#[must_use]
pub fn update_goal_parameters() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["action", "reason"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["complete", "block", "pause"],
                "description": "Goal transition to apply."
            },
            "reason": {
                "type": "string",
                "minLength": 1,
                "description": "Why: completion summary, the external blocker, or the pause request source."
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_descriptions_state_the_contract() {
        assert!(get_goal_description().contains("null when no goal is set"));
        let update = update_goal_description();
        assert!(update.contains("ONLY after verifying"), "{update}");
        assert!(update.contains("ONLY for a genuine external blocker"), "{update}");
        assert!(update.contains("ONLY when the user explicitly asked to pause"), "{update}");
    }

    #[test]
    fn goal_parameters_declare_actions_and_reason() {
        let get = get_goal_parameters();
        assert_eq!(get["type"], "object");

        let update = update_goal_parameters();
        assert_eq!(update["required"], json!(["action", "reason"]));
        assert_eq!(update["properties"]["action"]["enum"], json!(["complete", "block", "pause"]));
        assert_eq!(update["properties"]["reason"]["minLength"], 1);
    }
}
