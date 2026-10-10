#![allow(
    missing_docs,
    dead_code,
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
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
pub mod matrix;
pub mod trace;

/// Semantic version of the serialized event schema exported by this crate.
pub const EVENT_SCHEMA_VERSION: &str = "0.18.0";

/// Wraps a [`ThreadEvent`] with schema metadata so downstream consumers can
/// negotiate compatibility before processing an event stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct VersionedThreadEvent {
    /// Semantic version describing the schema of the nested event payload.
    schema_version: String,
    /// Concrete event emitted by the agent runtime.
    event: ThreadEvent,
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
pub(crate) mod json {
    use super::{ThreadEvent, VersionedThreadEvent};

    /// Converts an event into a `serde_json::Value`.
    pub fn to_value(event: &ThreadEvent) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(event)
    }

    /// Serializes an event into a JSON string.
    pub(crate) fn to_string(event: &ThreadEvent) -> serde_json::Result<String> {
        serde_json::to_string(event)
    }

    /// Deserializes an event from a JSON string.
    pub fn from_str(payload: &str) -> serde_json::Result<ThreadEvent> {
        serde_json::from_str(payload)
    }

    /// Serializes a [`VersionedThreadEvent`] wrapper.
    pub(crate) fn versioned_to_string(event: &ThreadEvent) -> serde_json::Result<String> {
        serde_json::to_string(&VersionedThreadEvent::new(event.clone()))
    }

    /// Deserializes a [`VersionedThreadEvent`] wrapper.
    pub(crate) fn versioned_from_str(payload: &str) -> serde_json::Result<VersionedThreadEvent> {
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
                    Err(err) => log::log!(self.level, "failed to serialize vtcode exec event for logging: {err}"),
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
    /// ```rust,ignore
    /// // Requires concrete SDK type (e.g. opentelemetry_sdk::trace::SdkTracerProvider)
    /// # use vtcode_exec_events::OtelEmitter;
    /// # let tracer = opentelemetry_sdk::trace::SdkTracerProvider::default()
    /// #     .tracer("vtcode");
    /// # let mut emitter = OtelEmitter::new(tracer);
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
                ThreadEvent::ContextReset(_) => "context.reset",
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
                        span.set_attribute(KeyValue::new("total_cost_usd", cost.as_f64().unwrap_or(0.0)));
                    }
                    span.set_attribute(KeyValue::new(
                        "input_tokens",
                        i64::try_from(e.usage.input_tokens).unwrap_or(i64::MAX),
                    ));
                    span.set_attribute(KeyValue::new(
                        "output_tokens",
                        i64::try_from(e.usage.output_tokens).unwrap_or(i64::MAX),
                    ));
                    span.set_attribute(KeyValue::new("completion_subtype", e.subtype.as_str().to_string()));
                }
                ThreadEvent::ContextReset(e) => {
                    span.set_attribute(KeyValue::new("thread_id", e.thread_id.clone()));
                    span.set_attribute(KeyValue::new("turn_id", e.turn_id.clone()));
                    span.set_attribute(KeyValue::new("plan_preserved", e.plan_preserved));
                    span.set_attribute(KeyValue::new(
                        "previous_context_usage_percent",
                        e.previous_context_usage_percent as i64,
                    ));
                    span.set_attribute(KeyValue::new("tool_budget_reset", e.tool_budget_reset));
                }
                ThreadEvent::TurnCompleted(e) => {
                    span.set_attribute(KeyValue::new(
                        "turn_input_tokens",
                        i64::try_from(e.usage.input_tokens).unwrap_or(i64::MAX),
                    ));
                    span.set_attribute(KeyValue::new(
                        "turn_output_tokens",
                        i64::try_from(e.usage.output_tokens).unwrap_or(i64::MAX),
                    ));
                }
                ThreadEvent::ItemCompleted(e) => {
                    if let ThreadItemDetails::Harness(harness) = &e.item.details {
                        span.set_attribute(KeyValue::new("harness_event", format!("{:?}", harness.event)));
                        if let Some(ref msg) = harness.message {
                            span.set_attribute(KeyValue::new("harness_message", msg.clone()));
                        }
                        if let Some(ref path) = harness.path {
                            span.set_attribute(KeyValue::new("harness_path", path.clone()));
                        }
                        if let Some(dur) = harness.duration_ms {
                            span.set_attribute(KeyValue::new("duration_ms", i64::try_from(dur).unwrap_or(i64::MAX)));
                        }
                        let mut event_attrs = vec![KeyValue::new("event_kind", format!("{:?}", harness.event))];
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
    /// Replayable local matrix lifecycle checkpoint.
    #[serde(rename = "matrix.updated")]
    MatrixUpdated(Box<matrix::MatrixSnapshot>),
    /// Indicates that a new execution thread has started.
    #[serde(rename = "thread.started")]
    ThreadStarted(ThreadStartedEvent),
    /// Indicates that an execution thread has reached a terminal outcome.
    #[serde(rename = "thread.completed")]
    ThreadCompleted(Box<ThreadCompletedEvent>),
    /// Indicates that conversation compaction replaced older history with a boundary.
    #[serde(rename = "thread.compact_boundary")]
    ThreadCompactBoundary(Box<ThreadCompactBoundaryEvent>),
    /// Indicates that the approved plan handoff rebuilt a fresh execution context.
    #[serde(rename = "context.reset")]
    ContextReset(ContextResetEvent),
    /// Marks the beginning of an execution turn.
    #[serde(rename = "turn.started")]
    TurnStarted(TurnStartedEvent),
    /// Marks the completion of an execution turn.
    #[serde(rename = "turn.completed")]
    TurnCompleted(TurnCompletedEvent),
    /// Marks a turn as failed with additional context.
    #[serde(rename = "turn.failed")]
    TurnFailed(TurnFailedEvent),
    /// Marks a turn as blocked before success could be confirmed. Emitted
    /// alongside `turn.failed` so UI subscribers get a first-class signal
    /// with the fuse counters and last tool instead of inferring it.
    #[serde(rename = "turn.blocked")]
    TurnBlocked(Box<TurnBlockedEvent>),
    /// Indicates that an item has started processing.
    #[serde(rename = "item.started")]
    ItemStarted(ItemStartedEvent),
    /// Indicates that an item has been updated.
    #[serde(rename = "item.updated")]
    ItemUpdated(ItemUpdatedEvent),
    /// Indicates that an item reached a terminal state.
    #[serde(rename = "item.completed")]
    ItemCompleted(ItemCompletedEvent),
    /// Emitted when a tool requires user permission before execution.
    #[serde(rename = "permission.requested")]
    PermissionRequested(PermissionRequestedEvent),
    /// Emitted when the user resolves a permission prompt.
    #[serde(rename = "permission.resolved")]
    PermissionResolved(PermissionResolvedEvent),
    /// A mid-turn user interjection was merged into the running turn.
    #[serde(rename = "interjected")]
    Interjected(InterjectedEvent),
    /// Streaming delta for a plan item in Planning workflow.
    #[serde(rename = "plan.delta")]
    PlanDelta(Box<PlanDeltaEvent>),
    /// Indicates that a completed plan is waiting for an implementation decision.
    #[serde(rename = "plan.approval.requested")]
    PlanApprovalRequested(PlanApprovalRequestedEvent),
    /// Records the user's or policy's decision about a completed plan.
    #[serde(rename = "plan.approval.resolved")]
    PlanApprovalResolved(PlanApprovalResolvedEvent),
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
    /// Runtime completion timestamp, absent from legacy events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
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
    /// Segment identifier that contained the request prefix before compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_segment_id: Option<String>,
    /// Segment identifier created after compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_segment_id: Option<String>,
    /// Hash of the immutable request prefix before compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_prefix_hash: Option<String>,
    /// Hash of the immutable request prefix after compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_prefix_hash: Option<String>,
    /// Hash of the ordered tool catalog before compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_catalog_hash: Option<String>,
    /// Hash of the ordered tool catalog after compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_catalog_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ContextResetTrigger {
    /// The user selected the fresh-context plan approval path.
    PlanApproval,
    /// Catch-all for triggers introduced by newer schema versions.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ContextResetEvent {
    /// Stable thread identifier for the session.
    pub thread_id: String,
    /// Identifier of the turn that approved the plan.
    pub turn_id: String,
    /// What initiated the context reset.
    pub trigger: ContextResetTrigger,
    /// Whether the approved plan and task tracker survived the reset.
    pub plan_preserved: bool,
    /// Context pressure reported before the reset, expressed as a percentage.
    pub previous_context_usage_percent: u8,
    /// Whether the per-turn and per-session tool budgets were reset.
    pub tool_budget_reset: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TurnStartedEvent {
    /// Optional decomposition of the assembled first-request prefix so
    /// downstream consumers can attribute token overhead without inventing
    /// parallel event types.
    #[serde(skip_serializing_if = "Option::is_none")]
    token_breakdown: Option<Box<TokenBreakdown>>,
    /// Task identity and public input recorded at the request boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Box<ExecutionContext>>,
}

impl TurnStartedEvent {
    /// Recorded prefix token breakdown, absent when the producer did not capture it.
    pub fn token_breakdown(&self) -> Option<&TokenBreakdown> {
        self.token_breakdown.as_deref()
    }
}

/// Origin of a turn's input; internal turns retain their parent task.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InputOrigin {
    User,
    Correction,
    PlanApproval,
    Continuation,
    Retry,
}

/// Optional recorded execution ancestry. Absent fields are historical gaps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ExecutionContext {
    pub task_id: String,
    pub turn_id: String,
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_actor_id: Option<String>,
    pub origin: InputOrigin,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
}

