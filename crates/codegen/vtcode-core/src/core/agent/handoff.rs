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

/// Status of a single feature or deliverable at handoff time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryStatus {
    /// Completed and verified.
    Done,
    /// Actively being worked on.
    InProgress,
    /// Not yet started.
    NotStarted,
    /// Cannot proceed without external input or a replan.
    Blocked,
}

/// A single item in the boundary list that explicitly marks what is done,
/// in-progress, or not started. This prevents the next agent from guessing
/// whether something is intentionally incomplete or a leftover mess.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryItem {
    /// Feature or deliverable name.
    pub feature: String,
    /// Current status.
    pub status: BoundaryStatus,
    /// Optional notes (e.g. "blocked on API key", "tests pass but UI needs polish").
    #[serde(default)]
    pub notes: Option<String>,
}

/// A handoff request from one agent to another.
///
/// Carries the current state context so the receiving agent can continue
/// without re-explaining the situation. Enriched with test results, boundary
/// status, known issues, and recommended next actions following the long-running
/// harness pattern: the next agent must be able to orient from this artifact alone.
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

    /// Last test run outcome: pass/fail summary with actual output.
    /// None if no tests were run in this session.
    #[serde(default)]
    pub test_results: Option<String>,

    /// Explicit boundary list: what is done, in-progress, not started, or blocked.
    /// Prevents the next agent from guessing intent vs. incompleteness.
    #[serde(default)]
    pub boundary_status: Vec<BoundaryItem>,

    /// Known issues the next agent should be aware of (bugs, limitations, tech debt).
    #[serde(default)]
    pub known_issues: Vec<String>,

    /// Recommended next actions for the receiving agent.
    #[serde(default)]
    pub next_actions: Vec<String>,
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
    ///
    /// The prompt follows the long-running harness pattern: it gives the next
    /// agent everything it needs to orient -- state, boundaries, test results,
    /// known issues, and recommended next actions -- so it can pick up without
    /// re-exploring the codebase from scratch.
    pub fn to_handoff_prompt(&self) -> String {
        let mut parts = vec![
            "## HANDOFF FROM PREVIOUS AGENT".to_string(),
            String::new(),
            "Previous agent completed work and is handing off to you.".to_string(),
            String::new(),
            format!("### Current State\n{}", self.state_summary),
        ];

        if !self.boundary_status.is_empty() {
            let items: Vec<String> = self
                .boundary_status
                .iter()
                .map(|item| {
                    let status_str = match item.status {
                        BoundaryStatus::Done => "DONE",
                        BoundaryStatus::InProgress => "IN PROGRESS",
                        BoundaryStatus::NotStarted => "NOT STARTED",
                        BoundaryStatus::Blocked => "BLOCKED",
                    };
                    match &item.notes {
                        Some(notes) => format!("- [{}] {} -- {}", status_str, item.feature, notes),
                        None => format!("- [{}] {}", status_str, item.feature),
                    }
                })
                .collect();
            parts.push(format!("### Boundary Status\n{}", items.join("\n")));
        }

        if !self.modified_files.is_empty() {
            parts.push(format!("### Modified Files\n{}", self.modified_files.join("\n")));
        }

        if let Some(test_results) = &self.test_results {
            parts.push(format!("### Test Results\n{test_results}"));
        }

        if !self.open_decisions.is_empty() {
            parts.push(format!("### Open Decisions\n{}", self.open_decisions.join("\n")));
        }

        if !self.known_issues.is_empty() {
            parts.push(format!(
                "### Known Issues\n{}",
                self.known_issues
                    .iter()
                    .map(|i| format!("- {i}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if !self.next_actions.is_empty() {
            parts.push(format!(
                "### Recommended Next Actions\n{}",
                self.next_actions
                    .iter()
                    .map(|a| format!("1. {a}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        parts.push(format!(
            "### Task Context\n{}\n\n\
             Continue from where the previous agent left off. \
             Read the modified files to understand the current state. \
             Check boundary status to know what's done vs. what needs work.",
            self.task_context
        ));

        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_request() -> HandoffRequest {
        HandoffRequest {
            target_agent: "coder".into(),
            state_summary: "Implemented the login feature".into(),
            modified_files: vec!["src/auth.rs".into()],
            open_decisions: vec!["Should we use JWT?".into()],
            task_context: "Build authentication system".into(),
            context_files: vec![],
            test_results: None,
            boundary_status: vec![],
            known_issues: vec![],
            next_actions: vec![],
        }
    }

    #[test]
    fn handoff_request_round_trip() {
        let request = minimal_request();
        let json = serde_json::to_string(&request).expect("serialize");
        let deserialized: HandoffRequest = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.target_agent, "coder");
        assert_eq!(deserialized.state_summary, "Implemented the login feature");
    }

    #[test]
    fn handoff_request_round_trip_with_new_fields() {
        let request = HandoffRequest {
            target_agent: "verifier".into(),
            state_summary: "All features implemented".into(),
            modified_files: vec!["src/main.rs".into()],
            open_decisions: vec![],
            task_context: "Review the implementation".into(),
            context_files: vec![],
            test_results: Some("12 passed, 0 failed".into()),
            boundary_status: vec![
                BoundaryItem {
                    feature: "login".into(),
                    status: BoundaryStatus::Done,
                    notes: Some("JWT auth working".into()),
                },
                BoundaryItem {
                    feature: "signup".into(),
                    status: BoundaryStatus::InProgress,
                    notes: None,
                },
            ],
            known_issues: vec!["Rate limiting not implemented".into()],
            next_actions: vec!["Add rate limiting".into(), "Write integration tests".into()],
        };

        let json = serde_json::to_string(&request).expect("serialize");
        let deserialized: HandoffRequest = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.test_results.as_deref(), Some("12 passed, 0 failed"));
        assert_eq!(deserialized.boundary_status.len(), 2);
        assert_eq!(deserialized.boundary_status[0].status, BoundaryStatus::Done);
        assert_eq!(deserialized.boundary_status[1].status, BoundaryStatus::InProgress);
        assert_eq!(deserialized.known_issues.len(), 1);
        assert_eq!(deserialized.next_actions.len(), 2);
    }

    #[test]
    fn handoff_prompt_includes_state() {
        let request = minimal_request();
        let prompt = request.to_handoff_prompt();
        assert!(!prompt.contains("Code is ready for review")); // not in minimal
        assert!(prompt.contains("src/auth.rs"));
        assert!(prompt.contains("Build authentication system"));
        assert!(prompt.contains("HANDOFF FROM PREVIOUS AGENT"));
    }

    #[test]
    fn handoff_prompt_includes_boundary_status() {
        let request = HandoffRequest {
            target_agent: "verifier".into(),
            state_summary: "Code ready".into(),
            modified_files: vec![],
            open_decisions: vec![],
            task_context: "Review".into(),
            context_files: vec![],
            test_results: None,
            boundary_status: vec![
                BoundaryItem {
                    feature: "auth".into(),
                    status: BoundaryStatus::Done,
                    notes: None,
                },
                BoundaryItem {
                    feature: "api".into(),
                    status: BoundaryStatus::InProgress,
                    notes: Some("endpoints working, tests pending".into()),
                },
            ],
            known_issues: vec![],
            next_actions: vec![],
        };

        let prompt = request.to_handoff_prompt();
        assert!(prompt.contains("[DONE] auth"));
        assert!(prompt.contains("[IN PROGRESS] api -- endpoints working, tests pending"));
    }

    #[test]
    fn handoff_prompt_includes_test_results() {
        let request = HandoffRequest {
            target_agent: "verifier".into(),
            state_summary: "Done".into(),
            modified_files: vec![],
            open_decisions: vec![],
            task_context: "Review".into(),
            context_files: vec![],
            test_results: Some(
                "15 passed, 2 failed\n  FAIL: test_login_rate_limit\n  FAIL: test_signup_validation".into(),
            ),
            boundary_status: vec![],
            known_issues: vec!["Rate limiting missing".into()],
            next_actions: vec!["Fix failing tests".into()],
        };

        let prompt = request.to_handoff_prompt();
        assert!(prompt.contains("### Test Results"));
        assert!(prompt.contains("15 passed, 2 failed"));
        assert!(prompt.contains("### Known Issues"));
        assert!(prompt.contains("- Rate limiting missing"));
        assert!(prompt.contains("### Recommended Next Actions"));
        assert!(prompt.contains("1. Fix failing tests"));
    }

    #[test]
    fn handoff_prompt_omits_empty_sections() {
        let request = minimal_request();
        let prompt = request.to_handoff_prompt();
        // These sections should be absent when empty/None
        assert!(!prompt.contains("### Boundary Status"));
        assert!(!prompt.contains("### Test Results"));
        assert!(!prompt.contains("### Known Issues"));
        assert!(!prompt.contains("### Recommended Next Actions"));
        // These should always be present
        assert!(prompt.contains("### Current State"));
        assert!(prompt.contains("### Modified Files"));
        assert!(prompt.contains("### Open Decisions"));
        assert!(prompt.contains("### Task Context"));
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
    fn boundary_status_default_deserialization() {
        // Verify that old-format JSON without new fields still deserializes
        let json = r#"{
            "target_agent": "coder",
            "state_summary": "done",
            "task_context": "build",
            "modified_files": [],
            "open_decisions": [],
            "context_files": []
        }"#;
        let request: HandoffRequest = serde_json::from_str(json).expect("deserialize");
        assert!(request.test_results.is_none());
        assert!(request.boundary_status.is_empty());
        assert!(request.known_issues.is_empty());
        assert!(request.next_actions.is_empty());
    }
}
