//! Legacy crate-internal HTTP session types (`message_delta`, `tool_call_start`, ...).
//!
//! Not the ACP stdio `session/update` surface. Do not extend; canonical session
//! lifecycle lives in `zed/agent/session_state.rs`.
//!
//! ACP session types and lifecycle management
//!
//! This module implements the session lifecycle as defined by ACP:
//! - Session creation (session/new)
//! - Session loading (session/load)
//! - Prompt handling (session/prompt)
//! - Session updates (session/update notifications)
//!
//! Reference: <https://agentclientprotocol.com/llms.txt>

use hashbrown::HashMap;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Session state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SessionState {
    /// Session created but not yet active
    #[default]
    Created,
    /// Session is active and processing
    Active,
    /// Session is waiting for user input
    AwaitingInput,
    /// Session completed successfully
    Completed,
    /// Session was cancelled
    Cancelled,
    /// Session failed with error
    Failed,
}

/// ACP Session representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSession {
    /// Unique session identifier
    session_id: String,

    /// Current session state
    state: SessionState,

    /// Session creation timestamp (ISO 8601)
    created_at: String,

    /// Last activity timestamp (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    last_activity_at: Option<String>,

    /// Session metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, Value>,

    /// Turn counter for prompt/response cycles
    #[serde(default)]
    turn_count: u32,
}

impl AcpSession {
    /// Create a new session with the given ID
    pub(crate) fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            state: SessionState::Created,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_activity_at: None,
            metadata: HashMap::new(),
            turn_count: 0,
        }
    }

    /// Update session state
    pub(crate) fn set_state(&mut self, state: SessionState) {
        self.state = state;
        self.last_activity_at = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Increment turn counter
    pub fn increment_turn(&mut self) {
        self.turn_count += 1;
        self.last_activity_at = Some(chrono::Utc::now().to_rfc3339());
    }
}

// ============================================================================
// Session/New Request/Response
// ============================================================================

/// Parameters for session/new method
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionNewParams {
    /// Optional session metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, Value>,

    /// Optional workspace context
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<WorkspaceContext>,

    /// Optional model preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    model_preferences: Option<ModelPreferences>,
}

/// Result of session/new method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNewResult {
    /// The created session ID
    pub(crate) session_id: String,

    /// Initial session state
    #[serde(default)]
    state: SessionState,
}

// ============================================================================
// Session/Load Request/Response
// ============================================================================

/// Parameters for session/load method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoadParams {
    /// Session ID to load
    pub(crate) session_id: String,
}

/// Result of session/load method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoadResult {
    /// The loaded session
    pub(crate) session: AcpSession,

    /// Conversation history (if available)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) history: Vec<ConversationTurn>,
}

// ============================================================================
// Session/Prompt Request/Response
// ============================================================================

/// Parameters for session/prompt method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptParams {
    /// Session ID
    pub(crate) session_id: String,

    /// Prompt content (can be text, images, etc.)
    content: Vec<PromptContent>,

    /// Optional turn-specific metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, Value>,
}

/// Prompt content types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptContent {
    /// Plain text content
    Text {
        /// The text content
        text: String,
    },

    /// Image content (base64 or URL)
    Image {
        /// Image data (base64) or URL
        data: String,
        /// MIME type (e.g., "image/png")
        mime_type: String,
        /// Whether data is a URL (false = base64)
        #[serde(default)]
        is_url: bool,
    },

    /// Embedded context (file contents, etc.)
    Context {
        /// Context identifier/path
        path: String,
        /// Context content
        content: String,
        /// Language hint for syntax highlighting
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
}

impl PromptContent {
    /// Create text content
    fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Create context content
    pub fn context(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self::Context {
            path: path.into(),
            content: content.into(),
            language: None,
        }
    }
}

/// Result of session/prompt method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptResult {
    /// Turn ID for this prompt/response cycle
    pub(crate) turn_id: String,

    /// Final response content (may be streamed via notifications first)
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<String>,

    /// Tool calls made during this turn
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ToolCallRecord>,

    /// Turn completion status
    pub(crate) status: TurnStatus,
}

