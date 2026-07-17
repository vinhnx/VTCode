#![allow(missing_docs)]
//! Structured execution telemetry events shared across VT Code crates.
//!
//! This crate exposes the serialized schema for thread lifecycle updates,
//! command execution results, and other timeline artifacts emitted by the
//! automation runtime. Downstream applications can deserialize these
//! structures to drive dashboards, logging, or auditing pipelines without
//! depending on the full `vtcode-core` crate.
//!
//! # Agent Trace Support
//!
//! This crate implements the [Agent Trace](https://agent-trace.dev/) specification
//! for tracking AI-generated code attribution. See the [`trace`] module for details.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod atif;
pub mod trace;

/// Semantic version of the serialized event schema exported by this crate.
pub const EVENT_SCHEMA_VERSION: &str = "0.8.0";

/// Wraps a [`ThreadEvent`] with schema metadata so downstream consumers can
/// negotiate compatibility before processing an event stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
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
                    Ok(serialized) => log::log!(self.level, "{serialized}"),
                    Err(err) => log::log!(
                        self.level,
                        "failed to serialize vtcode exec event for logging: {err}"
                    ),
                }
            }
        }
    }

    pub use LogEmitter as PublicLogEmitter;
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

    pub use TracingEmitter as PublicTracingEmitter;
}

#[cfg(feature = "telemetry-tracing")]
pub use tracing_support::PublicTracingEmitter as TracingEmitter;

#[cfg(feature = "telemetry-otel")]
mod otel_support {
    use opentelemetry::KeyValue;
    use opentelemetry::trace::{Span, Status, Tracer};

    use super::{EventEmitter, ThreadEvent, ThreadItemDetails};

    /// Emits [`ThreadEvent`]s as OpenTelemetry spans and span events.
    ///
    /// Each `ThreadEvent` is recorded as an OTel span with attributes derived
    /// from the event payload.  Harness events are attached as span events
    /// with their own attributes (event kind, message, path, etc.).
    ///
    /// # Usage
    ///
    /// ```rust,no_run
    /// use vtcode_exec_events::OtelEmitter;
    /// use opentelemetry::trace::TracerProvider;
    ///
    /// let provider = TracerProvider::default();
    /// let tracer = provider.tracer("vtcode");
    /// let mut emitter = OtelEmitter::new(tracer);
    /// ```
    pub struct OtelEmitter<T: Tracer> {
        tracer: T,
    }

    impl<T: Tracer> OtelEmitter<T> {
        pub fn new(tracer: T) -> Self {
            Self { tracer }
        }
    }

    impl<T: Tracer> EventEmitter for OtelEmitter<T> {
        fn emit(&mut self, event: &ThreadEvent) {
            let span_name = match event {
                ThreadEvent::ThreadStarted(_) => "thread.started",
                ThreadEvent::ThreadCompleted(_) => "thread.completed",
                ThreadEvent::TurnStarted(_) => "turn.started",
                ThreadEvent::TurnCompleted(_) => "turn.completed",
                ThreadEvent::TurnFailed(_) => "turn.failed",
                ThreadEvent::ItemStarted(_) => "item.started",
                ThreadEvent::ItemUpdated(_) => "item.updated",
                ThreadEvent::ItemCompleted(_) => "item.completed",
                ThreadEvent::Error(_) => "error",
                _ => "event",
            };

            let mut span = self.tracer.start(span_name);

            match event {
                ThreadEvent::ThreadStarted(e) => {
                    span.set_attribute(KeyValue::new("thread_id", e.thread_id.clone()));
                }
                ThreadEvent::ThreadCompleted(e) => {
                    if let Some(ref cost) = e.total_cost_usd {
                        span.set_attribute(KeyValue::new(
                            "total_cost_usd",
                            cost.as_f64().unwrap_or(0.0),
                        ));
                    }
                    span.set_attribute(KeyValue::new("input_tokens", e.usage.input_tokens as i64));
                    span.set_attribute(KeyValue::new(
                        "output_tokens",
                        e.usage.output_tokens as i64,
                    ));
                    span.set_attribute(KeyValue::new(
                        "completion_subtype",
                        e.subtype.as_str().to_string(),
                    ));
                }
                ThreadEvent::TurnCompleted(e) => {
                    span.set_attribute(KeyValue::new(
                        "turn_input_tokens",
                        e.usage.input_tokens as i64,
                    ));
                    span.set_attribute(KeyValue::new(
                        "turn_output_tokens",
                        e.usage.output_tokens as i64,
                    ));
                }
                ThreadEvent::ItemCompleted(e) => {
                    if let ThreadItemDetails::Harness(harness) = &e.item.details {
                        span.set_attribute(KeyValue::new(
                            "harness_event",
                            format!("{:?}", harness.event),
                        ));
                        if let Some(ref msg) = harness.message {
                            span.set_attribute(KeyValue::new("harness_message", msg.clone()));
                        }
                        if let Some(ref path) = harness.path {
                            span.set_attribute(KeyValue::new("harness_path", path.clone()));
                        }
                        if let Some(dur) = harness.duration_ms {
                            span.set_attribute(KeyValue::new("duration_ms", dur as i64));
                        }
                        let mut event_attrs =
                            vec![KeyValue::new("event_kind", format!("{:?}", harness.event))];
                        if let Some(ref msg) = harness.message {
                            event_attrs.push(KeyValue::new("message", msg.clone()));
                        }
                        span.add_event("harness_event", event_attrs);
                    }
                }
                ThreadEvent::Error(e) => {
                    span.set_status(Status::Error { description: e.message.clone().into() });
                    span.set_attribute(KeyValue::new("error_message", e.message.clone()));
                }
                _ => {}
            }

            span.end();
        }
    }

