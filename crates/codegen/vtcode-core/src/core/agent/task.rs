//! Task-related data structures shared across the agent runner modules.

use crate::exec::events::ThreadCompletionSubtype;
use crate::exec::events::ThreadEvent;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Task specification consumed by the benchmark/autonomous runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Stable identifier for reporting.
    pub id: String,
    /// Human-readable task title displayed in progress messages.
    pub title: String,
    /// High-level description of the task objective.
    pub description: String,
    /// Optional explicit instructions appended to the conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl Task {
    /// Construct a task with the provided metadata.
    pub fn new(id: String, title: String, description: String) -> Self {
        Self { id, title, description, instructions: None }
    }
}

/// Context entry supplied alongside the benchmark task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    /// Identifier used when referencing the context in prompts.
    pub id: String,
    /// Raw textual content exposed to the agent.
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    Success,
    StoppedNoAction,
    TurnLimitReached {
        max_turns: usize,
        actual_turns: usize,
    },
    BudgetLimitReached {
        max_budget_usd: f64,
        actual_cost_usd: f64,
    },
    /// Legacy outcome of the removed per-turn tool-loop budget, kept so older
    /// serialized artifacts still deserialize. No longer produced by the
    /// config-driven budget; a rate-limit safeguard halt may still construct
    /// it with `max_tool_loops: 0`.
    ToolLoopLimitReached {
        max_tool_loops: usize,
        actual_tool_loops: usize,
    },
    LoopDetected,
    Cancelled,
    HandedOff {
        target: String,
    },
    Escalated {
        reason: String,
        tool_name: String,
    },
    /// The provider declined the request (`FinishReason::Refusal`). Terminal
    /// for the prompt: resending it is refused again. `reason` is the
    /// user-facing notice from [`crate::core::agent::refusal::refusal_reason`].
    Refused {
        reason: String,
    },
    Failed {
        reason: String,
        /// What was accomplished before the failure occurred.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        accomplished: Vec<String>,
        /// Optional suggestion for recovery or escalation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovery_suggestion: Option<String>,
        /// Optional path to preserved state for task resume.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_path: Option<String>,
    },
    Unknown,
}

impl TaskOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success | Self::StoppedNoAction)
    }

    pub fn is_hard_block(&self) -> bool {
        matches!(self, Self::ToolLoopLimitReached { .. } | Self::LoopDetected)
    }

    pub fn description(&self) -> String {
        match self {
            Self::Success => "Task completed successfully".into(),
            Self::StoppedNoAction => "Stopped after agent signaled no further actions".into(),
            Self::TurnLimitReached { max_turns, actual_turns } => {
                format!("Stopped after reaching turn limit (max: {max_turns}, reached: {actual_turns})")
            }
            Self::BudgetLimitReached { max_budget_usd, actual_cost_usd } => {
                format!("Stopped after reaching budget limit (max: ${max_budget_usd:.4}, spent: ${actual_cost_usd:.4})")
            }
            Self::ToolLoopLimitReached { max_tool_loops, actual_tool_loops } => {
                if *max_tool_loops == 0 {
                    "Stopped after a tool safeguard halted execution".into()
                } else {
                    format!(
                        "Stopped after reaching tool loop limit (max: {max_tool_loops}, reached: {actual_tool_loops})"
                    )
                }
            }
            Self::LoopDetected => "Stopped due to infinite loop detection".into(),
            Self::Cancelled => "Task cancelled by user".into(),
            Self::HandedOff { target } => format!("Task handed off to `{target}`"),
            Self::Escalated { reason, tool_name } => {
                format!("Task escalated: {tool_name} — {reason}")
            }
            Self::Refused { reason } => reason.clone(),
            Self::Failed { reason, accomplished, recovery_suggestion, .. } => {
                let mut parts = vec![format!("Task failed: {reason}")];
                if !accomplished.is_empty() {
                    parts.push(format!("Accomplished: {}", accomplished.join(", ")));
                }
                if let Some(suggestion) = recovery_suggestion {
                    parts.push(format!("Suggestion: {suggestion}"));
                }
                parts.join("\n")
            }
            Self::Unknown => "Task outcome could not be determined".into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::StoppedNoAction => "stopped_no_action",
            Self::TurnLimitReached { .. } => "turn_limit_reached",
            Self::BudgetLimitReached { .. } => "budget_limit_reached",
            Self::ToolLoopLimitReached { .. } => "tool_loop_limit_reached",
            Self::LoopDetected => "loop_detected",
            Self::Cancelled => "cancelled",
            Self::HandedOff { .. } => "handed_off",
            Self::Escalated { .. } => "escalated",
            Self::Refused { .. } => "refused",
            Self::Failed { .. } => "failed",
            Self::Unknown => "unknown",
        }
    }

    pub fn thread_completion_subtype(&self) -> ThreadCompletionSubtype {
        match self {
            Self::Success | Self::StoppedNoAction => ThreadCompletionSubtype::Success,
            Self::TurnLimitReached { .. } => ThreadCompletionSubtype::ErrorMaxTurns,
            Self::BudgetLimitReached { .. } => ThreadCompletionSubtype::ErrorMaxBudgetUsd,
            Self::Cancelled => ThreadCompletionSubtype::Cancelled,
            Self::ToolLoopLimitReached { .. }
            | Self::LoopDetected
            | Self::HandedOff { .. }
            | Self::Escalated { .. }
            | Self::Refused { .. }
            | Self::Failed { .. }
            | Self::Unknown => ThreadCompletionSubtype::ErrorDuringExecution,
        }
    }

    pub fn success() -> Self {
        Self::Success
    }

    pub fn turn_limit_reached(max_turns: usize, actual_turns: usize) -> Self {
        Self::TurnLimitReached { max_turns, actual_turns }
    }

    pub fn budget_limit_reached(max_budget_usd: f64, actual_cost_usd: f64) -> Self {
        Self::BudgetLimitReached { max_budget_usd, actual_cost_usd }
    }

    /// Safeguard halt from a rate-limited tool (not the removed configured
    /// budget): reuses the legacy loop-limit outcome with `max_tool_loops: 0`
    /// so existing consumers keep one "tool safeguard stopped the run" shape.
    pub fn tool_safeguard_halt(actual_tool_loops: usize) -> Self {
        Self::ToolLoopLimitReached { max_tool_loops: 0, actual_tool_loops }
    }

    pub fn failed(
        reason: String,
        accomplished: Vec<String>,
        recovery_suggestion: Option<String>,
        checkpoint_path: Option<String>,
    ) -> Self {
        Self::Failed {
            reason,
            accomplished,
            recovery_suggestion,
            checkpoint_path,
        }
    }

    pub fn escalated(reason: String, tool_name: String) -> Self {
        Self::Escalated { reason, tool_name }
    }

    pub fn refused(reason: String) -> Self {
        Self::Refused { reason }
    }

    pub fn handed_off(target: String) -> Self {
        Self::HandedOff { target }
    }
}

