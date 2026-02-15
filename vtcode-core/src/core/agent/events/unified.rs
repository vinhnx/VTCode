//! Unified event model for the agent core.
//! Inspired by the Pi Agent architecture.

use crate::exec::events::Usage;
use std::fmt;

/// Unified event model that captures all significant agent activities.
/// This enum is intended to be the single source of truth for both Chat and Exec engines.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A new turn has started.
    TurnStarted {
        /// Unique ID for this turn.
        id: String,
    },

    /// The agent has entered a specific thinking stage.
    ThinkingStage {
        /// The stage name (e.g., "analysis", "plan", "verification").
        stage: String,
    },

    /// A delta of reasoning text from the model.
    ThinkingDelta {
        /// The new reasoning fragment.
        delta: String,
    },

    /// A delta of output text from the model.
    OutputDelta {
        /// The new output fragment.
        delta: String,
    },

    /// A tool call has been initiated by the model.
    ToolCallStarted {
        /// Unique ID for this tool call.
        id: String,
        /// The name of the tool being called.
        name: String,
        /// The arguments passed to the tool (as a JSON string).
        args: String,
    },

    /// A tool call has completed.
    ToolCallCompleted {
        /// Unique ID for this tool call.
        id: String,
        /// The result of the tool execution (as a JSON string).
        result: String,
        /// Whether the tool execution was successful.
        is_success: bool,
    },

    /// The current turn has completed.
    TurnCompleted {
        /// The reason the turn finished (e.g., "stop", "tool_calls").
        finish_reason: String,
        /// Resource usage for this turn.
        usage: Usage,
    },

    /// An error occurred during the turn.
    Error {
        /// Human-readable error message.
        message: String,
    },
}

impl fmt::Display for AgentEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TurnStarted { id } => write!(f, "TurnStarted({})", id),
            Self::ThinkingStage { stage } => write!(f, "ThinkingStage({})", stage),
            Self::ThinkingDelta { delta } => write!(f, "ThinkingDelta({} chars)", delta.len()),
            Self::OutputDelta { delta } => write!(f, "OutputDelta({} chars)", delta.len()),
            Self::ToolCallStarted { id, name, .. } => {
                write!(f, "ToolCallStarted({}, {})", id, name)
            }
            Self::ToolCallCompleted { id, is_success, .. } => {
                write!(f, "ToolCallCompleted({}, success={})", id, is_success)
            }
            Self::TurnCompleted { finish_reason, .. } => {
                write!(f, "TurnCompleted(reason={})", finish_reason)
            }
            Self::Error { message } => write!(f, "Error({})", message),
        }
    }
}
