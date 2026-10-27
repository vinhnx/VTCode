//! Versioned JSON-RPC messages shared by the VT Code desktop app and service.
//!
//! The protocol deliberately carries the canonical [`VersionedThreadEvent`]
//! instead of defining a second runtime event model.

use std::collections::{HashMap, VecDeque};
use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use vtcode_exec_events::VersionedThreadEvent;

pub mod slash_command;

/// Current protocol version supported by the desktop client and service.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 16 };

/// A turn was rejected before delivery because a startup interaction is pending.
pub const STARTUP_INTERACTION_PENDING_ERROR: i32 = -32001;

/// Largest permitted JSON-RPC line, excluding its line ending.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Capacity used by host-owned event and command queues.
pub const DEFAULT_QUEUE_CAPACITY: usize = 128;

/// A comparable protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Incompatible wire-contract changes increment the major version.
    pub major: u16,
    /// Compatible additive changes increment the minor version.
    pub minor: u16,
}

/// A client's acceptable range for protocol negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolRange {
    /// Lowest accepted version.
    pub minimum: ProtocolVersion,
    /// Highest accepted version.
    pub maximum: ProtocolVersion,
}

impl ProtocolRange {
    /// Select the highest compatible version in this range.
    pub fn negotiate(self, server: ProtocolVersion) -> Result<ProtocolVersion, ProtocolError> {
        if self.minimum.major != self.maximum.major || server.major != self.minimum.major {
            return Err(ProtocolError::IncompatibleVersion { client: self, server });
        }

        if server < self.minimum || server > self.maximum {
            return Err(ProtocolError::IncompatibleVersion { client: self, server });
        }

        Ok(server)
    }
}

/// JSON-RPC request id. `null` is intentionally not accepted for requests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// String request identifier.
    String(String),
    /// Integer request identifier.
    Number(i64),
}

/// A JSON-RPC request received from the desktop client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Request correlation id.
    pub id: RequestId,
    /// Method name.
    pub method: String,
    /// Typed by the method handler.
    #[serde(default)]
    pub params: Value,
}

/// A JSON-RPC notification received from or sent to the desktop client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Method name.
    pub method: String,
    /// Typed by the method handler.
    #[serde(default)]
    pub params: Value,
}

/// A successful JSON-RPC response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Request correlation id.
    pub id: RequestId,
    /// Result payload.
    pub result: Value,
}

/// A failed JSON-RPC response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Request correlation id; omitted for malformed requests without an id.
    pub id: Option<RequestId>,
    /// Error payload.
    pub error: RpcError,
}

/// Standard JSON-RPC error body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// JSON-RPC error code.
    pub code: i32,
    /// Sanitized error message.
    pub message: String,
    /// Optional structured data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Client initialization request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Range of protocol versions supported by the client.
    pub protocol: ProtocolRange,
    /// Client application name.
    pub client_name: String,
    /// Client application version.
    pub client_version: String,
}

/// Successful initialization response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Negotiated protocol version.
    pub protocol: ProtocolVersion,
    /// Canonical selected workspace path.
    pub workspace: String,
    /// Server version.
    pub server_version: String,
    /// CLI slash commands shown by the native composer.
    #[serde(default)]
    pub slash_commands: Vec<SlashCommandInfo>,
}

/// Display metadata for a slash command exposed by the CLI chat input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommandInfo {
    /// Command name without the leading slash.
    pub name: String,
    /// Short description shown in composer completion.
    pub description: String,
}

/// Basic workspace and provider configuration exposed to the GUI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationSnapshot {
    /// Active provider identifier.
    pub provider: String,
    /// Active model identifier.
    pub model: String,
    /// Whether provider authentication requirements passed startup preflight.
    pub authenticated: bool,
    /// Existing CLI command users can run to configure credentials.
    pub credential_setup_command: String,
    /// Human label for the execution policy the service runs under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    /// Context window of the active model, when the catalog knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Non-secret CLI UI preferences available to the native client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_ui: Option<SharedUiPreferences>,
    /// Effective options for the next runtime session, absent on older helpers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_options: Option<SessionOptionsSnapshot>,
}

/// A discoverable source for the native composer's Add menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComposerResourceKind {
    /// A CLI skill with an explicit dollar-name mention.
    Skill,
    /// An installed Agent Plugin.
    Plugin,
}

/// Bounded, non-secret discovery metadata; never executable content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerResource {
    /// Validated resource name.
    pub name: String,
    /// Short plain-text description.
    pub description: String,
    /// Which shared CLI discovery source owns this entry.
    pub kind: ComposerResourceKind,
}

impl ComposerResource {
    /// Resource names are safe single tokens for explicit CLI skill mentions.
    pub fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 128
            && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    }
}

/// Command confirmation policy; this never bypasses sandbox authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    /// Preserve interactive confirmation for protected commands.
    Ask,
    /// Use the CLI's skip-confirmations behavior for commands.
    AutoApproveCommands,
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
}

/// Persist a built-in CLI theme selected in Desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemePreferenceSelectParams {
    /// Built-in VT Code theme identifier.
    pub theme: String,
}

/// Writable configuration layer understood by the CLI config service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigTarget {
    /// User-level configuration.
    User,
    /// Workspace-level configuration.
    Workspace,
    /// Project profile configuration.
    Project,
}

/// Non-secret setting value exposed to the native configuration browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetting {
    /// Stable dotted path accepted by the CLI config writer.
    pub path: String,
    /// Effective CLI value after defaults; null represents an unset documented optional field.
    pub value: Value,
    /// Safe layer label such as `workspace` or `user`; never a filesystem path.
    pub origin: Option<String>,
    /// CLI `/config` display label, when documented.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// CLI schema type used when a setting has no explicit effective value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// Documented default value, rendered as plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Description from the shared CLI configuration reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Documented choices, if the CLI exposes a finite set.
    #[serde(default)]
    pub options: Vec<String>,
    /// Curated CLI `/config` group or `Advanced settings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Purpose of the curated group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_description: Option<String>,
    /// Whether the current CLI catalog considers this a writable setting.
    #[serde(default = "config_setting_editable_default")]
    pub editable: bool,
}

const fn config_setting_editable_default() -> bool {
    true
}

/// Version of one writable config layer, for optimistic write protection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigTargetVersion {
    /// Layer represented by this version.
    pub target: ConfigTarget,
    /// Layer fingerprint, when the selected layer was loaded.
    pub version: Option<String>,
}