/// Recorded shell classification, supplied by the runtime's shared classifier.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CommandActivity {
    Inspection,
    Verification,
    Mutation,
}

/// Item identity, timing, and command semantics for explanation consumers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ItemContext {
    pub task_id: String,
    pub turn_id: String,
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_actor_id: Option<String>,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<CommandActivity>,
}

/// Per-request token-budget breakdown for the assembled first-request prefix.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TokenBreakdown {
    /// System prompt text tokens.
    system_prompt_tokens: u64,
    /// On-wire tool schema tokens.
    tool_schema_tokens: u64,
    /// Instruction file tokens included in the prompt.
    instruction_file_tokens: u64,
    /// Message history text tokens.
    message_history_tokens: u64,
    /// Cache read tokens (served from prior turns).
    cache_read_tokens: u64,
    /// Cache write tokens (new cache entries created this turn).
    cache_write_tokens: u64,
    /// Tokens that missed cache (neither read nor written).
    cache_miss_tokens: u64,
    /// Subagent bootstrap tokens, if this turn spawned a child agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    subagent_bootstrap_tokens: Option<u64>,
}

/// Bound on exec session ids recorded in one turn's `turn.completed` event.
/// Mirrors `SnapshotTurnDiagnostics::in_progress_exec_sessions` (cap 4,
/// newest first) so `ThreadEvent` and checkpoint diagnostics cannot drift.
pub const MAX_IN_PROGRESS_EXEC_SESSIONS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TurnCompletedEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<Box<String>>,
    /// Token usage summary for the completed turn.
    pub usage: Usage,
    /// Exec sessions still running when the turn ended (bounded, newest
    /// first). Empty when every command settled within the turn. Correlates
    /// with the next turn's transient exec-session resume hint without
    /// requiring session-id reconstruction.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_null_as_default"
    )]
    pub in_progress_exec_sessions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TurnFailedEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<Box<String>>,
    /// Human-readable explanation describing why the turn failed.
    pub message: String,
    /// Optional token usage that was consumed before the failure occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct TurnBlockedEvent {
    /// Runtime terminal timestamp, absent from legacy events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Human-readable explanation describing why the turn was blocked.
    pub message: String,
    /// Display label of the last blocked tool call, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tool: Option<String>,
    /// Consecutive blocked tool calls observed this turn.
    #[serde(default)]
    pub blocked_streak: usize,
    /// Total blocked tool calls observed this turn.
    #[serde(default)]
    pub blocked_total: usize,
    /// Consecutive cap that was enforced.
    #[serde(default)]
    pub consecutive_cap: usize,
    /// Total cap that was enforced.
    #[serde(default)]
    pub total_cap: usize,
    /// Whether the fuse tripped while a tool-free recovery pass was active.
    #[serde(default)]
    pub recovery_active: bool,
    /// Optional token usage that was consumed before the block occurred.
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
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub input_tokens: u64,
    /// Number of cached prompt tokens reused from previous turns.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub cached_input_tokens: u64,
    /// Number of cache-creation tokens charged during the turn.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub cache_creation_tokens: u64,
    /// Number of completion tokens generated by the model.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub output_tokens: u64,
}