/// Turn completion status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    /// Turn completed successfully
    Completed,
    /// Turn was cancelled
    Cancelled,
    /// Turn failed with error
    Failed,
    /// Turn requires user input (e.g., permission approval)
    AwaitingInput,
}

// ============================================================================
// Session/RequestPermission (Client Method)
// ============================================================================

/// Parameters for session/request_permission method (client callable by agent)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionParams {
    /// Session ID
    session_id: String,

    /// Tool call requiring permission
    tool_call: ToolCallRecord,

    /// Available permission options
    options: Vec<PermissionOption>,
}

/// A permission option presented to the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionOption {
    /// Option ID
    id: String,

    /// Display label
    label: String,

    /// Detailed description
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// Result of session/request_permission
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RequestPermissionResult {
    /// User selected an option
    Selected {
        /// The selected option ID
        option_id: String,
    },
    /// User cancelled the request
    Cancelled,
}

// ============================================================================
// Session/Cancel Request
// ============================================================================

/// Parameters for session/cancel method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCancelParams {
    /// Session ID
    pub(crate) session_id: String,

    /// Optional turn ID to cancel (if not provided, cancels current turn)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) turn_id: Option<String>,
}

// ============================================================================
// Session/Update Notification (Streaming)
// ============================================================================

/// Session update notification payload
#[derive(Debug, Clone, Serialize)]
pub struct SessionUpdateNotification {
    /// Session ID
    pub(crate) session_id: String,

    /// Turn ID this update belongs to
    pub(crate) turn_id: String,

    /// Update type
    #[serde(flatten)]
    pub(crate) update: SessionUpdate,
}

impl<'de> Deserialize<'de> for SessionUpdateNotification {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SessionUpdateNotificationWire::deserialize(deserializer)?;
        let SessionUpdateNotificationWire {
            session_id,
            turn_id,
            update_type,
            delta,
            tool_call,
            tool_call_id,
            result,
            status,
            code,
            message,
            request,
        } = wire;
        let update = match update_type.as_str() {
            "message_delta" => SessionUpdate::MessageDelta {
                delta: required_update_field(delta, "delta", &update_type).map_err(de::Error::custom)?,
            },
            "tool_call_start" => SessionUpdate::ToolCallStart {
                tool_call: required_update_field(tool_call, "tool_call", &update_type).map_err(de::Error::custom)?,
            },
            "tool_call_end" => SessionUpdate::ToolCallEnd {
                tool_call_id: required_update_field(tool_call_id, "tool_call_id", &update_type)
                    .map_err(de::Error::custom)?,
                result: required_present_value(result, "result", &update_type).map_err(de::Error::custom)?,
            },
            "turn_complete" => SessionUpdate::TurnComplete {
                status: required_update_field(status, "status", &update_type).map_err(de::Error::custom)?,
            },
            "error" => SessionUpdate::Error {
                code: required_update_field(code, "code", &update_type).map_err(de::Error::custom)?,
                message: required_update_field(message, "message", &update_type).map_err(de::Error::custom)?,
            },
            "server_request" => SessionUpdate::ServerRequest {
                request: required_update_field(request, "request", &update_type).map_err(de::Error::custom)?,
            },
            _ => return Err(de::Error::unknown_variant(&update_type, SESSION_UPDATE_TYPES)),
        };

        Ok(Self { session_id, turn_id, update })
    }
}

/// Direct wire representation used to avoid buffering the notification map
/// for the flattened, tagged [`SessionUpdate`] payload.
#[derive(Debug, Deserialize)]
struct SessionUpdateNotificationWire {
    session_id: String,
    turn_id: String,
    update_type: String,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    tool_call: Option<ToolCallRecord>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    result: Present<Value>,
    #[serde(default)]
    status: Option<TurnStatus>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    request: Option<ToolExecutionRequest>,
}

/// Preserves the distinction between a missing field and an explicit JSON
/// `null` for required values such as a tool result.
#[derive(Debug)]
struct Present<T> {
    value: Option<T>,
    present: bool,
}