/// Bounded, credential-filtered catalog of the CLI's effective and documented settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSettingsSnapshot {
    /// Effective settings and documented schema entries from the shared CLI `/config` catalog.
    pub settings: Vec<ConfigSetting>,
    /// Fingerprints for user, workspace, and project layers.
    pub target_versions: Vec<ConfigTargetVersion>,
    /// Number of credential-bearing branches excluded from the catalog.
    pub omitted_sensitive: usize,
    /// Number of values omitted because they exceeded the UI size bound.
    pub omitted_large: usize,
    /// Number of additional settings omitted because the catalog reached its limit.
    pub omitted_limit: usize,
}

/// Write one setting through the shared, validated CLI configuration service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigSettingsWriteParams {
    /// Layer to update.
    pub target: ConfigTarget,
    /// Dotted setting path from `ConfigSettingsSnapshot`.
    pub path: String,
    /// Replacement value. Its type must match the effective setting or documented schema type.
    pub value: Value,
    /// Version observed when the setting editor opened.
    pub expected_layer_version: Option<String>,
}

/// Result of persisting a CLI configuration value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSettingsWriteResult {
    /// Layer updated by the CLI config service.
    pub target: ConfigTarget,
    /// Setting path that was persisted.
    pub path: String,
    /// Version of the updated layer.
    pub written_layer_version: String,
    /// Whether a higher-precedence layer still controls the effective value.
    pub overridden: bool,
}

/// CLI preferences projected through existing parsers rather than a second config loader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedUiPreferences {
    /// Theme ID resolved by the CLI startup loader.
    pub theme: String,
    /// Desktop command ID to normalized native key combinations; an empty list unbinds it.
    #[serde(default)]
    pub shortcuts: std::collections::BTreeMap<String, Vec<String>>,
    /// Explicit diagnostics for skipped bindings; contains no configuration secrets.
    #[serde(default)]
    pub shortcut_warnings: Vec<String>,
}

/// One selectable model from the service's catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Provider identifier the model runs on.
    pub provider: String,
    /// Canonical model identifier.
    pub id: String,
    /// Human-friendly display name.
    pub display_name: String,
    /// Context window in tokens, when the catalog knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Whether this is the currently active provider/model.
    #[serde(default)]
    pub current: bool,
    /// Position in the CLI's recommended model set, if the model is recommended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_rank: Option<u8>,
    /// Capability details sourced from the CLI's model catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
}

/// Non-secret model capabilities used to explain model choices in the desktop picker.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    /// Supports reasoning controls.
    #[serde(default)]
    pub reasoning: bool,
    /// Accepts image input.
    #[serde(default)]
    pub vision: bool,
    /// Supports tool calls.
    #[serde(default)]
    pub tool_call: bool,
    /// Supports structured output.
    #[serde(default)]
    pub structured_output: bool,
    /// Identifies a pro-tier model.
    #[serde(default)]
    pub is_pro: bool,
    /// Input modalities advertised by the CLI catalog.
    #[serde(default)]
    pub input_modalities: Vec<String>,
}

/// Parameters for selecting a provider and model for the next session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelectParams {
    /// Provider identifier.
    pub provider: String,
    /// Canonical model identifier.
    pub model: String,
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
}

/// Text restored from the durable message snapshot for a loaded session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryMessage {
    /// Stable zero-based index in the archived session snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_index: Option<usize>,
    /// Message role such as `user`, `assistant`, or `tool`.
    pub role: String,
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

/// Request identity for one bounded read-only Git snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceReviewParams {
    /// Include bounded previews of non-ignored untracked regular text files.
    #[serde(default)]
    pub include_untracked: bool,
    /// Opaque client identity used to reject superseded captures.
    pub request_id: String,
    /// Optional directory within the selected workspace; empty means the root.
    #[serde(default)]
    pub scope: Option<String>,
}

/// Current Git preview; separate from historical agent-attributed changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceReviewResult {
    /// Whether untracked previews were requested for this capture.
    #[serde(default)]
    pub include_untracked: bool,
    /// Canonical workspace-relative directory captured by this snapshot.
    #[serde(default)]
    pub scope: String,
    /// Echoed request identity.
    pub request_id: String,
    /// Canonical service workspace.
    pub workspace: String,
    /// Capture timestamp.
    pub captured_at: String,
    /// Redacted diff against HEAD or the empty tree before the first commit.
    pub patch: Option<String>,
    /// Whether the preview omits content.
    pub truncated: bool,
    /// Scope or availability explanation.
    pub note: String,
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

/// Parameters for submitting a prompt to the selected session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSubmitParams {
    /// Session that owns the new turn.
    pub session_id: String,
    /// User prompt.
    pub prompt: String,
    /// Inline text attachments read by the client, applied as content parts.
    #[serde(default)]
    pub attachments: Vec<TurnAttachment>,
}

/// One client-read text attachment for a submitted turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAttachment {
    /// Workspace-relative path used for labeling.
    pub path: String,
    /// Bounded file content.
    pub text: String,
}

/// Accepted turn response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAccepted {
    /// Session identifier.
    pub session_id: String,
    /// Runtime turn identifier.
    pub turn_id: String,
}

/// Parameters for cancelling an active turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelParams {
    /// Owning session.
    pub session_id: String,
    /// Exact turn identity to cancel.
    pub turn_id: String,
}

/// User response to a pending runtime interaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponse {
    /// Session owning the interaction.
    pub session_id: String,
    /// Turn that raised the interaction.
    pub turn_id: String,
    /// Opaque interaction identity provided by the service.
    pub interaction_id: String,
    /// Opaque choice identity provided by the service, if selecting an option.
    pub choice_id: Option<String>,
    /// Opaque identities selected for each step in a wizard interaction.
    #[serde(default)]
    pub choice_ids: Vec<String>,
    /// Free-form answer for text questions.
    pub text: Option<String>,
    /// Free-form answers for each ordered wizard step.
    #[serde(default)]
    pub answer_texts: Vec<Option<String>>,
    /// Whether the interaction was explicitly cancelled by the user.
    #[serde(default)]
    pub cancelled: bool,
}

