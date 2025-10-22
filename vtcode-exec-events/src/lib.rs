//! Structured execution telemetry events shared across VTCode crates.
//!
//! This crate exposes the serialized schema for thread lifecycle updates,
//! command execution results, and other timeline artifacts emitted by the
//! automation runtime. Downstream applications can deserialize these
//! structures to drive dashboards, logging, or auditing pipelines without
//! depending on the full `vtcode-core` crate.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Semantic version of the serialized event schema exported by this crate.
pub const EVENT_SCHEMA_VERSION: &str = "0.1.0";

/// Wraps a [`ThreadEvent`] with schema metadata so downstream consumers can
/// negotiate compatibility before processing an event stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionedThreadEvent {
    /// Semantic version describing the schema of the nested event payload.
    pub schema_version: String,
    /// Concrete event emitted by the agent runtime.
    pub event: ThreadEvent,
}

impl VersionedThreadEvent {
    /// Creates a new [`VersionedThreadEvent`] using the current
    /// [`EVENT_SCHEMA_VERSION`].
    pub fn new(event: ThreadEvent) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION.to_string(),
            event,
        }
    }

    /// Returns the nested [`ThreadEvent`], consuming the wrapper.
    pub fn into_event(self) -> ThreadEvent {
        self.event
    }
}

impl From<ThreadEvent> for VersionedThreadEvent {
    fn from(event: ThreadEvent) -> Self {
        Self::new(event)
    }
}

/// Sink for processing [`ThreadEvent`] instances.
pub trait EventEmitter {
    /// Invoked for each event emitted by the automation runtime.
    fn emit(&mut self, event: &ThreadEvent);
}

impl<F> EventEmitter for F
where
    F: FnMut(&ThreadEvent),
{
    fn emit(&mut self, event: &ThreadEvent) {
        self(event);
    }
}

/// JSON helper utilities for serializing and deserializing thread events.
#[cfg(feature = "serde-json")]
pub mod json {
    use super::{ThreadEvent, VersionedThreadEvent};

    /// Converts an event into a `serde_json::Value`.
    pub fn to_value(event: &ThreadEvent) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(event)
    }

    /// Serializes an event into a JSON string.
    pub fn to_string(event: &ThreadEvent) -> serde_json::Result<String> {
        serde_json::to_string(event)
    }

    /// Deserializes an event from a JSON string.
    pub fn from_str(payload: &str) -> serde_json::Result<ThreadEvent> {
        serde_json::from_str(payload)
    }

    /// Serializes a [`VersionedThreadEvent`] wrapper.
    pub fn versioned_to_string(event: &ThreadEvent) -> serde_json::Result<String> {
        serde_json::to_string(&VersionedThreadEvent::new(event.clone()))
    }

    /// Deserializes a [`VersionedThreadEvent`] wrapper.
    pub fn versioned_from_str(payload: &str) -> serde_json::Result<VersionedThreadEvent> {
        serde_json::from_str(payload)
    }
}

#[cfg(feature = "telemetry-log")]
mod log_support {
    use log::Level;

    use super::{EventEmitter, ThreadEvent, json};

    /// Emits JSON serialized events to the `log` facade at the configured level.
    #[derive(Debug, Clone)]
    pub struct LogEmitter {
        level: Level,
    }

    impl LogEmitter {
        /// Creates a new [`LogEmitter`] that logs at the provided [`Level`].
        pub fn new(level: Level) -> Self {
            Self { level }
        }
    }

    impl Default for LogEmitter {
        fn default() -> Self {
            Self { level: Level::Info }
        }
    }

    impl EventEmitter for LogEmitter {
        fn emit(&mut self, event: &ThreadEvent) {
            if log::log_enabled!(self.level) {
                match json::to_string(event) {
                    Ok(serialized) => log::log!(self.level, "{}", serialized),
                    Err(err) => log::log!(
                        self.level,
                        "failed to serialize vtcode exec event for logging: {err}"
                    ),
                }
            }
        }
    }

    pub(crate) use LogEmitter as PublicLogEmitter;
}

#[cfg(feature = "telemetry-log")]
pub use log_support::PublicLogEmitter as LogEmitter;

#[cfg(feature = "telemetry-tracing")]
mod tracing_support {
    use tracing::Level;