impl fmt::Display for TaskOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::{TaskOutcome, ThreadCompletionSubtype};

    #[test]
    fn tool_safeguard_halt_renders_without_a_budget_count() {
        let outcome = TaskOutcome::tool_safeguard_halt(0);

        assert!(outcome.is_hard_block());
        assert_eq!(outcome.code(), "tool_loop_limit_reached");
        assert_eq!(outcome.description(), "Stopped after a tool safeguard halted execution");
    }

    #[test]
    fn thread_completion_subtype_matches_public_result_states() {
        assert_eq!(TaskOutcome::Success.thread_completion_subtype(), ThreadCompletionSubtype::Success);
        assert_eq!(TaskOutcome::StoppedNoAction.thread_completion_subtype(), ThreadCompletionSubtype::Success);
        assert_eq!(
            TaskOutcome::turn_limit_reached(3, 3).thread_completion_subtype(),
            ThreadCompletionSubtype::ErrorMaxTurns
        );
        assert_eq!(
            TaskOutcome::budget_limit_reached(1.0, 1.2).thread_completion_subtype(),
            ThreadCompletionSubtype::ErrorMaxBudgetUsd
        );
        assert_eq!(TaskOutcome::Cancelled.thread_completion_subtype(), ThreadCompletionSubtype::Cancelled);
        assert_eq!(
            (TaskOutcome::failed("boom".to_string(), vec![], None, None)).thread_completion_subtype(),
            ThreadCompletionSubtype::ErrorDuringExecution
        );
        assert_eq!(
            TaskOutcome::refused("declined".to_string()).thread_completion_subtype(),
            ThreadCompletionSubtype::ErrorDuringExecution
        );
    }

    #[test]
    fn refused_outcome_is_a_distinct_failure_that_carries_the_reason() {
        let outcome = TaskOutcome::refused("The model declined this request.".to_string());

        assert!(!outcome.is_success());
        assert!(!outcome.is_hard_block());
        assert_eq!(outcome.code(), "refused");
        assert_eq!(outcome.description(), "The model declined this request.");

        let json = serde_json::to_value(&outcome).expect("serialize refused outcome");
        assert_eq!(json, serde_json::json!({"refused": {"reason": "The model declined this request."}}));
        let round_trip: TaskOutcome = serde_json::from_value(json).expect("deserialize refused outcome");
        assert_eq!(round_trip, outcome);
    }
}

/// Aggregated results returned by the autonomous agent runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResults {
    /// Identifiers of any contexts created during execution.
    #[serde(default)]
    pub created_contexts: Vec<String>,
    /// File paths modified during the task.
    #[serde(default)]
    pub modified_files: Vec<String>,
    /// Terminal commands executed while solving the task.
    #[serde(default)]
    pub executed_commands: Vec<String>,
    /// Natural-language summary of the run assembled by the agent.
    pub summary: String,
    /// Provider stop reason associated with the last model turn, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Estimated total API cost in USD, when pricing metadata is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    /// Collected warnings emitted while processing the task.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Structured execution timeline for headless modes.
    #[serde(default)]
    pub thread_events: Vec<ThreadEvent>,
    /// Finalized outcome of the task.
    pub outcome: TaskOutcome,
    /// Number of autonomous turns executed.
    pub turns_executed: usize,
    /// Total runtime in milliseconds.
    pub total_duration_ms: u128,
    /// Average turn duration in milliseconds (if turns executed).
    #[serde(default)]
    pub average_turn_duration_ms: Option<f64>,
    /// Longest individual turn duration in milliseconds.
    #[serde(default)]
    pub max_turn_duration_ms: Option<u128>,
    /// Per-turn duration metrics in milliseconds.
    #[serde(default)]
    pub turn_durations_ms: Vec<u128>,
}