/// Interaction request sent from the service to the desktop client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    /// Session owning the interaction.
    pub session_id: String,
    /// Turn that raised the interaction.
    pub turn_id: String,
    /// Opaque interaction identity.
    pub interaction_id: String,
    /// Kind of interaction required.
    pub kind: InteractionKind,
    /// Whether the runtime expects an ordered wizard response.
    #[serde(default)]
    pub wizard: bool,
    /// Dialog title or question prompt.
    pub title: String,
    /// Supporting text shown by the client.
    pub details: Vec<String>,
    /// Available choices, preserving each runtime's opaque identity.
    pub choices: Vec<InteractionChoice>,
    /// Ordered multi-step questions when the runtime request is a wizard.
    #[serde(default)]
    pub steps: Vec<InteractionStep>,
    /// Optional selected choice identity.
    pub selected_choice_id: Option<String>,
    /// Optional selected choice identities, one per wizard step.
    #[serde(default)]
    pub selected_choice_ids: Vec<String>,
}

/// One ordered step in a question or approval wizard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionStep {
    /// Step label.
    pub title: String,
    /// Question or instruction for this step.
    pub question: String,
    /// Choices preserving exact service-issued identities.
    pub choices: Vec<InteractionChoice>,
    /// Optional selected identity.
    pub selected_choice_id: Option<String>,
    /// Whether the client may submit a free-form answer.
    pub allow_freeform: bool,
}

/// Interaction category understood by native clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    /// A tool or policy approval decision.
    Approval,
    /// Plan approval decision.
    PlanApproval,
    /// User-facing question requiring a response.
    Question,
    /// Non-security choice picker.
    Choice,
    /// Unsupported or informational transient request.
    Other,
}

/// One display label paired with a stable opaque identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionChoice {
    /// Opaque identity returned unchanged by the client.
    pub id: String,
    /// User-facing label.
    pub label: String,
    /// Optional supporting description.
    pub description: Option<String>,
    /// Whether the service recommends this option.
    pub recommended: bool,
}

/// Canonical runtime event notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEventNotification {
    /// Session owning the event.
    pub session_id: String,
    /// Turn identity when the event is turn scoped.
    pub turn_id: Option<String>,
    /// Authoritative, schema-versioned runtime event.
    pub event: VersionedThreadEvent,
}

/// Ephemeral active primary-agent display state for one live session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryAgentChanged {
    /// Session owning the active-agent state.
    pub session_id: String,
    /// Sanitized active-agent label, or `None` to use the configured fallback.
    pub display_name: Option<String>,
}

/// WebMCP bridge state exposed to the desktop client without pairing secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebmcpBridgeStatus {
    /// Whether the listener is running in this desktop service process.
    pub running: bool,
    /// Loopback WebSocket endpoint when the listener is running.
    pub endpoint: Option<String>,
    /// Exact origins allowed to pair. Empty means the next explicit pair request binds one origin.
    pub allowed_origins: Vec<String>,
    /// Selected durable session, if any.
    pub session_id: Option<String>,
    /// Whether an active desktop runtime can accept a browser turn now.
    pub turns_available: bool,
    /// Canonical workspace used by the bridge.
    pub workspace: String,
}

/// Request to issue a one-time WebMCP pairing code for one exact browser origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebmcpPairParams {
    /// Exact browser origin, including scheme and optional port, without a path.
    pub origin: String,
    /// Revoke existing browser sessions before issuing the replacement code.
    #[serde(default)]
    pub replace: bool,
}

/// One-time pairing material. The code is returned only by the pair operation and is never persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebmcpPairResult {
    /// Loopback WebSocket endpoint for the paired browser.
    pub endpoint: String,
    /// Exact origin bound to the pairing code.
    pub origin: String,
    /// Short-lived, single-use pairing code.
    pub pairing_code: String,
    /// Remaining lifetime of the pairing code.
    pub expires_in_secs: u64,
}

/// Known desktop JSON-RPC methods.
pub mod method {
    /// Initialize and negotiate protocol support.
    pub const INITIALIZE: &str = "desktop/initialize";
    /// Capture a bounded read-only Git preview, including valid unborn branches.
    pub const WORKSPACE_REVIEW: &str = "workspace/review";
    /// List sessions in the selected workspace.
    pub const SESSION_LIST: &str = "session/list";
    /// Create a session.
    pub const SESSION_CREATE: &str = "session/create";
    /// Load a durable session.
    pub const SESSION_LOAD: &str = "session/load";
    /// Load one bounded earlier page from the selected durable session.
    pub const SESSION_HISTORY: &str = "session/history";
    /// Read one bounded page of archived tool output on demand.
    pub const SESSION_OUTPUT_READ: &str = "session/output/read";
    /// Close the selected session after cleanup.
    pub const SESSION_CLOSE: &str = "session/close";
    /// Submit a user turn.
    pub const TURN_SUBMIT: &str = "turn/submit";
    /// Cancel a running turn.
    pub const TURN_CANCEL: &str = "turn/cancel";
    /// Respond to a pending interaction.
    pub const INTERACTION_RESPOND: &str = "interaction/respond";
    /// Discover active provider/model setup.
    pub const CONFIG_DISCOVER: &str = "config/discover";
    /// Change runtime session options while idle.
    pub const CONFIG_OPTIONS: &str = "config/options";
    /// Persist the selected built-in CLI theme.
    pub const CONFIG_THEME: &str = "config/theme";
    /// Read a bounded, credential-filtered CLI settings catalog.
    pub const CONFIG_SETTINGS_READ: &str = "config/settings/read";
    /// Persist one CLI setting through the shared configuration service.
    pub const CONFIG_SETTINGS_WRITE: &str = "config/settings/write";
    /// Discover installed CLI skills and Agent Plugins.
    pub const COMPOSER_RESOURCES: &str = "composer/resources";
    /// List selectable models from the service's catalog.
    pub const CONFIG_MODELS: &str = "config/models";
    /// Select the provider/model used for the next session.
    pub const CONFIG_SELECT: &str = "config/select";
    /// Shut down the local service after owned tasks finish.
    pub const SHUTDOWN: &str = "desktop/shutdown";
    /// Stream an authoritative runtime event.
    pub const THREAD_EVENT: &str = "thread/event";
    /// Ask the client to resolve a runtime interaction.
    pub const INTERACTION_REQUESTED: &str = "interaction/requested";
    /// Publish the active primary agent for a live session.
    pub const PRIMARY_AGENT_CHANGED: &str = "session/primaryAgentChanged";
    /// Publish visible turn state.
    pub const TURN_STATUS: &str = "turn/status";
    /// Read bridge state and the non-secret configured origin list.
    pub const WEBMCP_STATUS: &str = "webmcp/status";
    /// Start or reuse the WebMCP bridge and issue an origin-bound pairing code.
    pub const WEBMCP_PAIR: &str = "webmcp/pair";
    /// Revoke the bridge's pairings and stop its listener.
    pub const WEBMCP_STOP: &str = "webmcp/stop";
}