impl<T> Default for Present<T> {
    fn default() -> Self {
        Self { value: None, present: false }
    }
}

impl<'de, T> Deserialize<'de> for Present<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self {
            value: Option::<T>::deserialize(deserializer)?,
            present: true,
        })
    }
}

fn required_present_value(field: Present<Value>, field_name: &str, update_type: &str) -> Result<Value, String> {
    if field.present {
        Ok(field.value.unwrap_or(Value::Null))
    } else {
        Err(format!("ACP update {update_type:?} is missing {field_name:?}"))
    }
}

fn required_update_field<T>(value: Option<T>, field: &str, update_type: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("ACP update {update_type:?} is missing {field:?}"))
}

const SESSION_UPDATE_TYPES: &[&str] = &[
    "message_delta",
    "tool_call_start",
    "tool_call_end",
    "turn_complete",
    "error",
    "server_request",
];

/// Session update types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "update_type", rename_all = "snake_case")]
pub enum SessionUpdate {
    /// Text delta (streaming response)
    MessageDelta {
        /// Incremental text content
        delta: String,
    },

    /// Tool call started
    ToolCallStart {
        /// Tool call details
        tool_call: ToolCallRecord,
    },

    /// Tool call completed
    ToolCallEnd {
        /// Tool call ID
        tool_call_id: String,
        /// Tool result
        result: Value,
    },

    /// Turn completed
    TurnComplete {
        /// Final status
        status: TurnStatus,
    },

    /// Error occurred
    Error {
        /// Error code
        code: String,
        /// Error message
        message: String,
    },

    /// Server requests the client to execute a tool (bidirectional ACP protocol).
    ///
    /// Arrives via a `server/request` SSE event. After executing the tool,
    /// send the result back with `AcpClientV2::session_tool_response`.
    ServerRequest {
        /// The tool execution request from the agent.
        request: ToolExecutionRequest,
    },
}

// ============================================================================
// Supporting Types
// ============================================================================

/// Workspace context for session initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceContext {
    /// Workspace root path
    root_path: String,

    /// Workspace name
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    /// Active file paths
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    active_files: Vec<String>,
}

/// Model preferences for session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreferences {
    /// Preferred model ID
    #[serde(skip_serializing_if = "Option::is_none")]
    model_id: Option<String>,

    /// Temperature setting
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,

    /// Max tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Record of a tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// Unique tool call ID
    id: String,

    /// Tool name
    name: String,

    /// Tool arguments
    arguments: Value,

    /// Tool result (if completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,

    /// Timestamp
    timestamp: String,
}

/// Server-to-client tool execution request (arrives via `server/request` SSE event).
///
/// When the ACP agent needs the client to run a tool on its behalf it emits a
/// `server/request` SSE event containing this payload. The client must execute
/// the tool and reply with [`ToolExecutionResult`] via `client/response`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionRequest {
    /// Unique request identifier used to correlate the response.
    request_id: String,
    /// The tool call the agent wants the client to execute.
    tool_call: ToolCallRecord,
}

/// Result of a client-side tool execution, sent back via `client/response`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    /// Must match the `request_id` from [`ToolExecutionRequest`].
    pub(crate) request_id: String,
    /// ID of the tool call that was executed.
    pub(crate) tool_call_id: String,
    /// Tool output (structured or text).
    pub(crate) output: Value,
    /// Whether execution succeeded.
    pub(crate) success: bool,
    /// Error message when `success` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

/// Wrapper for a `server/request` SSE event notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerRequestNotification {
    /// Session this request belongs to.
    pub(crate) session_id: String,
    /// The tool execution request.
    pub(crate) request: ToolExecutionRequest,
}