    pub use OtelEmitter as PublicOtelEmitter;
}

#[cfg(feature = "telemetry-otel")]
pub use otel_support::PublicOtelEmitter as OtelEmitter;

#[cfg(feature = "schema-export")]
pub mod schema {
    use schemars::{Schema, schema_for};

    use super::{ThreadEvent, VersionedThreadEvent};

    /// Generates a JSON Schema describing [`ThreadEvent`].
    pub fn thread_event_schema() -> Schema {
        schema_for!(ThreadEvent)
    }

    /// Generates a JSON Schema describing [`VersionedThreadEvent`].
    pub fn versioned_thread_event_schema() -> Schema {
        schema_for!(VersionedThreadEvent)
    }
}

/// Structured events emitted during autonomous execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(tag = "type")]
pub enum ThreadEvent {
    /// Indicates that a new execution thread has started.
    #[serde(rename = "thread.started")]
    ThreadStarted(ThreadStartedEvent),
    /// Indicates that an execution thread has reached a terminal outcome.
    #[serde(rename = "thread.completed")]
    ThreadCompleted(ThreadCompletedEvent),
    /// Indicates that conversation compaction replaced older history with a boundary.
    #[serde(rename = "thread.compact_boundary")]
    ThreadCompactBoundary(ThreadCompactBoundaryEvent),
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
    /// Streaming delta for a plan item in Planning workflow.
    #[serde(rename = "plan.delta")]
    PlanDelta(PlanDeltaEvent),
    /// Represents a fatal error.
    #[serde(rename = "error")]
    Error(ThreadErrorEvent),
    /// Catch-all for unknown event types added in newer schema versions.
    /// Preserves forward compatibility when older binaries read newer event streams.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ThreadStartedEvent {
    /// Unique identifier for the thread that was started.
    pub thread_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThreadCompletionSubtype {
    Success,
    ErrorMaxTurns,
    ErrorMaxBudgetUsd,
    ErrorDuringExecution,
    Cancelled,
    /// Catch-all for unknown completion subtypes added in newer schema versions.
    #[serde(other)]
    Unknown,
}

impl ThreadCompletionSubtype {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ErrorMaxTurns => "error_max_turns",
            Self::ErrorMaxBudgetUsd => "error_max_budget_usd",
            Self::ErrorDuringExecution => "error_during_execution",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    Manual,
    Auto,
    Recovery,
    /// Compaction triggered by a mid-session switch of the main model or
    /// provider, so the newly selected model starts from a clean summary.
    ModelSwitch,
    /// Catch-all for unknown triggers added in newer schema versions.
    #[serde(other)]
    Unknown,
}

impl CompactionTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::Recovery => "recovery",
            Self::ModelSwitch => "model_switch",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CompactionMode {
    Provider,
    Local,
    /// Catch-all for unknown modes added in newer schema versions.
    #[serde(other)]
    Unknown,
}

impl CompactionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Local => "local",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ThreadCompletedEvent {
    /// Stable thread identifier for the session.
    pub thread_id: String,
    /// Stable session identifier for the runtime that produced the thread.
    pub session_id: String,
    /// Coarse result category aligned with SDK-style terminal states.
    pub subtype: ThreadCompletionSubtype,
    /// VT Code-specific detailed outcome code.
    pub outcome_code: String,
    /// Final assistant result text when the thread completed successfully.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Provider stop reason or VT Code terminal reason when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Aggregated token usage across the thread.
    pub usage: Usage,
    /// Optional estimated total API cost for the thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<serde_json::Number>,
    /// Number of turns executed before completion.
    pub num_turns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ThreadCompactBoundaryEvent {
    /// Stable thread identifier for the session.
    pub thread_id: String,
    /// Whether compaction was triggered manually or automatically.
    pub trigger: CompactionTrigger,
    /// Whether the compaction boundary came from provider-native or local compaction.
    pub mode: CompactionMode,
    /// Number of messages before compaction.
    pub original_message_count: usize,
    /// Number of messages after compaction.
    pub compacted_message_count: usize,
    /// Optional persisted artifact containing the archived compaction summary/history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_artifact_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TurnStartedEvent {
    /// Optional decomposition of the assembled first-request prefix so
    /// downstream consumers can attribute token overhead without inventing
    /// parallel event types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_breakdown: Option<TokenBreakdown>,
}

/// Per-request token-budget breakdown for the assembled first-request prefix.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TokenBreakdown {
    /// System prompt text tokens.
    pub system_prompt_tokens: u64,
    /// On-wire tool schema tokens.
    pub tool_schema_tokens: u64,
    /// Instruction file tokens included in the prompt.
    pub instruction_file_tokens: u64,
    /// Message history text tokens.
    pub message_history_tokens: u64,
    /// Cache read tokens (served from prior turns).
    pub cache_read_tokens: u64,
    /// Cache write tokens (new cache entries created this turn).
    pub cache_write_tokens: u64,
    /// Tokens that missed cache (neither read nor written).
    pub cache_miss_tokens: u64,
    /// Subagent bootstrap tokens, if this turn spawned a child agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_bootstrap_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TurnCompletedEvent {
    /// Token usage summary for the completed turn.
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TurnFailedEvent {
    /// Human-readable explanation describing why the turn failed.
    pub message: String,
    /// Optional token usage that was consumed before the failure occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ThreadErrorEvent {
    /// Fatal error message associated with the thread.
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct Usage {
    /// Number of prompt tokens processed during the turn.
    pub input_tokens: u64,
    /// Number of cached prompt tokens reused from previous turns.
    pub cached_input_tokens: u64,
    /// Number of cache-creation tokens charged during the turn.
    pub cache_creation_tokens: u64,
    /// Number of completion tokens generated by the model.
    pub output_tokens: u64,
}

impl Usage {
    /// Number of input tokens billed at the full input rate: neither served
    /// from cache nor written to it. `input_tokens` is the total prompt token
    /// count (uncached + cached + cache-creation), so both cached and
    /// cache-creation tokens are subtracted out here.
    #[must_use]
    pub fn uncached_input_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_sub(self.cached_input_tokens)
            .saturating_sub(self.cache_creation_tokens)
    }

    /// Cache hit rate as a fraction (0.0 to 1.0): cached input over total input.
    /// Returns `None` when no input tokens were recorded.
    #[must_use]
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.input_tokens == 0 {
            return None;
        }
        Some(self.cached_input_tokens as f64 / self.input_tokens as f64)
    }