    use super::{EVENT_SCHEMA_VERSION, EventEmitter, ThreadEvent, VersionedThreadEvent};

    /// Emits structured events as `tracing` events at the specified level.
    #[derive(Debug, Clone)]
    pub struct TracingEmitter {
        level: Level,
    }

    impl TracingEmitter {
        /// Creates a new [`TracingEmitter`] with the provided [`Level`].
        pub fn new(level: Level) -> Self {
            Self { level }
        }
    }

    impl Default for TracingEmitter {
        fn default() -> Self {
            Self { level: Level::INFO }
        }
    }

    impl EventEmitter for TracingEmitter {
        fn emit(&mut self, event: &ThreadEvent) {
            match self.level {
                Level::TRACE => tracing::event!(
                    target: "vtcode_exec_events",
                    Level::TRACE,
                    schema_version = EVENT_SCHEMA_VERSION,
                    event = ?VersionedThreadEvent::new(event.clone()),
                    "vtcode_exec_event"
                ),
                Level::DEBUG => tracing::event!(
                    target: "vtcode_exec_events",
                    Level::DEBUG,
                    schema_version = EVENT_SCHEMA_VERSION,
                    event = ?VersionedThreadEvent::new(event.clone()),
                    "vtcode_exec_event"
                ),
                Level::INFO => tracing::event!(
                    target: "vtcode_exec_events",
                    Level::INFO,
                    schema_version = EVENT_SCHEMA_VERSION,
                    event = ?VersionedThreadEvent::new(event.clone()),
                    "vtcode_exec_event"
                ),
                Level::WARN => tracing::event!(
                    target: "vtcode_exec_events",
                    Level::WARN,
                    schema_version = EVENT_SCHEMA_VERSION,
                    event = ?VersionedThreadEvent::new(event.clone()),
                    "vtcode_exec_event"
                ),
                Level::ERROR => tracing::event!(
                    target: "vtcode_exec_events",
                    Level::ERROR,
                    schema_version = EVENT_SCHEMA_VERSION,
                    event = ?VersionedThreadEvent::new(event.clone()),
                    "vtcode_exec_event"
                ),
            }
        }
    }

    pub(crate) use TracingEmitter as PublicTracingEmitter;
}

#[cfg(feature = "telemetry-tracing")]
pub use tracing_support::PublicTracingEmitter as TracingEmitter;

#[cfg(feature = "schema-export")]
pub mod schema {
    use schemars::{schema::RootSchema, schema_for};

    use super::{ThreadEvent, VersionedThreadEvent};

    /// Generates a JSON Schema describing [`ThreadEvent`].
    pub fn thread_event_schema() -> RootSchema {
        schema_for!(ThreadEvent)
    }

    /// Generates a JSON Schema describing [`VersionedThreadEvent`].
    pub fn versioned_thread_event_schema() -> RootSchema {
        schema_for!(VersionedThreadEvent)
    }
}

