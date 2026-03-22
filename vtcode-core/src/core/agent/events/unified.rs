//! Unified event model for the agent core.
//! Inspired by the Pi Agent architecture.

use crate::exec::events::{ThreadEvent, ThreadItemDetails, ToolCallStatus, Usage};
use serde_json::Value;
use std::fmt;

/// Unified event model that captures all significant agent activities.
/// This is a legacy compatibility surface.
///
/// `ThreadEvent` is VT Code's canonical runtime event contract. Keep this type
/// only for adapters that still expect the older, narrower event model.
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

impl AgentEvent {
    /// Best-effort adapter from the canonical `ThreadEvent` stream.
    ///
    /// This mapping is intentionally lossy. It only converts lifecycle events
    /// that preserve the legacy `AgentEvent` meaning without inventing new
    /// semantics.
    pub fn from_thread_event_lossy(event: &ThreadEvent) -> Option<Self> {
        match event {
            ThreadEvent::ItemStarted(started) => match &started.item.details {
                ThreadItemDetails::ToolInvocation(details) => Some(Self::ToolCallStarted {
                    id: details
                        .tool_call_id
                        .clone()
                        .unwrap_or_else(|| started.item.id.clone()),
                    name: details.tool_name.clone(),
                    args: json_string(details.arguments.as_ref()),
                }),
                _ => None,
            },
            ThreadEvent::ItemCompleted(completed) => match &completed.item.details {
                ThreadItemDetails::ToolInvocation(details) => Some(Self::ToolCallCompleted {
                    id: details
                        .tool_call_id
                        .clone()
                        .unwrap_or_else(|| completed.item.id.clone()),
                    result: json_string(details.arguments.as_ref()),
                    is_success: matches!(details.status, ToolCallStatus::Completed),
                }),
                ThreadItemDetails::Error(details) => Some(Self::Error {
                    message: details.message.clone(),
                }),
                _ => None,
            },
            ThreadEvent::TurnCompleted(completed) => Some(Self::TurnCompleted {
                finish_reason: "unknown".to_string(),
                usage: completed.usage.clone(),
            }),
            ThreadEvent::Error(error) => Some(Self::Error {
                message: error.message.clone(),
            }),
            _ => None,
        }
    }
}

fn json_string(value: Option<&Value>) -> String {
    value
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::events::{
        ErrorItem, ItemCompletedEvent, ItemStartedEvent, ThreadItem, ToolInvocationItem,
    };

    #[test]
    fn adapts_tool_invocation_start_lossily() {
        let event = ThreadEvent::ItemStarted(ItemStartedEvent {
            item: ThreadItem {
                id: "item_1".to_string(),
                details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                    tool_name: "shell".to_string(),
                    arguments: Some(serde_json::json!({ "cmd": "echo hi" })),
                    tool_call_id: Some("call_1".to_string()),
                    status: ToolCallStatus::InProgress,
                }),
            },
        });

        let Some(AgentEvent::ToolCallStarted { id, name, args }) =
            AgentEvent::from_thread_event_lossy(&event)
        else {
            panic!("expected tool start adaptation");
        };

        assert_eq!(id, "call_1");
        assert_eq!(name, "shell");
        assert_eq!(args, r#"{"cmd":"echo hi"}"#);
    }

    #[test]
    fn adapts_tool_invocation_completion_lossily() {
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "item_1".to_string(),
                details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                    tool_name: "shell".to_string(),
                    arguments: Some(serde_json::json!({ "cmd": "echo hi" })),
                    tool_call_id: Some("call_1".to_string()),
                    status: ToolCallStatus::Completed,
                }),
            },
        });

        let Some(AgentEvent::ToolCallCompleted {
            id,
            result,
            is_success,
        }) = AgentEvent::from_thread_event_lossy(&event)
        else {
            panic!("expected tool completion adaptation");
        };

        assert_eq!(id, "call_1");
        assert_eq!(result, r#"{"cmd":"echo hi"}"#);
        assert!(is_success);
    }

    #[test]
    fn adapts_error_items_lossily() {
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "item_2".to_string(),
                details: ThreadItemDetails::Error(ErrorItem {
                    message: "tool denied".to_string(),
                }),
            },
        });

        let Some(AgentEvent::Error { message }) = AgentEvent::from_thread_event_lossy(&event)
        else {
            panic!("expected error adaptation");
        };

        assert_eq!(message, "tool denied");
    }
}
