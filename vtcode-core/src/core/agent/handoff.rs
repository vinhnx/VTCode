//! Structured agent handoff protocol (Swarm pattern).
//!
//! Implements handoff types that allow one agent to transfer control to another
//! agent with full conversation context.  This follows the Swarm pattern
//! described in "The Hitchhiker's Guide to Agentic AI" §18.5.3:
//!
//! - Agents have instructions and tools.
//! - Handoffs are special tools that transfer control.
//! - Context variables are shared state passed between agents.
//! - The active agent changes dynamically based on task needs.

use serde::{Deserialize, Serialize};

/// A handoff request from one agent to another.
///
/// Carries the current state context so the receiving agent can continue
/// without re-explaining the situation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffRequest {
    /// The target agent name (subagent spec name or alias).
    pub target_agent: String,

    /// Current state summary: what was accomplished, what remains.
    pub state_summary: String,

    /// Files that have been modified so far.
    #[serde(default)]
    pub modified_files: Vec<String>,

    /// Any unresolved decisions the target needs to know about.
    #[serde(default)]
    pub open_decisions: Vec<String>,

    /// The original task context (carried through).
    pub task_context: String,

    /// Optional file attachment paths for additional context.
    #[serde(default)]
    pub context_files: Vec<String>,
}

/// The handoff receipt that the target agent writes back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffReceipt {
    /// Whether the handoff was accepted.
    pub accepted: bool,

    /// A message from the target agent about how it will proceed.
    #[serde(default)]
    pub continuation_message: String,

    /// Optional updated plan from the target agent.
    #[serde(default)]
    pub revised_plan: Option<String>,
}

impl HandoffRequest {
    /// Serialize the handoff context into a prompt for the target agent.
    pub fn to_handoff_prompt(&self) -> String {
        let mut parts = vec![
            "## HANDOFF FROM PREVIOUS AGENT".to_string(),
            String::new(),
            "Previous agent completed work and is handing off to you.".to_string(),
            String::new(),
            format!("### Current State\n{}", self.state_summary),
        ];

        if !self.modified_files.is_empty() {
            parts.push(format!(
                "### Modified Files\n{}",
                self.modified_files.join("\n")
            ));
        }

        if !self.open_decisions.is_empty() {
            parts.push(format!(
                "### Open Decisions\n{}",
                self.open_decisions.join("\n")
            ));
        }

        parts.push(format!(
            "### Task Context\n{}\n\n\
             Continue from where the previous agent left off. \
             Read the modified files to understand the current state.",
            self.task_context
        ));

        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_request_round_trip() {
        let request = HandoffRequest {
            target_agent: "coder".into(),
            state_summary: "Implemented the login feature".into(),
            modified_files: vec!["src/auth.rs".into()],
            open_decisions: vec!["Should we use JWT?".into()],
            task_context: "Build authentication system".into(),
            context_files: vec![],
        };

        let json = serde_json::to_string(&request).expect("serialize");
        let deserialized: HandoffRequest = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.target_agent, "coder");
        assert_eq!(deserialized.state_summary, "Implemented the login feature");
    }

    #[test]
    fn handoff_prompt_includes_state() {
        let request = HandoffRequest {
            target_agent: "verifier".into(),
            state_summary: "Code is ready for review".into(),
            modified_files: vec!["src/main.rs".into()],
            open_decisions: vec![],
            task_context: "Review the implementation".into(),
            context_files: vec![],
        };

        let prompt = request.to_handoff_prompt();
        assert!(prompt.contains("Code is ready for review"));
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("Review the implementation"));
        assert!(prompt.contains("HANDOFF FROM PREVIOUS AGENT"));
    }

    #[test]
    fn handoff_receipt_round_trip() {
        let receipt = HandoffReceipt {
            accepted: true,
            continuation_message: "Will review the code".into(),
            revised_plan: Some("1. Check auth".into()),
        };

        let json = serde_json::to_string(&receipt).expect("serialize");
        let deserialized: HandoffReceipt = serde_json::from_str(&json).expect("deserialize");

        assert!(deserialized.accepted);
        assert_eq!(deserialized.continuation_message, "Will review the code");
    }

    #[test]
    fn handoff_request_defaults() {
        let request = HandoffRequest {
            target_agent: "debugger".into(),
            state_summary: "Investigating crash".into(),
            task_context: "Find root cause".into(),
            modified_files: vec![],
            open_decisions: vec![],
            context_files: vec![],
        };

        assert!(request.modified_files.is_empty());
        assert!(request.open_decisions.is_empty());
        assert!(request.context_files.is_empty());
    }
}