/// A single turn in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    /// Turn ID
    turn_id: String,

    /// User prompt
    prompt: Vec<PromptContent>,

    /// Agent response
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<String>,

    /// Tool calls made during this turn
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ToolCallRecord>,

    /// Turn timestamp
    timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_session_new_params() {
        let params = SessionNewParams::default();
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json, json!({}));
    }

    #[test]
    fn test_prompt_content_text() {
        let content = PromptContent::text("Hello, world!");
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "Hello, world!");
    }

    #[test]
    fn test_session_update_message_delta() {
        let update = SessionUpdate::MessageDelta { delta: "Hello".to_string() };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["update_type"], "message_delta");
        assert_eq!(json["delta"], "Hello");
    }

    #[test]
    fn session_update_notification_deserializes_each_update_shape() {
        let tool_call = json!({
            "id": "tc-1",
            "name": "code_search",
            "arguments": {"query": "fn main"},
            "timestamp": "2025-01-01T00:00:00Z"
        });
        let request = json!({
            "request_id": "req-1",
            "tool_call": tool_call.clone()
        });
        let cases = [
            (json!({"session_id":"s","turn_id":"t","update_type":"message_delta","delta":"hi"}), "message"),
            (
                json!({"session_id":"s","turn_id":"t","update_type":"tool_call_start","tool_call":tool_call}),
                "start",
            ),
            (
                json!({"session_id":"s","turn_id":"t","update_type":"tool_call_end","tool_call_id":"tc-1","result":null}),
                "end",
            ),
            (
                json!({"session_id":"s","turn_id":"t","update_type":"turn_complete","status":"completed"}),
                "complete",
            ),
            (
                json!({"session_id":"s","turn_id":"t","update_type":"error","code":"bad_request","message":"nope"}),
                "error",
            ),
            (json!({"session_id":"s","turn_id":"t","update_type":"server_request","request":request}), "request"),
        ];

        for (payload, expected) in cases {
            let notification: SessionUpdateNotification =
                serde_json::from_value(payload).expect("valid session update notification");
            let actual = match notification.update {
                SessionUpdate::MessageDelta { .. } => "message",
                SessionUpdate::ToolCallStart { .. } => "start",
                SessionUpdate::ToolCallEnd { result, .. } if result.is_null() => "end",
                SessionUpdate::TurnComplete { .. } => "complete",
                SessionUpdate::Error { .. } => "error",
                SessionUpdate::ServerRequest { .. } => "request",
                _ => "other",
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn session_update_notification_rejects_missing_payload() {
        let missing_delta = json!({
            "session_id": "s",
            "turn_id": "t",
            "update_type": "message_delta"
        });
        assert!(serde_json::from_value::<SessionUpdateNotification>(missing_delta).is_err());

        let unknown = json!({
            "session_id": "s",
            "turn_id": "t",
            "update_type": "future_update"
        });
        assert!(serde_json::from_value::<SessionUpdateNotification>(unknown).is_err());
    }

    #[test]
    fn test_session_state_transitions() {
        let mut session = AcpSession::new("test-session");
        assert_eq!(session.state, SessionState::Created);

        session.set_state(SessionState::Active);
        assert_eq!(session.state, SessionState::Active);
        assert!(session.last_activity_at.is_some());
    }

    #[test]
    fn server_request_update_serializes_correctly() {
        let tool_call = ToolCallRecord {
            id: "tc-1".to_string(),
            name: "code_search".to_string(),
            arguments: json!({"query": "fn main"}),
            result: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
        };
        let request = ToolExecutionRequest { request_id: "req-1".to_string(), tool_call };
        let update = SessionUpdate::ServerRequest { request };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["update_type"], "server_request");
        assert_eq!(json["request"]["request_id"], "req-1");
    }

    #[test]
    fn tool_execution_result_success_serializes() {
        let result = ToolExecutionResult {
            request_id: "req-1".to_string(),
            tool_call_id: "tc-1".to_string(),
            output: json!({"matches": []}),
            success: true,
            error: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["success"], true);
        assert!(json.get("error").is_none());
    }

    #[test]
    fn tool_execution_result_failure_includes_error() {
        let result = ToolExecutionResult {
            request_id: "req-1".to_string(),
            tool_call_id: "tc-1".to_string(),
            output: Value::Null,
            success: false,
            error: Some("permission denied".to_string()),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "permission denied");
    }
}