/// Serde helper that accepts explicit `null` as `T::default()` for
/// backward-compatible checkpoint/diagnostics payloads. Pair with
/// `#[serde(default, deserialize_with = "deserialize_null_as_default")]` so
/// both missing and `null` fields degrade to the default instead of failing
/// deserialization. Reused by downstream crates (e.g. `vtcode-core`
/// snapshots) so the null-tolerance rule cannot drift between copies.
pub fn deserialize_null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

impl Usage {
    /// Number of input tokens billed at the full input rate: neither served
    /// from cache nor written to it. `input_tokens` is the total prompt token
    /// count (uncached + cached + cache-creation), so both cached and
    /// cache-creation tokens are subtracted out here.
    #[must_use]
    fn uncached_input_tokens(&self) -> u64 {
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
        self.cached_input_tokens = self.cached_input_tokens.saturating_add(other.cached_input_tokens);
        self.cache_creation_tokens = self.cache_creation_tokens.saturating_add(other.cache_creation_tokens);
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
pub struct PlanApprovalRequestedEvent {
    /// Identifier of the thread emitting the approval request.
    pub thread_id: String,
    /// Identifier of the turn that produced the plan.
    pub turn_id: String,
    /// Plan file associated with the approval request, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_file: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlanApprovalDecision {
    /// Execute with normal per-edit approval prompts.
    Execute,
    /// Execute with automatic edit approval enabled.
    AutoAccept,
    /// Execute the plan after rebuilding a fresh context.
    FreshContext,
    /// Keep planning and revise the proposed plan.
    Revise,
    /// Dismiss the approval request without implementing.
    Cancel,
    /// Hand the plan to the build primary agent.
    SwitchBuild,
    /// Hand the plan to the auto primary agent.
    SwitchAuto,
    /// Catch-all for decisions added in newer schema versions.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct PlanApprovalResolvedEvent {
    /// Identifier of the thread emitting the approval decision.
    pub thread_id: String,
    /// Identifier of the turn in which the decision was made.
    pub turn_id: String,
    /// Decision selected by the user or active execution policy.
    pub decision: PlanApprovalDecision,
    /// Whether the decision came from policy rather than an interactive user action.
    pub automatic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ThreadItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Box<ItemContext>>,
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
    Reasoning(Box<ReasoningItem>),
    /// Public rationale explicitly recorded by the agent; never private reasoning.
    Decision(Box<DecisionItem>),
    /// Command execution lifecycle update for an actual shell/PTY process.
    CommandExecution(Box<CommandExecutionItem>),
    /// Tool invocation lifecycle update.
    ToolInvocation(Box<ToolInvocationItem>),
    /// Tool output lifecycle update tied to a tool invocation.
    ToolOutput(Box<ToolOutputItem>),
    /// File change summary associated with the turn.
    FileChange(Box<FileChangeItem>),
    /// MCP tool invocation status.
    McpToolCall(Box<McpToolCallItem>),
    /// Web search event emitted by a registered search provider.
    WebSearch(Box<WebSearchItem>),
    /// Harness-managed continuation or verification lifecycle event.
    Harness(Box<HarnessEventItem>),
    /// General error captured for auditing.
    Error(ErrorItem),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct AgentMessageItem {
    /// Textual content of the agent message.
    pub text: String,
}

/// A consequential choice and the agent's reported public rationale.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct DecisionItem {
    pub summary: String,
    pub rationale: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
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
    /// Optional stage of reasoning (e.g., "analysis", "plan", "verification",
    /// or the bounded evidence-only "diagnosis" stage).
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

/// Fine-grained outcome of a tool invocation lifecycle.
///
/// Mirrors the outcome taxonomy used by the runtime: `status` remains the
/// coarse lifecycle signal (`Completed` / `Failed` / `InProgress`), while
/// `outcome` captures *why* the invocation terminated. Consumers that only
/// need success/failure can continue to read `status`; analytics and the UI
/// layer use `outcome` for richer classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    /// Tool executed and returned a result.
    #[default]
    Success,
    /// Tool executed but returned an error.
    Error,
    /// User rejected the permission prompt.
    PermissionRejected,
    /// User cancelled the permission prompt (e.g. Ctrl+C / Esc).
    PermissionCancelled,
    /// User provided a followup message instead of approving.
    Followup,
    /// A user-configured hook blocked execution.
    HookDenied,
    /// Tool not found or arguments couldn't be parsed.
    InvalidTool,
    /// Tool was cancelled during execution or closed before dispatch when its turn ended.
    Cancelled,
}

impl ToolOutcome {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Followup)
    }
}