    /// Human-readable summary of prompt cache efficiency.
    #[must_use]
    pub fn cache_summary(&self) -> String {
        let total_input = self.input_tokens;
        if total_input == 0 {
            return "No input tokens recorded.".to_string();
        }

        let cached = self.cached_input_tokens;
        let creation = self.cache_creation_tokens;
        let uncached = self.uncached_input_tokens();
        let rate = cached as f64 / total_input as f64 * 100.0;
        format!(
            "Cache: {cached} cached / {total_input} total input ({rate:.1}% hit rate), \
             {creation} cache-creation, {uncached} uncached"
        )
    }

    /// Accumulate another usage sample into this one.
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.cached_input_tokens =
            self.cached_input_tokens.saturating_add(other.cached_input_tokens);
        self.cache_creation_tokens =
            self.cache_creation_tokens.saturating_add(other.cache_creation_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ItemCompletedEvent {
    /// Snapshot of the thread item that completed.
    pub item: ThreadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ItemStartedEvent {
    /// Snapshot of the thread item that began processing.
    pub item: ThreadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ItemUpdatedEvent {
    /// Snapshot of the thread item after it was updated.
    pub item: ThreadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct PlanDeltaEvent {
    /// Identifier of the thread emitting this plan delta.
    pub thread_id: String,
    /// Identifier of the current turn.
    pub turn_id: String,
    /// Identifier of the plan item receiving the delta.
    pub item_id: String,
    /// Incremental plan text chunk.
    pub delta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ThreadItem {
    /// Stable identifier associated with the item.
    pub id: String,
    /// Embedded event details for the item type.
    #[serde(flatten)]
    pub details: ThreadItemDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadItemDetails {
    /// Message authored by the agent.
    AgentMessage(AgentMessageItem),
    /// Structured plan content authored by the agent in Planning workflow.
    Plan(PlanItem),
    /// Free-form reasoning text produced during a turn.
    Reasoning(ReasoningItem),
    /// Command execution lifecycle update for an actual shell/PTY process.
    CommandExecution(Box<CommandExecutionItem>),
    /// Tool invocation lifecycle update.
    ToolInvocation(ToolInvocationItem),
    /// Tool output lifecycle update tied to a tool invocation.
    ToolOutput(ToolOutputItem),
    /// File change summary associated with the turn.
    FileChange(Box<FileChangeItem>),
    /// MCP tool invocation status.
    McpToolCall(McpToolCallItem),
    /// Web search event emitted by a registered search provider.
    WebSearch(WebSearchItem),
    /// Harness-managed continuation or verification lifecycle event.
    Harness(HarnessEventItem),
    /// General error captured for auditing.
    Error(ErrorItem),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct AgentMessageItem {
    /// Textual content of the agent message.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct PlanItem {
    /// Plan markdown content.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ReasoningItem {
    /// Free-form reasoning content captured during planning.
    pub text: String,
    /// Optional stage of reasoning (e.g., "analysis", "plan", "verification").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct CommandExecutionItem {
    /// Tool or command identifier executed by the runner.
    pub command: String,
    /// Arguments passed to the tool invocation, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// Aggregated output emitted by the command.
    #[serde(default)]
    pub aggregated_output: String,
    /// Exit code reported by the process, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Current status of the command execution.
    pub status: CommandExecutionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Tool finished successfully.
    #[default]
    Completed,
    /// Tool failed.
    Failed,
    /// Tool is still running and may emit additional output.
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ToolInvocationItem {
    /// Name of the invoked tool.
    pub tool_name: String,
    /// Structured arguments passed to the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// Raw model-emitted tool call identifier, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Current lifecycle status of the invocation.
    pub status: ToolCallStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ToolOutputItem {
    /// Identifier of the related harness invocation item.
    pub call_id: String,
    /// Raw model-emitted tool call identifier, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Canonical spool file path when the full output was written to disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spool_path: Option<String>,
    /// Aggregated output emitted by the tool.
    #[serde(default)]
    pub output: String,
    /// Exit code reported by the tool, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Current lifecycle status of the output item.
    pub status: ToolCallStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct FileChangeItem {
    /// List of individual file updates included in the change set.
    pub changes: Vec<FileUpdateChange>,
    /// Whether the patch application succeeded.
    pub status: PatchApplyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct FileUpdateChange {
    /// Path of the file that was updated.
    pub path: String,
    /// Type of change applied to the file.
    pub kind: PatchChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PatchApplyStatus {
    /// Patch successfully applied.
    Completed,
    /// Patch application failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HarnessEventKind {
    PlanningStarted,
    PlanningCompleted,
    ContinuationStarted,
    ContinuationSkipped,
    BlockedHandoffWritten,
    EvaluationStarted,
    EvaluationPassed,
    EvaluationFailed,
    RevisionStarted,
    EscalationTriggered,
    EscalationBypassed,
    VerificationStarted,
    VerificationPassed,
    VerificationFailed,
    /// Agent recovered from a transient error (e.g. after retry succeeded).
    ErrorRecovered,
    /// A transient tool failure triggered an automatic retry attempt.
    ToolRetryAttempted,
    /// Latency record for a tool execution, emitted on turn completion.
    ToolLatencyRecorded,
    /// A checkpoint snapshot was created for the current turn.
    SnapshotCreated,
    /// A checkpoint snapshot was restored (rewind operation).
    SnapshotRestored,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct HarnessEventItem {
    /// Specific harness event emitted by the runtime.
    pub event: HarnessEventKind,
    /// Optional human-readable message associated with the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Optional verification command associated with the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Optional artifact path associated with the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Optional exit code associated with verification results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Retry/recovery attempt number (1-indexed). Only set for retry-related events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// Canonical error category for retry/recovery events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
    /// Latency in milliseconds for tool-execution latency events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ErrorItem {
    /// Error message displayed to the user or logs.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn thread_event_round_trip() -> Result<(), Box<dyn Error>> {
        let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
            usage: Usage {
                input_tokens: 1,
                cached_input_tokens: 2,
                cache_creation_tokens: 0,
                output_tokens: 3,
            },
        });

        let json = serde_json::to_string(&event)?;
        let restored: ThreadEvent = serde_json::from_str(&json)?;

        assert_eq!(restored, event);
        Ok(())
    }

    #[test]
    fn usage_uncached_input_tokens_saturates() {
        let usage = Usage {
            input_tokens: 1_000,
            cached_input_tokens: 800,
            cache_creation_tokens: 100,
            output_tokens: 50,
        };
        assert_eq!(usage.uncached_input_tokens(), 100);

        let inconsistent = Usage {
            input_tokens: 100,
            cached_input_tokens: 150,
            cache_creation_tokens: 0,
            output_tokens: 0,
        };
        assert_eq!(inconsistent.uncached_input_tokens(), 0);

        let inconsistent_with_creation = Usage {
            input_tokens: 100,
            cached_input_tokens: 80,
            cache_creation_tokens: 50,
            output_tokens: 0,
        };
        assert_eq!(inconsistent_with_creation.uncached_input_tokens(), 0);
    }

    #[test]
    fn usage_cache_hit_rate() {
        assert_eq!(Usage::default().cache_hit_rate(), None);

        let usage = Usage {
            input_tokens: 1_000,
            cached_input_tokens: 750,
            cache_creation_tokens: 0,
            output_tokens: 0,
        };
        let rate = usage.cache_hit_rate().expect("rate");
        assert!((rate - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn usage_cache_summary_formats() {
        assert_eq!(Usage::default().cache_summary(), "No input tokens recorded.");

        let usage = Usage {
            input_tokens: 1_000,
            cached_input_tokens: 800,
            cache_creation_tokens: 100,
            output_tokens: 50,
        };
        assert_eq!(
            usage.cache_summary(),
            "Cache: 800 cached / 1000 total input (80.0% hit rate), 100 cache-creation, 100 uncached"
        );
    }

    #[test]
    fn usage_add_accumulates_all_fields_with_saturation() {
        let mut total = Usage {
            input_tokens: 100,
            cached_input_tokens: 20,
            cache_creation_tokens: 5,
            output_tokens: 10,
        };
        total.add(&Usage {
            input_tokens: 50,
            cached_input_tokens: 10,
            cache_creation_tokens: 2,
            output_tokens: 8,
        });

        assert_eq!(total.input_tokens, 150);
        assert_eq!(total.cached_input_tokens, 30);
        assert_eq!(total.cache_creation_tokens, 7);
        assert_eq!(total.output_tokens, 18);

        let mut saturating = Usage {
            input_tokens: u64::MAX,
            cached_input_tokens: u64::MAX,
            cache_creation_tokens: u64::MAX,
            output_tokens: u64::MAX,
        };
        saturating.add(&Usage {
            input_tokens: 1,
            cached_input_tokens: 1,
            cache_creation_tokens: 1,
            output_tokens: 1,
        });
        assert_eq!(saturating.input_tokens, u64::MAX);
        assert_eq!(saturating.cached_input_tokens, u64::MAX);
        assert_eq!(saturating.cache_creation_tokens, u64::MAX);
        assert_eq!(saturating.output_tokens, u64::MAX);
    }

    #[test]
    fn versioned_event_wraps_schema_version() {
        let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "abc".to_string() });

        let versioned = VersionedThreadEvent::new(event.clone());

        assert_eq!(versioned.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(versioned.event, event);
        assert_eq!(versioned.into_event(), event);
    }

    #[cfg(feature = "serde-json")]
    #[test]
    fn versioned_json_round_trip() -> Result<(), Box<dyn Error>> {
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "item-1".to_string(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: "hello".to_string(),
                }),
            },
        });

        let payload = json::versioned_to_string(&event)?;
        let restored = json::versioned_from_str(&payload)?;

        assert_eq!(restored.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(restored.event, event);
        Ok(())
    }

    #[test]
    fn compaction_trigger_serializes_snake_case_and_round_trips() {
        for trigger in [
            CompactionTrigger::Manual,
            CompactionTrigger::Auto,
            CompactionTrigger::Recovery,
            CompactionTrigger::ModelSwitch,
            CompactionTrigger::Unknown,
        ] {
            let json = serde_json::to_string(&trigger).unwrap();
            assert_eq!(json, format!("\"{}\"", trigger.as_str()));
            let restored: CompactionTrigger = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, trigger);
        }
    }

    #[test]
    fn tool_invocation_round_trip() -> Result<(), Box<dyn Error>> {
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "tool_1".to_string(),
                details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                    tool_name: "read_file".to_string(),
                    arguments: Some(serde_json::json!({ "path": "README.md" })),
                    tool_call_id: Some("tool_call_0".to_string()),
                    status: ToolCallStatus::Completed,
                }),
            },
        });

        let json = serde_json::to_string(&event)?;
        let restored: ThreadEvent = serde_json::from_str(&json)?;

        assert_eq!(restored, event);
        Ok(())
    }

    #[test]
    fn tool_output_round_trip_preserves_raw_tool_call_id() -> Result<(), Box<dyn Error>> {
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "tool_1:output".to_string(),
                details: ThreadItemDetails::ToolOutput(ToolOutputItem {
                    call_id: "tool_1".to_string(),
                    tool_call_id: Some("tool_call_0".to_string()),
                    spool_path: None,
                    output: "done".to_string(),
                    exit_code: Some(0),
                    status: ToolCallStatus::Completed,
                }),
            },
        });

        let json = serde_json::to_string(&event)?;
        let restored: ThreadEvent = serde_json::from_str(&json)?;

        assert_eq!(restored, event);
        Ok(())
    }

    #[test]
    fn harness_item_round_trip() -> Result<(), Box<dyn Error>> {
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "harness_1".to_string(),
                details: ThreadItemDetails::Harness(HarnessEventItem {
                    event: HarnessEventKind::VerificationFailed,
                    message: Some("cargo check failed".to_string()),
                    command: Some("cargo check".to_string()),
                    path: None,
                    exit_code: Some(101),
                    attempt: None,
                    error_category: None,
                    duration_ms: None,
                }),
            },
        });