/// Maximum message count accepted by one desktop archive-history page request.
pub const MAX_SESSION_HISTORY_PAGE_MESSAGES: usize = 160;

/// Maximum captured output returned by one on-demand desktop page.
pub const MAX_SESSION_OUTPUT_PAGE_BYTES: usize = 32 * 1024;

/// Marker appended while more archived tool output is available.
pub const RESTORED_OUTPUT_TRUNCATION_SUFFIX: &str = "\n[Output truncated in restored history.]";

/// Error raised by protocol framing, negotiation, or identity validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// A frame exceeds the configured byte limit.
    FrameTooLarge {
        /// Maximum accepted frame size in bytes.
        limit: usize,
    },
    /// The client and service do not share a supported protocol version.
    IncompatibleVersion {
        /// Client-supported range.
        client: ProtocolRange,
        /// Server version.
        server: ProtocolVersion,
    },
    /// Interaction does not exactly match the active session, turn, and id.
    StaleInteraction,
    /// Interaction was already resolved or never registered.
    UnknownInteraction,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FrameTooLarge { limit } => write!(formatter, "JSON-RPC frame exceeds {limit} bytes"),
            Self::IncompatibleVersion { client, server } => write!(
                formatter,
                "incompatible desktop protocol versions: client {}.{}..={}.{}, server {}.{}",
                client.minimum.major,
                client.minimum.minor,
                client.maximum.major,
                client.maximum.minor,
                server.major,
                server.minor
            ),
            Self::StaleInteraction => {
                formatter.write_str("interaction response does not match the active session and turn")
            }
            Self::UnknownInteraction => formatter.write_str("interaction is no longer pending"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Track pending interaction identities and reject stale or duplicate replies.
#[derive(Debug, Default)]
pub struct InteractionRegistry {
    pending: HashMap<String, (String, String)>,
}

impl InteractionRegistry {
    /// Register an interaction id for an exact session and turn.
    pub fn insert(&mut self, interaction: &InteractionRequest) -> Result<(), ProtocolError> {
        if self.pending.contains_key(&interaction.interaction_id) {
            return Err(ProtocolError::UnknownInteraction);
        }
        let replaced = self
            .pending
            .insert(interaction.interaction_id.clone(), (interaction.session_id.clone(), interaction.turn_id.clone()));
        debug_assert!(replaced.is_none(), "duplicate interaction identity was checked before insertion");
        Ok(())
    }

    /// Validate a response without consuming the pending interaction.
    pub fn validate(&self, response: &InteractionResponse) -> Result<(), ProtocolError> {
        let Some((session_id, turn_id)) = self.pending.get(&response.interaction_id) else {
            return Err(ProtocolError::UnknownInteraction);
        };
        if session_id != &response.session_id || turn_id != &response.turn_id {
            return Err(ProtocolError::StaleInteraction);
        }
        Ok(())
    }

    /// Validate and consume exactly one matching response.
    pub fn resolve(&mut self, response: &InteractionResponse) -> Result<(), ProtocolError> {
        self.validate(response)?;
        if self.pending.remove(&response.interaction_id).is_none() {
            return Err(ProtocolError::UnknownInteraction);
        }
        Ok(())
    }

    /// Drop all requests for a turn after cancellation or completion.
    pub fn finish_turn(&mut self, session_id: &str, turn_id: &str) {
        self.pending
            .retain(|_, (pending_session, pending_turn)| pending_session != session_id || pending_turn != turn_id);
    }

    /// Drop all requests for a disconnected session.
    pub fn close_session(&mut self, session_id: &str) {
        self.pending.retain(|_, (pending_session, _)| pending_session != session_id);
    }
}

/// Read one bounded JSON-RPC line. A final unterminated line is accepted at EOF.
pub async fn read_frame<R>(reader: &mut R) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::with_capacity(1024);
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if frame.is_empty() { Ok(None) } else { Ok(Some(frame)) };
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        let content_end = if newline.is_some() && available.get(take.saturating_sub(1)) == Some(&b'\n') {
            take.saturating_sub(1)
        } else {
            take
        };
        let content_end = if content_end > 0 && available.get(content_end.saturating_sub(1)) == Some(&b'\r') {
            content_end.saturating_sub(1)
        } else {
            content_end
        };

        if frame.len().saturating_add(content_end) > MAX_FRAME_BYTES {
            reader.consume(take);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                ProtocolError::FrameTooLarge { limit: MAX_FRAME_BYTES },
            ));
        }
        let content = available
            .get(..content_end)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid buffered frame boundary"))?;
        frame.extend_from_slice(content);
        reader.consume(take);

        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

/// Write one compact JSON-RPC line and flush it to the client.
pub async fn write_frame<W, T>(writer: &mut W, message: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let encoded = serde_json::to_vec(message).map_err(io::Error::other)?;
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            ProtocolError::FrameTooLarge { limit: MAX_FRAME_BYTES },
        ));
    }
    writer.write_all(&encoded).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

/// Build a JSON-RPC success response.
pub fn success_response(id: RequestId, result: impl Serialize) -> Result<Response, serde_json::Error> {
    Ok(Response {
        jsonrpc: "2.0".to_string(),
        id,
        result: serde_json::to_value(result)?,
    })
}

/// Build a JSON-RPC error response with a sanitized public message.
pub fn error_response(id: Option<RequestId>, code: i32, message: impl Into<String>) -> ErrorResponse {
    ErrorResponse {
        jsonrpc: "2.0".to_string(),
        id,
        error: RpcError { code, message: message.into(), data: None },
    }
}

/// Create the bounded live event channel that streams runtime events from the
/// agent runloop to the desktop service pump.
///
/// Unlike a plain bounded channel, a full queue never fails the producer:
/// `item.updated` snapshots replace the pending snapshot for the same item and
/// consecutive `plan.delta` events for one item merge into one entry, so a
/// slow consumer degrades the stream instead of failing a healthy turn.
/// Ordered events are only dropped when the queue holds no coalescable entry,
/// and every such drop is counted for consumer-side degradation reporting.
pub fn live_event_channel(capacity: usize) -> (LiveEventSender, LiveEventReceiver) {
    assert!(capacity > 0, "live event capacity must be positive");
    let inner = std::sync::Arc::new(LiveEventQueueInner {
        capacity,
        queue: std::sync::Mutex::new(VecDeque::new()),
        senders: std::sync::atomic::AtomicUsize::new(1),
        receiver_alive: std::sync::atomic::AtomicBool::new(true),
        dropped_ordered: std::sync::atomic::AtomicU64::new(0),
        dropped_stale: std::sync::atomic::AtomicU64::new(0),
        notify: tokio::sync::Notify::new(),
    });
    (LiveEventSender { inner: inner.clone() }, LiveEventReceiver { inner })
}

