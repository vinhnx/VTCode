//! Session lifecycle, history paging, and idle-session option DTOs.

use serde::{Deserialize, Serialize};
use vtcode_commons::history_scope::HistoryScope;

/// Session execution and command confirmation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    /// Preserve interactive confirmation for protected commands.
    Ask,
    /// Use the CLI's skip-confirmations behavior for commands.
    AutoApproveCommands,
    /// Run without the process sandbox or interactive approval prompts.
    FullAccess,
}

/// Runtime options exposed without credentials or complete configuration documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOptionsSnapshot {
    /// Effective command confirmation policy.
    pub permission_mode: PermissionMode,
    /// Selected primary agent, such as build or plan.
    pub mode: String,
    /// Primary agents discovered by the CLI for this workspace.
    #[serde(default)]
    pub available_modes: Vec<AgentModeInfo>,
    /// CLI reasoning effort identifier.
    pub reasoning_effort: String,
    /// Efforts supported by this exact provider/model route.
    pub supported_efforts: Vec<String>,
    /// Effective provider speed badge, when supported by this route.
    pub speed: Option<String>,
    /// Whether this exact provider/model route supports the CLI's fast setting.
    #[serde(default)]
    pub fast_mode_supported: bool,
    /// Whether fast processing is currently enabled for this route.
    #[serde(default)]
    pub fast_mode_enabled: bool,
    /// Label used by the CLI for the selected fast tier on this route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_mode_label: Option<String>,
}

/// Non-secret metadata for a selectable CLI primary agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModeInfo {
    /// Canonical CLI primary-agent name.
    pub name: String,
    /// CLI display name.
    pub display_name: String,
    /// Short description of the agent's role.
    pub description: String,
}

/// An explicit idle-session option change. Omitted values preserve existing choices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionOptionsSelectParams {
    /// Change command confirmations, retaining sandbox and tool authorization.
    pub permission_mode: Option<PermissionMode>,
    /// Select a built-in build or plan agent.
    pub mode: Option<String>,
    /// Select a capability-validated reasoning effort.
    pub reasoning_effort: Option<String>,
    /// Select the CLI's fast processing tier for a compatible route.
    pub fast_mode: Option<bool>,
    /// One-shot attestation that the user completed the Full access
    /// confirmation dialog (risk disclosure + explicit acknowledge) for this
    /// request. Required when `permission_mode` is `FullAccess`; never
    /// persisted, never merged into stored session options.
    pub full_access_confirmed: Option<bool>,
}

/// Persist a built-in CLI theme selected in Desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemePreferenceSelectParams {
    /// Built-in VT Code theme identifier.
    pub theme: String,
}

/// A persisted session available for reload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// Stable durable session identifier.
    pub session_id: String,
    /// Human-readable title when known.
    pub title: Option<String>,
    /// Session workspace path.
    pub workspace: String,
    /// Whether the last observed state was terminal.
    pub completed: bool,
    /// Whether this session can be continued using existing archive data.
    pub resumable: bool,
    /// Archive size in bytes, zero when unknown.
    #[serde(default)]
    pub size_bytes: u64,
    /// Snapshot end time as unix seconds, absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

/// Text restored from the durable message snapshot for a loaded session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryMessage {
    /// Model-only context retained for archive indexing but hidden from the transcript.
    #[serde(default)]
    pub hidden: bool,
    /// Stable zero-based index in the archived session snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_index: Option<usize>,
    /// Message role such as `user`, `assistant`, or `tool`.
    pub role: String,
    /// Recorded assistant phase, absent for older archives or non-assistant messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Canonical event item identity, when retained in the archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_item_id: Option<String>,
    /// Text projection of the persisted message content.
    pub text: String,
}

/// Loaded durable session metadata and its bounded transcript projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadResult {
    /// Selected session metadata.
    pub session: SessionSummary,
    /// Recent messages restored from the archive snapshot.
    pub messages: Vec<SessionHistoryMessage>,
    /// Index of the earliest restored message in the archive snapshot.
    #[serde(default)]
    pub history_start_index: usize,
    /// Whether earlier archived messages can be requested.
    #[serde(default)]
    pub history_has_earlier: bool,
    /// Recent file-change snapshots from canonical history, in persisted order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_items: Vec<vtcode_exec_events::ThreadItem>,
    /// Whether bounded history projection omitted review evidence.
    #[serde(default)]
    pub review_history_incomplete: bool,
    /// Recent canonical tool, command, and supporting activity snapshots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity_items: Vec<vtcode_exec_events::ThreadItem>,
    /// Bounded cursors for archived tool output that can be loaded on demand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity_output_cursors: Vec<SessionOutputCursor>,
    /// Whether bounded history projection omitted activity or output evidence.
    #[serde(default)]
    pub activity_history_incomplete: bool,
    /// Ordered canonical event suffix for reconstructing transcript turns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline_events: Vec<vtcode_exec_events::VersionedThreadEvent>,
    /// Whether the event suffix omits earlier events.
    #[serde(default)]
    pub timeline_incomplete: bool,
}