        let json = serde_json::to_string(&event)?;
        let restored: ThreadEvent = serde_json::from_str(&json)?;

        assert_eq!(restored, event);
        Ok(())
    }

    #[test]
    fn thread_completed_round_trip() -> Result<(), Box<dyn Error>> {
        let event = ThreadEvent::ThreadCompleted(ThreadCompletedEvent {
            thread_id: "thread-1".to_string(),
            session_id: "session-1".to_string(),
            subtype: ThreadCompletionSubtype::ErrorMaxBudgetUsd,
            outcome_code: "budget_limit_reached".to_string(),
            result: None,
            stop_reason: Some("max_tokens".to_string()),
            usage: Usage {
                input_tokens: 10,
                cached_input_tokens: 4,
                cache_creation_tokens: 2,
                output_tokens: 5,
            },
            total_cost_usd: serde_json::Number::from_f64(1.25),
            num_turns: 3,
        });

        let json = serde_json::to_string(&event)?;
        let restored: ThreadEvent = serde_json::from_str(&json)?;

        assert_eq!(restored, event);
        Ok(())
    }

    #[test]
    fn compact_boundary_round_trip() -> Result<(), Box<dyn Error>> {
        let event = ThreadEvent::ThreadCompactBoundary(ThreadCompactBoundaryEvent {
            thread_id: "thread-1".to_string(),
            trigger: CompactionTrigger::Recovery,
            mode: CompactionMode::Provider,
            original_message_count: 12,
            compacted_message_count: 5,
            history_artifact_path: Some("/tmp/history.jsonl".to_string()),
        });

        let json = serde_json::to_string(&event)?;
        let restored: ThreadEvent = serde_json::from_str(&json)?;

        assert_eq!(restored, event);
        Ok(())
    }
}