/// How a streaming event combines with the pending entry for the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoalesceOp {
    /// The new event is a full snapshot that supersedes the pending one.
    Replace,
    /// The new event carries incremental text appended to the pending one.
    Merge,
}

/// One queue entry: a coalescable streaming event keyed by item, or an
/// ordered event that must keep its relative position.
struct LiveEventSlot {
    key: Option<String>,
    op: CoalesceOp,
    event: VersionedThreadEvent,
}

struct LiveEventQueueInner {
    capacity: usize,
    queue: std::sync::Mutex<VecDeque<LiveEventSlot>>,
    senders: std::sync::atomic::AtomicUsize,
    receiver_alive: std::sync::atomic::AtomicBool,
    dropped_ordered: std::sync::atomic::AtomicU64,
    dropped_stale: std::sync::atomic::AtomicU64,
    notify: tokio::sync::Notify,
}

impl LiveEventQueueInner {
    fn classify(event: &VersionedThreadEvent) -> Option<(String, CoalesceOp)> {
        match event.event() {
            vtcode_exec_events::ThreadEvent::ItemUpdated(update) => Some((update.item.id.clone(), CoalesceOp::Replace)),
            vtcode_exec_events::ThreadEvent::PlanDelta(delta) => Some((delta.item_id.clone(), CoalesceOp::Merge)),
            _ => None,
        }
    }

