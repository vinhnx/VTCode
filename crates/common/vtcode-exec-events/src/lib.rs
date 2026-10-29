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
pub mod diagnostic;
pub use diagnostic::{Diagnostic, DiagnosticAction, DiagnosticSeverity};
pub mod items;
pub use items::{
    AgentMessageItem, AppPreviewItem, AppUiPosture, CommandExecutionItem, CommandExecutionStatus, DecisionItem,
    FileChangeItem, FileUpdateChange, McpToolCallItem, McpToolCallStatus, PatchApplyStatus, PatchChangeKind, PlanItem,
    ReasoningItem, ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem, ToolOutcome, ToolOutputItem,
    WebSearchItem, tool_outcome_from_status,
};
pub mod matrix;
pub mod planning;
pub mod trace;

/// Semantic version of the serialized event schema exported by this crate.
pub const EVENT_SCHEMA_VERSION: &str = "0.22.0";

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

    /// Returns the nested [`ThreadEvent`] by reference.
    pub fn event(&self) -> &ThreadEvent {
        &self.event
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

#[cfg(feature = "serde-json")]
pub(crate) mod json;

#[cfg(feature = "telemetry-log")]
mod log_support;

#[cfg(feature = "telemetry-log")]
pub use log_support::PublicLogEmitter as LogEmitter;

#[cfg(feature = "telemetry-tracing")]
mod tracing_support;

#[cfg(feature = "telemetry-tracing")]
pub use tracing_support::PublicTracingEmitter as TracingEmitter;

#[cfg(feature = "telemetry-otel")]
mod otel_support;

#[cfg(feature = "telemetry-otel")]
pub use otel_support::PublicOtelEmitter as OtelEmitter;

#[cfg(feature = "schema-export")]
pub mod schema;

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
    /// Emitted when an MCP server offers app UI that the `off` apps tier
    /// refuses. No `ui://` resource was fetched and no app rows render; the
    /// refused URIs are listed so the denial stays visible.
    #[serde(rename = "app.sandbox_denied")]
    AppSandboxDenied(Box<AppSandboxDeniedEvent>),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct AppSandboxDeniedEvent {
    /// Tool whose result offered the refused app UI.
    pub tool: String,
    /// Apps tier that refused the UI (`off`).
    pub tier: String,
    /// Refused `ui://` URIs, in first-seen order (capped at 4).
    pub uris: Vec<String>,
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
    /// Runtime-owned recovery checkpoint; absent in older sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Box<planning::PlanningRecoverySnapshot>>,
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
    /// Identifier of the tool invocation that requested permission, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct PermissionResolvedEvent {
    /// Name of the tool that was permitted or denied.
    pub tool_name: String,
    /// Identifier of the tool invocation whose permission was resolved, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
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
