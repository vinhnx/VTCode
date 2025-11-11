use super::justification::ToolJustification;
use super::risk_scorer::RiskLevel;
/// Justification Extraction from Agent Context
///
/// Extracts agent reasoning from decision ledger to create tool justifications
/// for high-risk tool approvals.
use crate::core::decision_tracker::{Decision, DecisionTracker};

/// Extractor for tool justifications from agent decision context
pub struct JustificationExtractor;

impl JustificationExtractor {
    /// Extract a justification from the latest decision context
    ///
    /// Returns a ToolJustification with the agent's reasoning if available,
    /// or None if no relevant reasoning is found.
    pub fn extract_from_decision(
        decision: &Decision,
        tool_name: &str,
        risk_level: &RiskLevel,
    ) -> Option<ToolJustification> {
        // Only create justification for medium/high/critical risk tools
        match risk_level {
            RiskLevel::Low => None,
            _ => {
                // Use the decision's reasoning as the justification
                if decision.reasoning.is_empty() {
                    return None;
                }

                let just = ToolJustification::new(tool_name, &decision.reasoning, risk_level);

                // Add expected outcome if available from the decision action
                Some(just)
            }
        }
    }

    /// Extract justification from the most recent decision in the tracker
    pub fn extract_latest_from_tracker(
        tracker: &DecisionTracker,
        tool_name: &str,
        risk_level: &RiskLevel,
    ) -> Option<ToolJustification> {
        tracker
            .latest_decision()
            .and_then(|decision| Self::extract_from_decision(decision, tool_name, risk_level))
    }

    /// Create a brief justification from multiple recent decisions
    /// Useful for multi-step operations
    pub fn extract_from_recent_decisions(
        tracker: &DecisionTracker,
        tool_name: &str,
        risk_level: &RiskLevel,
        depth: usize,
    ) -> Option<ToolJustification> {
        let decisions = tracker.recent_decisions(depth);
        if decisions.is_empty() {
            return None;
        }

        // Combine reasoning from recent decisions
        let combined_reasoning = decisions
            .iter()
            .filter(|d| !d.reasoning.is_empty())
            .map(|d| d.reasoning.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        if combined_reasoning.is_empty() {
            return None;
        }

        Some(ToolJustification::new(
            tool_name,
            combined_reasoning,
            risk_level,
        ))
    }

    /// Suggest justification based on tool name and context
    /// Used as fallback when decision context doesn't have explicit reasoning
    pub fn suggest_default_justification(
        tool_name: &str,
        risk_level: &RiskLevel,
    ) -> Option<ToolJustification> {
        let (reason, outcome) = match tool_name {
            "run_command" | "execute" => (
                "Execute command to perform necessary system operation or build/test task.",
                Some("Will capture command output for analysis and decision-making."),
            ),
            "write_file" | "create_file" | "edit_file" => (
                "Modify code or configuration to implement necessary changes.",
                Some("Will create/update file with the generated content."),
            ),
            "grep_file" | "find_files" | "list_files" => (
                "Search or list files to understand codebase structure.",
                Some("Will return matching files and their contents for analysis."),
            ),
            "delete_file" | "remove_file" => (
                "Remove unnecessary or generated files as part of cleanup.",
                Some("Will delete the specified file(s)."),
            ),
            "apply_patch" => (
                "Apply code changes to fix issues or implement features.",
                Some("Will apply the patch and verify the changes."),
            ),
            _ => return None,
        };

        let just = ToolJustification::new(tool_name, reason, risk_level);
        if let Some(outcome_str) = outcome {
            Some(just.with_outcome(outcome_str))
        } else {
            Some(just)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::decision_tracker::{Action, DecisionContext, DecisionOutcome, ResponseType};
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_test_decision(reasoning: &str) -> Decision {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Decision {
            id: "test-1".to_string(),
            timestamp: now,
            context: DecisionContext {
                conversation_turn: 1,
                user_input: Some("analyze code".to_string()),
                previous_actions: vec![],
                available_tools: vec!["read_file".to_string()],
                current_state: HashMap::new(),
            },
            reasoning: reasoning.to_string(),
            action: Action::ToolCall {
                name: "read_file".to_string(),
                args: serde_json::json!({"path": "src/main.rs"}),
                expected_outcome: "Get file contents".to_string(),
            },
            outcome: Some(DecisionOutcome::Success {
                result: "File read successfully".to_string(),
                metrics: HashMap::new(),
            }),
            confidence_score: Some(0.95),
        }
    }

    #[test]
    fn test_extract_from_decision_low_risk() {
        let decision = create_test_decision("Read the source file");
        let just =
            JustificationExtractor::extract_from_decision(&decision, "read_file", &RiskLevel::Low);

        assert!(just.is_none()); // Low risk shouldn't generate justification
    }

    #[test]
    fn test_extract_from_decision_high_risk() {
        let decision = create_test_decision("Need to understand code structure deeply");
        let just = JustificationExtractor::extract_from_decision(
            &decision,
            "run_command",
            &RiskLevel::High,
        );

        assert!(just.is_some());
        let just = just.unwrap();
        assert_eq!(just.tool_name, "run_command");
        assert_eq!(just.reason, "Need to understand code structure deeply");
        assert_eq!(just.risk_level, "High");
    }

    #[test]
    fn test_extract_from_decision_empty_reasoning() {
        let decision = create_test_decision("");
        let just = JustificationExtractor::extract_from_decision(
            &decision,
            "run_command",
            &RiskLevel::High,
        );

        assert!(just.is_none()); // Empty reasoning should return None
    }

    #[test]
    fn test_suggest_default_justification() {
        let just =
            JustificationExtractor::suggest_default_justification("run_command", &RiskLevel::High);

        assert!(just.is_some());
        let just = just.unwrap();
        assert!(just.reason.contains("Execute command"));
        assert!(just.expected_outcome.is_some());
    }

    #[test]
    fn test_suggest_default_for_write_file() {
        let just =
            JustificationExtractor::suggest_default_justification("write_file", &RiskLevel::Medium);

        assert!(just.is_some());
        let just = just.unwrap();
        assert!(just.reason.contains("Modify code"));
    }

    #[test]
    fn test_suggest_default_for_unknown_tool() {
        let just =
            JustificationExtractor::suggest_default_justification("unknown_tool", &RiskLevel::High);

        assert!(just.is_none()); // Unknown tools should return None
    }
}