    /// Append an event, coalescing streaming updates and keeping the queue
    /// bounded. Returns `Err(())` only when the consumer half was dropped.
    fn publish(&self, event: VersionedThreadEvent) -> Result<(), ()> {
        if !self.receiver_alive.load(std::sync::atomic::Ordering::Acquire) {
            return Err(());
        }
        let mut queue = self.queue.lock().map_err(|_poisoned| ())?;
        let classified = Self::classify(&event);
        let slot_key = classified.as_ref().map(|(key, _)| key.clone());
        let slot_op = classified.as_ref().map_or(CoalesceOp::Replace, |(_, op)| *op);
        if let Some((key, op)) = classified {
            let pending_slot = queue
                .iter()
                .position(|slot| slot.key.as_deref() == Some(key.as_str()) && slot.op == op)
                .and_then(|index| queue.remove(index).map(|removed| (index, removed)));
            if let Some((index, pending)) = pending_slot {
                // Re-insert at the removed slot's position so the relative
                // order of every other queued event is preserved.
                let merged = match op {
                    CoalesceOp::Replace => event,
                    CoalesceOp::Merge => Self::merge_plan_deltas(pending.event, event),
                };
                queue.insert(index, LiveEventSlot { key: Some(key), op, event: merged });
                drop(queue);
                self.notify.notify_waiters();
                return Ok(());
            }
        }
        if queue.len() >= self.capacity {
            // Prefer displacing a stale streaming entry over dropping an
            // ordered event: the latest snapshot for an item always arrives
            // with its terminal `item.completed` event.
            let victim = queue.iter().position(|slot| slot.key.is_some());
            match victim {
                Some(index) => {
                    drop(queue.remove(index));
                    let _ = self.dropped_stale.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                None => {
                    drop(queue.pop_front());
                    let _ = self.dropped_ordered.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        queue.push_back(LiveEventSlot { key: slot_key, op: slot_op, event });
        drop(queue);
        self.notify.notify_waiters();
        Ok(())
    }

    /// Concatenate two `plan.delta` events for the same item, keeping the
    /// newest metadata and the freshest non-empty recovery snapshot.
    fn merge_plan_deltas(pending: VersionedThreadEvent, next: VersionedThreadEvent) -> VersionedThreadEvent {
        let (pending, next) = (pending.into_event(), next.into_event());
        match (pending, next) {
            (vtcode_exec_events::ThreadEvent::PlanDelta(older), vtcode_exec_events::ThreadEvent::PlanDelta(newer)) => {
                let mut merged = newer.clone();
                let mut text = older.delta;
                text.push_str(&newer.delta);
                merged.delta = text;
                if merged.recovery.is_none() {
                    merged.recovery = older.recovery;
                }
                VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::PlanDelta(merged))
            }
            (_, fallback) => VersionedThreadEvent::new(fallback),
        }
    }
}

/// Producer half of the live event channel.
pub struct LiveEventSender {
    inner: std::sync::Arc<LiveEventQueueInner>,
}

impl Clone for LiveEventSender {
    fn clone(&self) -> Self {
        let _ = self.inner.senders.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self { inner: std::sync::Arc::clone(&self.inner) }
    }
}

impl LiveEventSender {
    /// Publish one event. Coalesces streaming updates when the queue is
    /// saturated; fails only when the consumer half is gone.
    pub fn publish(&self, event: VersionedThreadEvent) -> Result<(), LiveEventSendError> {
        self.inner.publish(event).map_err(|()| LiveEventSendError(()))
    }
}

impl Drop for LiveEventSender {
    fn drop(&mut self) {
        let _ = self.inner.senders.fetch_sub(1, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }
}

/// Consumer half of the live event channel.
pub struct LiveEventReceiver {
    inner: std::sync::Arc<LiveEventQueueInner>,
}

impl LiveEventReceiver {
    /// Receive the next event, waiting while the queue is empty.
    ///
    /// Returns `None` once every sender was dropped and the queue drained.
    pub async fn recv(&self) -> Option<VersionedThreadEvent> {
        loop {
            {
                let mut queue = self.inner.queue.lock().ok()?;
                if let Some(slot) = queue.pop_front() {
                    return Some(slot.event);
                }
                if self.inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return None;
                }
            }
            let notified = self.inner.notify.notified();
            {
                let queue = self.inner.queue.lock().ok()?;
                if !queue.is_empty() {
                    continue;
                }
                if self.inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return None;
                }
            }
            notified.await;
        }
    }

    /// Take and reset the count of ordered events dropped under saturation.
    pub fn take_dropped_ordered(&self) -> u64 {
        self.inner.dropped_ordered.swap(0, std::sync::atomic::Ordering::Relaxed)
    }

    /// Take and reset the count of superseded streaming snapshots displaced
    /// under saturation. These are best-effort previews, not stream gaps.
    pub fn take_dropped_stale(&self) -> u64 {
        self.inner.dropped_stale.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for LiveEventReceiver {
    fn drop(&mut self) {
        self.inner.receiver_alive.store(false, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }
}

/// The consumer half of the live event channel is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveEventSendError(());

impl std::fmt::Display for LiveEventSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("live event consumer disconnected")
    }
}

impl std::error::Error for LiveEventSendError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn legacy_session_load_defaults_review_history() {
        let loaded: SessionLoadResult = serde_json::from_value(serde_json::json!({
            "session": {"sessionId": "legacy", "title": "Legacy", "workspace": "/workspace", "completed": true, "resumable": true},
            "messages": [{"role": "user", "text": "Continue"}]
        })).unwrap();
        assert!(loaded.review_items.is_empty());
        assert!(!loaded.review_history_incomplete);
        assert!(loaded.activity_items.is_empty());
        assert!(loaded.activity_output_cursors.is_empty());
        assert!(!loaded.activity_history_incomplete);
        assert_eq!(loaded.history_start_index, 0);
        assert!(!loaded.history_has_earlier);
        assert_eq!(loaded.messages[0].archive_index, None);
        assert_eq!(loaded.messages[0].text, "Continue");
        let round_trip: SessionLoadResult = serde_json::from_value(serde_json::to_value(&loaded).unwrap()).unwrap();
        assert_eq!(round_trip, loaded);
    }

    #[test]
    fn session_history_page_round_trips_archive_cursor_and_request_identity() {
        let params = SessionHistoryParams {
            session_id: "saved-session".into(),
            request_id: "page-2".into(),
            before_index: 160,
            limit: 80,
        };
        let page = SessionHistoryPage {
            session_id: params.session_id.clone(),
            request_id: params.request_id.clone(),
            start_index: 80,
            messages: vec![SessionHistoryMessage {
                archive_index: Some(80),
                role: "assistant".into(),
                text: "Earlier reply".into(),
            }],
            has_earlier: true,
        };
        let decoded_params: SessionHistoryParams =
            serde_json::from_value(serde_json::to_value(&params).unwrap()).unwrap();
        let decoded_page: SessionHistoryPage = serde_json::from_value(serde_json::to_value(&page).unwrap()).unwrap();
        assert_eq!(decoded_params, params);
        assert_eq!(decoded_page, page);
    }

    #[test]
    fn session_output_page_round_trips_bounded_cursor_and_request_identity() {
        let params = SessionOutputReadParams {
            session_id: "saved-session".into(),
            request_id: "output-page-2".into(),
            item_id: "tool-output-1".into(),
            offset: 32_768,
        };
        let page = SessionOutputPage {
            session_id: params.session_id.clone(),
            request_id: params.request_id.clone(),
            item_id: params.item_id.clone(),
            offset: params.offset,
            next_offset: params.offset + 3,
            total_bytes: params.offset + 8,
            text: "界".into(),
            has_more: true,
        };
        let loaded = SessionLoadResult {
            session: SessionSummary {
                session_id: params.session_id.clone(),
                title: None,
                workspace: "/workspace".into(),
                completed: true,
                resumable: true,
            },
            messages: Vec::new(),
            history_start_index: 0,
            history_has_earlier: false,
            review_items: Vec::new(),
            review_history_incomplete: false,
            activity_items: Vec::new(),
            activity_output_cursors: vec![SessionOutputCursor {
                item_id: params.item_id.clone(),
                next_offset: params.offset,
                total_bytes: page.total_bytes,
            }],
            activity_history_incomplete: true,
        };

        assert_eq!(
            serde_json::from_value::<SessionOutputReadParams>(serde_json::to_value(&params).unwrap()).unwrap(),
            params
        );
        assert_eq!(serde_json::from_value::<SessionOutputPage>(serde_json::to_value(&page).unwrap()).unwrap(), page);
        assert_eq!(
            serde_json::from_value::<SessionLoadResult>(serde_json::to_value(&loaded).unwrap()).unwrap(),
            loaded
        );
    }

    #[test]
    fn negotiation_selects_server_version_and_rejects_major_mismatch() {
        let range = ProtocolRange {
            minimum: ProtocolVersion { major: 1, minor: 0 },
            maximum: PROTOCOL_VERSION,
        };
        assert_eq!(range.negotiate(PROTOCOL_VERSION), Ok(PROTOCOL_VERSION));
        let tracked_only: WorkspaceReviewParams =
            serde_json::from_value(serde_json::json!({"requestId":"old-client"})).unwrap();
        assert!(!tracked_only.include_untracked);
        let with_untracked: WorkspaceReviewParams =
            serde_json::from_value(serde_json::json!({"requestId":"new-client","includeUntracked":true})).unwrap();
        assert!(with_untracked.include_untracked);
        assert!(matches!(
            ProtocolRange {
                minimum: ProtocolVersion { major: 2, minor: 0 },
                maximum: ProtocolVersion { major: 2, minor: 1 }
            }
            .negotiate(PROTOCOL_VERSION),
            Err(ProtocolError::IncompatibleVersion { .. })
        ));
    }

    #[test]
    fn cli_settings_requests_keep_target_versions_and_typed_values() {
        let request = ConfigSettingsWriteParams {
            target: ConfigTarget::Workspace,
            path: "agent.max_conversation_turns".into(),
            value: serde_json::json!(24),
            expected_layer_version: Some("version-a".into()),
        };
        let serialized = serde_json::to_value(&request).expect("serialize request");
        assert_eq!(serialized["target"], "workspace");
        assert_eq!(serialized["expectedLayerVersion"], "version-a");
        assert_eq!(serialized["value"], 24);
        assert_eq!(
            serde_json::from_value::<ConfigSettingsWriteParams>(serialized).expect("deserialize request"),
            request
        );
        assert!(
            serde_json::from_value::<ConfigSettingsWriteParams>(serde_json::json!({
                "target": "workspace",
                "path": "agent.enabled",
                "value": true,
                "unexpected": "field"
            }))
            .is_err()
        );
    }

    #[test]
    fn legacy_cli_settings_rows_default_to_editable_without_metadata() {
        let setting: ConfigSetting = serde_json::from_value(serde_json::json!({
            "path": "agent.default_model",
            "value": "model-a",
            "origin": "workspace"
        }))
        .expect("legacy settings row");
        assert!(setting.editable);
        assert!(setting.description.is_none());
        assert!(setting.options.is_empty());
    }

    #[test]
    fn interaction_responses_are_session_turn_scoped_and_single_use() {
        let mut registry = InteractionRegistry::default();
        let interaction = InteractionRequest {
            session_id: "session-a".to_string(),
            turn_id: "turn-9".to_string(),
            interaction_id: "interaction-x".to_string(),
            kind: InteractionKind::Approval,
            wizard: false,
            title: "Approve tool".to_string(),
            details: vec![],
            choices: vec![InteractionChoice {
                id: "runtime-choice-allow".to_string(),
                label: "Allow once".to_string(),
                description: None,
                recommended: true,
            }],
            steps: Vec::new(),
            selected_choice_id: None,
            selected_choice_ids: Vec::new(),
        };
        registry.insert(&interaction).expect("register request");
        let stale = InteractionResponse {
            session_id: "session-a".to_string(),
            turn_id: "turn-8".to_string(),
            interaction_id: "interaction-x".to_string(),
            choice_id: Some("runtime-choice-allow".to_string()),
            choice_ids: Vec::new(),
            text: None,
            answer_texts: Vec::new(),
            cancelled: false,
        };
        assert_eq!(registry.validate(&stale), Err(ProtocolError::StaleInteraction));
        assert_eq!(registry.resolve(&stale), Err(ProtocolError::StaleInteraction));
        let valid = InteractionResponse { turn_id: "turn-9".to_string(), ..stale };
        assert_eq!(registry.validate(&valid), Ok(()));
        assert_eq!(registry.validate(&valid), Ok(()));
        assert_eq!(registry.resolve(&valid), Ok(()));
        assert_eq!(registry.validate(&valid), Err(ProtocolError::UnknownInteraction));
        assert_eq!(registry.resolve(&valid), Err(ProtocolError::UnknownInteraction));
    }

    #[tokio::test]
    async fn bounded_line_reader_accepts_crlf_and_final_unterminated_frame() -> Result<(), Box<dyn Error>> {
        let input = b"{\"jsonrpc\":\"2.0\"}\r\n{}";
        let mut reader = tokio::io::BufReader::new(&input[..]);
        assert_eq!(read_frame(&mut reader).await?, Some(br#"{"jsonrpc":"2.0"}"#.to_vec()));
        assert_eq!(read_frame(&mut reader).await?, Some(b"{}".to_vec()));
        assert_eq!(read_frame(&mut reader).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn bounded_line_reader_rejects_oversized_input() {
        let input = vec![b'x'; MAX_FRAME_BYTES + 1];
        let mut reader = tokio::io::BufReader::new(&input[..]);
        let error = read_frame(&mut reader).await.expect_err("oversized frame");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeds"));
    }

    #[test]
    fn protocol_methods_and_wire_structs_use_camel_case() -> Result<(), Box<dyn Error>> {
        let params = InteractionResponse {
            session_id: "s".to_string(),
            turn_id: "t".to_string(),
            interaction_id: "i".to_string(),
            choice_id: Some("choice-opaque".to_string()),
            choice_ids: Vec::new(),
            text: None,
            answer_texts: Vec::new(),
            cancelled: false,
        };
        let theme = ThemePreferenceSelectParams { theme: "ciapre-blue".into() };
        let theme_json = serde_json::to_value(theme)?;
        assert_eq!(theme_json, serde_json::json!({ "theme": "ciapre-blue" }));
        assert!(
            serde_json::from_value::<ThemePreferenceSelectParams>(
                serde_json::json!({ "theme": "ciapre-blue", "path": "/tmp/unsafe" })
            )
            .is_err()
        );
        let value = serde_json::to_value(params)?;
        assert_eq!(value["sessionId"], "s");
        assert_eq!(value["interactionId"], "i");
        assert_eq!(value["choiceId"], "choice-opaque");
        assert_eq!(method::SESSION_CREATE, "session/create");

        let webmcp_pair: WebmcpPairParams = serde_json::from_value(serde_json::json!({
            "origin": "https://vtcode.vinhnx.chatgpt.site"
        }))?;
        assert_eq!(webmcp_pair.origin, "https://vtcode.vinhnx.chatgpt.site");
        assert!(!webmcp_pair.replace);
        let webmcp_result = serde_json::to_value(WebmcpPairResult {
            endpoint: "ws://127.0.0.1:4000/webmcp".into(),
            origin: "https://vtcode.vinhnx.chatgpt.site".into(),
            pairing_code: "ephemeral-code".into(),
            expires_in_secs: 60,
        })?;
        assert_eq!(webmcp_result["pairingCode"], "ephemeral-code");
        assert_eq!(webmcp_result["expiresInSecs"], 60);
        assert_eq!(method::WEBMCP_PAIR, "webmcp/pair");
        Ok(())
    }

    #[test]
    fn optional_protocol_extensions_are_camel_cased() -> Result<(), Box<dyn Error>> {
        // A 1.0-era snapshot without the additive fields still deserializes.
        let snapshot: ConfigurationSnapshot = serde_json::from_value(serde_json::json!({
            "provider": "ollama",
            "model": "gpt-oss:20b",
            "authenticated": true,
            "credentialSetupCommand": "vtcode login ollama",
        }))?;
        assert_eq!(snapshot.sandbox, None);
        assert_eq!(snapshot.context_window, None);
        assert_eq!(snapshot.shared_ui, None);

        let legacy_options: SessionOptionsSnapshot = serde_json::from_value(serde_json::json!({
            "permissionMode": "ask",
            "mode": "build",
            "reasoningEffort": "medium",
            "supportedEfforts": ["low", "medium", "high"],
            "speed": null
        }))?;
        assert!(!legacy_options.fast_mode_supported);
        assert!(!legacy_options.fast_mode_enabled);
        assert_eq!(legacy_options.fast_mode_label, None);
        assert!(legacy_options.available_modes.is_empty());

        let value = serde_json::to_value(ConfigurationSnapshot {
            provider: "ollama".to_string(),
            model: "gpt-oss:20b".to_string(),
            authenticated: true,
            credential_setup_command: "vtcode login ollama".to_string(),
            sandbox: Some("Ask before protected actions".to_string()),
            context_window: Some(131_072),
            shared_ui: None,
            session_options: None,
        })?;
        assert_eq!(value["sandbox"], "Ask before protected actions");
        assert_eq!(value["contextWindow"], 131_072);

        let submit: TurnSubmitParams = serde_json::from_value(serde_json::json!({
            "sessionId": "s",
            "prompt": "hi",
        }))?;
        assert!(submit.attachments.is_empty(), "attachments default for 1.0 clients");

        let value = serde_json::to_value(TurnSubmitParams {
            session_id: "s".to_string(),
            prompt: "hi".to_string(),
            attachments: vec![TurnAttachment {
                path: "src/a.rs".to_string(),
                text: "fn a() {}".to_string(),
            }],
        })?;
        assert_eq!(value["attachments"][0]["path"], "src/a.rs");

        let model = serde_json::to_value(ModelInfo {
            provider: "openai".to_string(),
            id: "gpt-6-sol".to_string(),
            display_name: "GPT-6 Sol".to_string(),
            context_window: Some(400_000),
            current: true,
            recommended_rank: Some(2),
            capabilities: Some(ModelCapabilities {
                reasoning: true,
                vision: true,
                tool_call: true,
                structured_output: false,
                is_pro: false,
                input_modalities: vec!["text".into(), "image".into()],
            }),
        })?;
        assert_eq!(model["displayName"], "GPT-6 Sol");
        assert_eq!(model["contextWindow"], 400_000);
        assert_eq!(model["current"], true);
        assert_eq!(model["recommendedRank"], 2);
        assert_eq!(model["capabilities"]["reasoning"], true);
        assert_eq!(model["capabilities"]["inputModalities"], serde_json::json!(["text", "image"]));
        let legacy_model: ModelInfo = serde_json::from_value(serde_json::json!({
            "provider": "openai",
            "id": "gpt-6-sol",
            "displayName": "GPT-6 Sol",
            "current": true
        }))?;
        assert_eq!(legacy_model.capabilities, None);
        assert_eq!(legacy_model.recommended_rank, None);
        assert_eq!(method::CONFIG_MODELS, "config/models");
        assert_eq!(method::CONFIG_SELECT, "config/select");
        Ok(())
    }

    fn item_update(id: &str, text: &str) -> VersionedThreadEvent {
        use vtcode_exec_events::{AgentMessageItem, ItemUpdatedEvent, ThreadItem, ThreadItemDetails};
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: id.to_string(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.to_string() }),
            },
        }))
    }

    fn plan_delta(item_id: &str, delta: &str) -> VersionedThreadEvent {
        use vtcode_exec_events::PlanDeltaEvent;
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
            thread_id: "thread".to_string(),
            turn_id: "turn".to_string(),
            item_id: item_id.to_string(),
            delta: delta.to_string(),
            recovery: None,
        })))
    }

    fn turn_completed() -> VersionedThreadEvent {
        use vtcode_exec_events::{TurnCompletedEvent, Usage};
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage {
                input_tokens: 0,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
                output_tokens: 0,
            },
            in_progress_exec_sessions: Vec::new(),
        }))
    }

    fn thread_started(id: &str) -> VersionedThreadEvent {
        use vtcode_exec_events::ThreadStartedEvent;
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: id.to_string(),
        }))
    }

    #[tokio::test]
    async fn saturated_queue_never_fails_the_producer_and_keeps_lifecycle_events() {
        let (sender, receiver) = live_event_channel(8);
        // Fill the queue past capacity with streaming snapshots and deltas.
        for round in 0..64 {
            sender
                .publish(item_update(&format!("item-{}", round % 4), &format!("round {round}")))
                .expect("publish");
            sender.publish(plan_delta("plan", &format!("{round} "))).expect("publish");
        }
        // A lifecycle event must be admitted even while the queue is full.
        sender.publish(turn_completed()).expect("lifecycle publish must not fail");
        // The producer going away ends the stream after draining.
        drop(sender);

        let mut ordered = Vec::new();
        while let Some(event) = receiver.recv().await {
            ordered.push(event);
        }

        assert_eq!(
            ordered.last().expect("drained events"),
            &turn_completed(),
            "lifecycle event must survive saturation"
        );
        let plan_events: Vec<_> = ordered
            .iter()
            .filter(|event| matches!(event.event(), vtcode_exec_events::ThreadEvent::PlanDelta(_)))
            .collect();
        assert_eq!(plan_events.len(), 1, "all plan deltas for one item merge into one event");
        if let vtcode_exec_events::ThreadEvent::PlanDelta(merged) = plan_events[0].event() {
            assert_eq!(merged.delta, (0..64).map(|round| format!("{round} ")).collect::<String>());
        } else {
            panic!("expected merged plan delta");
        }
        assert!(ordered.len() <= 8 + 1, "queue stays bounded: {}", ordered.len());
        assert_eq!(receiver.take_dropped_ordered(), 0, "no ordered event dropped while coalescable entries existed");
    }

    #[tokio::test]
    async fn saturated_ordered_events_drop_oldest_and_count_the_gap() {
        let (sender, receiver) = live_event_channel(4);
        for round in 0..10 {
            sender.publish(thread_started(&round.to_string())).expect("publish");
        }
        drop(sender);
        let mut drained = Vec::new();
        while let Some(event) = receiver.recv().await {
            drained.push(event);
        }
        assert_eq!(drained.len(), 4, "queue stays bounded");
        assert_eq!(receiver.take_dropped_ordered(), 6, "every displaced ordered event is counted");
        if let vtcode_exec_events::ThreadEvent::ThreadStarted(last) = drained[3].event() {
            assert_eq!(last.thread_id, "9", "the newest ordered event must survive");
        } else {
            panic!("expected thread.started");
        }
    }

    #[tokio::test]
    async fn publish_fails_only_after_the_consumer_half_drops() {
        let (sender, receiver) = live_event_channel(4);
        sender.publish(item_update("item", "text")).expect("publish with live consumer");
        drop(receiver);
        assert!(
            sender.publish(item_update("item", "more")).is_err(),
            "publishing after the consumer dropped must fail"
        );
    }

    #[tokio::test]
    async fn cloned_senders_keep_the_stream_alive_until_the_last_one_drops() {
        let (sender, receiver) = live_event_channel(4);
        let clone = sender.clone();
        drop(sender);
        // A second sender keeps the stream reachable.
        clone.publish(item_update("item", "text")).expect("publish with live clone");
        drop(clone);
        assert!(receiver.recv().await.is_some());
        assert_eq!(receiver.recv().await, None, "drained queue with no senders ends the stream");
    }
}