/// Structured events emitted during autonomous execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ThreadEvent {
    /// Indicates that a new execution thread has started.
    #[serde(rename = "thread.started")]
    ThreadStarted(ThreadStartedEvent),
    /// Marks the beginning of an execution turn.
    #[serde(rename = "turn.started")]
    TurnStarted(TurnStartedEvent),
    /// Marks the completion of an execution turn.
    #[serde(rename = "turn.completed")]
    TurnCompleted(TurnCompletedEvent),
    /// Marks a turn as failed with additional context.
    #[serde(rename = "turn.failed")]
    TurnFailed(TurnFailedEvent),
    /// Indicates that an item has started processing.
    #[serde(rename = "item.started")]
    ItemStarted(ItemStartedEvent),
    /// Indicates that an item has been updated.
    #[serde(rename = "item.updated")]
    ItemUpdated(ItemUpdatedEvent),
    /// Indicates that an item reached a terminal state.
    #[serde(rename = "item.completed")]
    ItemCompleted(ItemCompletedEvent),
    /// Represents a fatal error.
    #[serde(rename = "error")]
    Error(ThreadErrorEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadStartedEvent {
    /// Unique identifier for the thread that was started.
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TurnStartedEvent {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnCompletedEvent {
    /// Token usage summary for the completed turn.
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnFailedEvent {
    /// Human-readable explanation describing why the turn failed.
    pub message: String,
    /// Optional token usage that was consumed before the failure occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadErrorEvent {
    /// Fatal error message associated with the thread.
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Usage {
    /// Number of prompt tokens processed during the turn.
    pub input_tokens: u64,
    /// Number of cached prompt tokens reused from previous turns.
    pub cached_input_tokens: u64,
    /// Number of completion tokens generated by the model.
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemCompletedEvent {
    /// Snapshot of the thread item that completed.
    pub item: ThreadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemStartedEvent {
    /// Snapshot of the thread item that began processing.
    pub item: ThreadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemUpdatedEvent {
    /// Snapshot of the thread item after it was updated.
    pub item: ThreadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadItem {
    /// Stable identifier associated with the item.
    pub id: String,
    /// Embedded event details for the item type.
    #[serde(flatten)]
    pub details: ThreadItemDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadItemDetails {
    /// Message authored by the agent.
    AgentMessage(AgentMessageItem),
    /// Free-form reasoning text produced during a turn.
    Reasoning(ReasoningItem),
    /// Command execution lifecycle update.
    CommandExecution(CommandExecutionItem),
    /// File change summary associated with the turn.
    FileChange(FileChangeItem),
    /// MCP tool invocation status.
    McpToolCall(McpToolCallItem),
    /// Web search event emitted by a registered search provider.
    WebSearch(WebSearchItem),
    /// General error captured for auditing.
    Error(ErrorItem),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentMessageItem {
    /// Textual content of the agent message.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningItem {
    /// Free-form reasoning content captured during planning.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandExecutionStatus {
    /// Command finished successfully.
    #[default]
    Completed,
    /// Command failed (non-zero exit code or runtime error).
    Failed,
    /// Command is still running and may emit additional output.
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandExecutionItem {
    /// Command string executed by the runner.
    pub command: String,
    /// Aggregated output emitted by the command.
    #[serde(default)]
    pub aggregated_output: String,
    /// Exit code reported by the process, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Current status of the command execution.
    pub status: CommandExecutionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChangeItem {
    /// List of individual file updates included in the change set.
    pub changes: Vec<FileUpdateChange>,
    /// Whether the patch application succeeded.
    pub status: PatchApplyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileUpdateChange {
    /// Path of the file that was updated.
    pub path: String,
    /// Type of change applied to the file.
    pub kind: PatchChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatchApplyStatus {
    /// Patch successfully applied.
    Completed,
    /// Patch application failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatchChangeKind {
    /// File addition.
    Add,
    /// File deletion.
    Delete,
    /// File update in place.
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpToolCallItem {
    /// Name of the MCP tool invoked by the agent.
    pub tool_name: String,
    /// Arguments passed to the tool invocation, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// Result payload returned by the tool, if captured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Lifecycle status for the tool call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<McpToolCallStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpToolCallStatus {
    /// Tool invocation has started.
    Started,
    /// Tool invocation completed successfully.
    Completed,
    /// Tool invocation failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchItem {
    /// Query that triggered the search.
    pub query: String,
    /// Search provider identifier, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional raw search results captured for auditing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorItem {
    /// Error message displayed to the user or logs.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_event_round_trip() {
        let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
            usage: Usage {
                input_tokens: 1,
                cached_input_tokens: 2,
                output_tokens: 3,
            },
        });

        let json = serde_json::to_string(&event).expect("serialize");
        let restored: ThreadEvent = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored, event);
    }

    #[test]
    fn versioned_event_wraps_schema_version() {
        let event = ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: "abc".to_string(),
        });

        let versioned = VersionedThreadEvent::new(event.clone());

        assert_eq!(versioned.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(versioned.event, event);
        assert_eq!(versioned.into_event(), event);
    }

    #[cfg(feature = "serde-json")]
    #[test]
    fn versioned_json_round_trip() {
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "item-1".to_string(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: "hello".to_string(),
                }),
            },
        });

        let payload = crate::json::versioned_to_string(&event).expect("serialize");
        let restored = crate::json::versioned_from_str(&payload).expect("deserialize");

        assert_eq!(restored.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(restored.event, event);
    }
}