/// Map a terminal [`ToolCallStatus`] to its corresponding [`ToolOutcome`].
///
/// # Panics
///
/// Panics if `status` is [`ToolCallStatus::InProgress`], which is a non-terminal
/// state and must never be passed to a completion-event emitter.
#[must_use]
#[allow(
    clippy::unreachable,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub fn tool_outcome_from_status(status: &ToolCallStatus) -> ToolOutcome {
    match status {
        ToolCallStatus::Completed => ToolOutcome::Success,
        ToolCallStatus::Failed => ToolOutcome::Error,
        ToolCallStatus::InProgress => unreachable!("InProgress status passed to completion event"),
    }
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
    /// Fine-grained outcome of the invocation lifecycle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ToolOutcome>,
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
    /// Captured preview was truncated, suppressed, or unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_incomplete: Option<bool>,
    /// List of individual file updates included in the change set.
    pub changes: Vec<FileUpdateChange>,
    /// Whether the patch application succeeded.
    pub status: PatchApplyStatus,
    /// Optional precomputed unified diff for the change set.
    ///
    /// Populated by the turn diff tracker so consumers can render per-change
    /// previews without recomputation. Absent in older events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_diff: Option<String>,
    /// Optional added-line count for the change set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    /// Optional deleted-line count for the change set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
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
    /// A turn was blocked before success could be confirmed. Carries the fuse
    /// counters so UI layers can render without correlating multiple events.
    TurnBlocked,
    /// A bounded tool-free recovery pass was scheduled after blocked calls.
    BlockedRecoveryStarted,
    /// A bounded tool-free recovery pass finished.
    BlockedRecoveryFinished,
    BlockedHandoffWritten,
    /// The owning session resolved its archived blocked handoff and removed
    /// the live recovery pointer.
    BlockedHandoffResolved,
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
    /// The user granted additional session tool-call capacity and the
    /// pending call will be retried in the same turn.
    SessionToolLimitIncreased,
    /// The user granted additional tool-loop capacity for the current turn.
    ToolLoopLimitIncreased,
    /// A background subprocess or exec session reached a terminal state.
    BackgroundSubprocessCompleted,
    /// A native delegated agent's public status and ancestry were observed.
    DelegatedAgentStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    Cancelled,
    Followup,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct PermissionRequestedEvent {
    /// Name of the tool that requires permission.
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct PermissionResolvedEvent {
    /// Name of the tool that was permitted or denied.
    pub tool_name: String,
    /// User's decision on the permission prompt.
    pub decision: PermissionDecision,
    /// Wall-clock time the prompt was visible, in milliseconds.
    pub wait_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum InterjectionSource {
    Direct,
    Queue,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RedirectKind {
    Interjection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct InterjectedEvent {
    /// Public correction text, when recorded by the runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Box<String>>,
    /// How the interjection reached the running turn.
    pub source: InterjectionSource,
    /// Number of image attachments that accompanied the interjection.
    pub image_count: u32,
    /// Always `Interjection` for this event; carried so the shared
    /// `redirect_kind` field is queryable uniformly across redirect events.
    pub redirect_kind: RedirectKind,
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
    /// Stable task identifier for background completion events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Child session identifier for background completion events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Exec-session identifier for background completion events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_session_id: Option<String>,
    /// Terminal background status, when the event represents a subprocess.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Archived transcript reference for background completion events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    /// Archived session reference for background completion events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ErrorItem {
    /// Error message displayed to the user or logs.
    pub message: String,
}

#[cfg(test)]
mod tests;
