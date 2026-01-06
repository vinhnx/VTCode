//! ACP session types and lifecycle management
//!
//! This module implements the session lifecycle as defined by ACP:
//! - Session creation (session/new)
//! - Session loading (session/load)
//! - Prompt handling (session/prompt)
//! - Session updates (session/update notifications)
//!
//! Reference: https://agentclientprotocol.com/llms.txt

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Session state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Session created but not yet active
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

impl Default for SessionState {
    fn default() -> Self {
        Self::Created
    }
}

/// ACP Session representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSession {
    /// Unique session identifier
    pub session_id: String,

    /// Current session state
    pub state: SessionState,

    /// Session creation timestamp (ISO 8601)
    pub created_at: String,

    /// Last activity timestamp (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<String>,

    /// Session metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,

    /// Turn counter for prompt/response cycles
    #[serde(default)]
    pub turn_count: u32,
}

impl AcpSession {
    /// Create a new session with the given ID
    pub fn new(session_id: impl Into<String>) -> Self {
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
    pub fn set_state(&mut self, state: SessionState) {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNewParams {
    /// Optional session metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,

    /// Optional workspace context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceContext>,

    /// Optional model preferences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
}

impl Default for SessionNewParams {
    fn default() -> Self {
        Self {
            metadata: HashMap::new(),
            workspace: None,
            model_preferences: None,
        }
    }
}

/// Result of session/new method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNewResult {
    /// The created session ID
    pub session_id: String,

    /// Initial session state
    #[serde(default)]
    pub state: SessionState,
}

// ============================================================================
// Session/Load Request/Response
// ============================================================================

/// Parameters for session/load method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoadParams {
    /// Session ID to load
    pub session_id: String,
}

/// Result of session/load method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoadResult {
    /// The loaded session
    pub session: AcpSession,

    /// Conversation history (if available)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<ConversationTurn>,
}

// ============================================================================
// Session/Prompt Request/Response
// ============================================================================

/// Parameters for session/prompt method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptParams {
    /// Session ID
    pub session_id: String,

    /// Prompt content (can be text, images, etc.)
    pub content: Vec<PromptContent>,

    /// Optional turn-specific metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
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
    pub fn text(text: impl Into<String>) -> Self {
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
    pub turn_id: String,

    /// Final response content (may be streamed via notifications first)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,

    /// Tool calls made during this turn
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,

    /// Turn completion status
    pub status: TurnStatus,
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
    pub session_id: String,

    /// Tool call requiring permission
    pub tool_call: ToolCallRecord,

    /// Available permission options
    pub options: Vec<PermissionOption>,
}

/// A permission option presented to the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionOption {
    /// Option ID
    pub id: String,

    /// Display label
    pub label: String,

    /// Detailed description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    pub session_id: String,

    /// Optional turn ID to cancel (if not provided, cancels current turn)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

// ============================================================================
// Session/Update Notification (Streaming)
// ============================================================================

/// Session update notification payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdateNotification {
    /// Session ID
    pub session_id: String,

    /// Turn ID this update belongs to
    pub turn_id: String,

    /// Update type
    #[serde(flatten)]
    pub update: SessionUpdate,
}

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
}

// ============================================================================
// Supporting Types
// ============================================================================

/// Workspace context for session initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceContext {
    /// Workspace root path
    pub root_path: String,

    /// Workspace name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Active file paths
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_files: Vec<String>,
}

/// Model preferences for session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreferences {
    /// Preferred model ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,

    /// Temperature setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Max tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Record of a tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// Unique tool call ID
    pub id: String,

    /// Tool name
    pub name: String,

    /// Tool arguments
    pub arguments: Value,

    /// Tool result (if completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Timestamp
    pub timestamp: String,
}

/// A single turn in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    /// Turn ID
    pub turn_id: String,

    /// User prompt
    pub prompt: Vec<PromptContent>,

    /// Agent response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,

    /// Tool calls made during this turn
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,

    /// Turn timestamp
    pub timestamp: String,
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
        let update = SessionUpdate::MessageDelta {
            delta: "Hello".to_string(),
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["update_type"], "message_delta");
        assert_eq!(json["delta"], "Hello");
    }

    #[test]
    fn test_session_state_transitions() {
        let mut session = AcpSession::new("test-session");
        assert_eq!(session.state, SessionState::Created);

        session.set_state(SessionState::Active);
        assert_eq!(session.state, SessionState::Active);
        assert!(session.last_activity_at.is_some());
    }
}
