//! Versioned JSON-RPC messages shared by the VT Code desktop app and service.
//!
//! The protocol deliberately carries the canonical [`VersionedThreadEvent`]
//! instead of defining a second runtime event model.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vtcode_exec_events::VersionedThreadEvent;

pub mod handoff_url;
pub mod slash_command;

/// Current protocol version supported by the desktop client and service.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 27 };

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

pub mod method;

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

pub mod storage;
pub use storage::{SessionDeleteResult, StorageSummary, WorktreeEntry, WorktreeRemoveParams, WorktreeRemoveResult};

pub mod composer;
pub use composer::{ComposerResource, ComposerResourceKind};

pub mod config;
pub use config::{
    ConfigOpenFileParams, ConfigSetting, ConfigSettingsSnapshot, ConfigSettingsWriteParams, ConfigSettingsWriteResult,
    ConfigTarget, ConfigTargetVersion, ConfigurationSnapshot, ModelCapabilities, ModelInfo, ModelSelectParams,
    SharedUiPreferences,
};

pub mod interaction;
pub use interaction::{InteractionChoice, InteractionKind, InteractionRequest, InteractionResponse, InteractionStep};

pub mod review;
pub use review::{
    MAX_MCP_APP_BODY_BYTES, McpAppReadParams, McpAppReadResult, WorkspaceReviewParams, WorkspaceReviewResult,
};

pub mod session;
pub use session::{
    AgentModeInfo, MAX_RECENT_PROMPTS, MAX_SESSION_HISTORY_PAGE_MESSAGES, MAX_SESSION_LIST_LIMIT,
    MAX_SESSION_OUTPUT_PAGE_BYTES, PermissionMode, RESTORED_OUTPUT_TRUNCATION_SUFFIX, RecentPrompt,
    RecentPromptsParams, RecentPromptsResult, SessionCreateParams, SessionHistoryMessage, SessionHistoryPage,
    SessionHistoryParams, SessionIdParams, SessionListParams, SessionLoadResult, SessionOptionsSelectParams,
    SessionOptionsSnapshot, SessionOutputCursor, SessionOutputPage, SessionOutputReadParams, SessionSummary,
    ThemePreferenceSelectParams,
};

pub mod turn;
pub use turn::{
    MAX_QUEUE_PREVIEW_CHARS, MAX_QUEUE_STATUS_PREVIEWS, SESSION_START_TURN_ID, STARTUP_INTERACTION_PENDING_ERROR,
    TurnAccepted, TurnAttachment, TurnCancelParams, TurnQueueStatus, TurnQueueTakeLatestParams,
    TurnQueueTakeLatestResult, TurnSubmitParams, TurnUndoFile, TurnUndoParams, TurnUndoResult, queue_preview_label,
};

pub mod update;
pub use update::UpdateStatusSnapshot;

pub mod webmcp;
pub use webmcp::{WebmcpBridgeStatus, WebmcpPairParams, WebmcpPairResult};

#[cfg(test)]
mod tests;
