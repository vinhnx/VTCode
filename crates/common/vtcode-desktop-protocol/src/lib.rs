//! Versioned JSON-RPC messages shared by the VT Code desktop app and service.
//!
//! The protocol deliberately carries the canonical [`VersionedThreadEvent`]
//! instead of defining a second runtime event model.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vtcode_exec_events::VersionedThreadEvent;

pub mod slash_command;

/// Current protocol version supported by the desktop client and service.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 22 };

/// A turn was rejected before delivery because a startup interaction is pending.
pub const STARTUP_INTERACTION_PENDING_ERROR: i32 = -32001;

/// Fallback turn id for interactions raised with no active turn (startup
/// decisions). Produced by the service, matched by the desktop client.
pub const SESSION_START_TURN_ID: &str = "session-start";

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
    /// In-app secret-form support for the active provider, absent for
    /// login-flow/local providers and older services.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_store: Option<SecretStoreInfo>,
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

/// Open one layered config file in the platform default editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigOpenFileParams {
    /// Layer whose file should be opened.
    pub target: ConfigTarget,
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
    /// Model-only context retained for archive indexing but hidden from the transcript.
    #[serde(default)]
    pub hidden: bool,
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

/// Request one bounded MCP app-resource body for static preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppReadParams {
    /// `ui://` URI to read.
    pub uri: String,
    /// Opaque client identity used to reject superseded reads.
    pub request_id: String,
}

/// One bounded MCP app-resource body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppReadResult {
    /// URI that was read.
    pub uri: String,
    /// Echoed request identity.
    pub request_id: String,
    /// MIME type of the first text block, when the server provides one.
    pub mime_type: Option<String>,
    /// Bounded body bytes; empty when the resource has no text block.
    pub body: String,
    /// Total body bytes available across text blocks.
    pub total_bytes: u64,
    /// Whether the body was truncated to [`MAX_MCP_APP_BODY_BYTES`].
    pub truncated: bool,
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

/// Cached updater status for the desktop client. The service owns update
/// policy (channel, pin, check interval); this snapshot never triggers
/// network I/O, it only reports the last cached check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusSnapshot {
    /// Version of the running service binary.
    pub current_version: String,
    /// Latest known release version, when a check has reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// Whether `latest_version` is newer than `current_version` and not dismissed.
    #[serde(default)]
    pub update_available: bool,
    /// HTTPS release page for `latest_version`, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_url: Option<String>,
    /// Release channel the service follows (stable/beta/nightly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Whether the configured check interval has elapsed since the last check.
    #[serde(default)]
    pub check_due: bool,
}

pub mod method;

/// Maximum message count accepted by one desktop archive-history page request.
pub const MAX_SESSION_HISTORY_PAGE_MESSAGES: usize = 160;

/// Maximum captured output returned by one on-demand desktop page.
pub const MAX_SESSION_OUTPUT_PAGE_BYTES: usize = 32 * 1024;

/// Maximum MCP app-resource body returned by one `mcp/app/read` call.
pub const MAX_MCP_APP_BODY_BYTES: usize = 64 * 1024;

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

pub mod interaction_registry;
pub use interaction_registry::InteractionRegistry;

pub mod frames;
pub use frames::{read_frame, write_frame};

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

pub mod live_events;
pub use live_events::{LiveEventReceiver, LiveEventSendError, LiveEventSender, live_event_channel};

pub mod secret;
pub use secret::{SecretStoreInfo, SecretStoreParams, SecretStoreResult};

#[cfg(test)]
mod tests;