/// Request an earlier bounded page of the selected archived conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryParams {
    /// Durable session identifier.
    pub session_id: String,
    /// Request identity echoed by the service to reject stale pages.
    pub request_id: String,
    /// Exclusive end index for the page; use the earliest loaded archive index.
    pub before_index: usize,
    /// Number of messages requested, capped by [`MAX_SESSION_HISTORY_PAGE_MESSAGES`].
    pub limit: usize,
}

/// One bounded earlier page from a durable conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryPage {
    /// Durable session identifier that owns the page.
    pub session_id: String,
    /// Request identity echoed from [`SessionHistoryParams`].
    pub request_id: String,
    /// Index of the earliest message included in this page.
    pub start_index: usize,
    /// Messages in chronological order.
    pub messages: Vec<SessionHistoryMessage>,
    /// Whether the archive contains messages before this page.
    pub has_earlier: bool,
}

/// One genuine user-typed prompt from a workspace session archive.
///
/// Mirrors the CLI history picker's archived entries: harness-generated
/// follow-ups and enrichment wrappers are stripped server-side, so the client
/// can merge these with the live thread without re-cleaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentPrompt {
    /// Cleaned user-typed prompt text.
    pub content: String,
    /// Durable session identifier that owns the prompt.
    pub session_id: String,
}

/// Cursor for reading more of a restored tool-output capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOutputCursor {
    /// Stable canonical activity item identity.
    pub item_id: String,
    /// Byte offset to request next; always a UTF-8 boundary for text output.
    pub next_offset: u64,
    /// Captured spool size observed during session restore.
    pub total_bytes: u64,
}

/// Request one bounded page from a restored tool-output capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOutputReadParams {
    /// Durable session that owns the output item.
    pub session_id: String,
    /// Request identity echoed by the service to reject stale pages.
    pub request_id: String,
    /// Stable canonical activity item identity.
    pub item_id: String,
    /// UTF-8 byte offset supplied by the current cursor.
    pub offset: u64,
}

/// One bounded page from an archived tool-output capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOutputPage {
    /// Durable session that owns the output item.
    pub session_id: String,
    /// Request identity echoed from [`SessionOutputReadParams`].
    pub request_id: String,
    /// Stable canonical activity item identity.
    pub item_id: String,
    /// Requested starting offset.
    pub offset: u64,
    /// Next UTF-8 boundary after the returned text.
    pub next_offset: u64,
    /// Captured spool size observed during this read.
    pub total_bytes: u64,
    /// Text captured in this page.
    pub text: String,
    /// Whether more captured bytes remain.
    pub has_more: bool,
}

/// Parameters for creating an empty session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateParams {
    /// Optional title shown in history.
    pub title: Option<String>,
}

/// Parameters for loading a durable session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionIdParams {
    /// Durable session identifier.
    pub session_id: String,
}

/// Parameters for listing recent sessions. The limit is a soft page size:
/// omitted values keep the historical full-list behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListParams {
    /// Maximum sessions returned, newest first.
    #[serde(default)]
    pub limit: Option<usize>,
}

impl SessionListParams {
    /// Clamp the requested limit into `1..=MAX_SESSION_LIST_LIMIT`.
    #[must_use]
    pub fn effective_limit(&self) -> usize {
        self.limit.unwrap_or(MAX_SESSION_LIST_LIMIT).clamp(1, MAX_SESSION_LIST_LIMIT)
    }
}

/// Maximum sessions returned by one `session/list` request.
pub const MAX_SESSION_LIST_LIMIT: usize = 200;

/// Parameters for listing recent cross-session prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecentPromptsParams {
    /// Which sessions contribute; omitted scopes to the current project.
    #[serde(default = "default_recent_prompts_scope")]
    pub scope: HistoryScope,
}

fn default_recent_prompts_scope() -> HistoryScope {
    HistoryScope::Project
}

/// Maximum message count accepted by one desktop archive-history page request.
pub const MAX_SESSION_HISTORY_PAGE_MESSAGES: usize = 160;

/// Recent-prompts result. Echoes the requested scope so the client can
/// match out-of-order responses when the scope changes mid-fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecentPromptsResult {
    /// Echo of the requested scope.
    pub scope: HistoryScope,
    /// Cleaned prompts, newest first, bounded by [`MAX_RECENT_PROMPTS`].
    pub prompts: Vec<RecentPrompt>,
}

/// Maximum prompts returned by one recent-prompts request.
pub const MAX_RECENT_PROMPTS: usize = 100;

/// Maximum captured output returned by one on-demand desktop page.
pub const MAX_SESSION_OUTPUT_PAGE_BYTES: usize = 32 * 1024;

/// Marker appended while more archived tool output is available.
pub const RESTORED_OUTPUT_TRUNCATION_SUFFIX: &str = "\n[Output truncated in restored history.]";
