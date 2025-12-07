//! Task-related data structures shared across the agent runner modules.

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
        Self {
            id,
            title,
            description,
            instructions: None,
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    Success,
    StoppedNoAction,
    TurnLimitReached {
        max_turns: usize,
        actual_turns: usize,
    },
    ToolLoopLimitReached {
        max_tool_loops: usize,
        actual_tool_loops: usize,
    },
    LoopDetected,
    Unknown,
}

impl TaskOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success | Self::StoppedNoAction)
    }

    pub fn description(&self) -> String {
        match self {
            Self::Success => "Task completed successfully".into(),
            Self::StoppedNoAction => "Stopped after agent signaled no further actions".into(),
            Self::TurnLimitReached {
                max_turns,
                actual_turns,
            } => format!(
                "Stopped after reaching turn limit (max: {}, reached: {})",
                max_turns, actual_turns
            ),
            Self::ToolLoopLimitReached {
                max_tool_loops,
                actual_tool_loops,
            } => format!(
                "Stopped after reaching tool loop limit (max: {}, reached: {})",
                max_tool_loops, actual_tool_loops
            ),
            Self::LoopDetected => "Stopped due to infinite loop detection".into(),
            Self::Unknown => "Task outcome could not be determined".into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::StoppedNoAction => "stopped_no_action",
            Self::TurnLimitReached { .. } => "turn_limit_reached",
            Self::ToolLoopLimitReached { .. } => "tool_loop_limit_reached",
            Self::LoopDetected => "loop_detected",
            Self::Unknown => "unknown",
        }
    }

    pub fn success() -> Self {
        Self::Success
    }

    pub fn turn_limit_reached(max_turns: usize, actual_turns: usize) -> Self {
        Self::TurnLimitReached {
            max_turns,
            actual_turns,
        }
    }

    pub fn tool_loop_limit_reached(max_tool_loops: usize, actual_tool_loops: usize) -> Self {
        Self::ToolLoopLimitReached {
            max_tool_loops,
            actual_tool_loops,
        }
    }
}

impl fmt::Display for TaskOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
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
