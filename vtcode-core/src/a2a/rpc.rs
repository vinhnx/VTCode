//! JSON-RPC 2.0 structures for A2A Protocol
//!
//! Implements the JSON-RPC 2.0 request/response format used by the A2A protocol,
//! along with A2A-specific RPC method constants.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::A2aErrorCode;

// ============================================================================
// A2A RPC Method Constants
// ============================================================================

/// Send a message to initiate or continue a task
pub const METHOD_MESSAGE_SEND: &str = "message/send";

/// Send a message and subscribe to real-time updates via SSE
pub const METHOD_MESSAGE_STREAM: &str = "message/stream";

/// Retrieve the current state of a task
pub const METHOD_TASKS_GET: &str = "tasks/get";

/// Retrieve a list of tasks with optional filtering
pub const METHOD_TASKS_LIST: &str = "tasks/list";

/// Request cancellation of a running task
pub const METHOD_TASKS_CANCEL: &str = "tasks/cancel";

/// Set push notification configuration for a task
pub const METHOD_TASKS_PUSH_CONFIG_SET: &str = "tasks/pushNotificationConfig/set";

/// Get push notification configuration for a task
pub const METHOD_TASKS_PUSH_CONFIG_GET: &str = "tasks/pushNotificationConfig/get";

/// Resubscribe to task updates after connection interruption
pub const METHOD_TASKS_RESUBSCRIBE: &str = "tasks/resubscribe";

/// Get authenticated extended agent card
pub const METHOD_AGENT_GET_EXTENDED_CARD: &str = "agent/getAuthenticatedExtendedCard";

// ============================================================================
// JSON-RPC 2.0 Structures
// ============================================================================

/// JSON-RPC 2.0 version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol version (always "2.0")
    pub jsonrpc: String,
    /// Method name
    pub method: String,
    /// Method parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Request ID (can be string, number, or null)
    pub id: Value,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request
    pub fn new(method: impl Into<String>, params: Option<Value>, id: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
            id,
        }
    }

    /// Create a request with a string ID
    pub fn with_string_id(
        method: impl Into<String>,
        params: Option<Value>,
        id: impl Into<String>,
    ) -> Self {
        Self::new(method, params, Value::String(id.into()))
    }

    /// Create a request with a numeric ID
    pub fn with_numeric_id(method: impl Into<String>, params: Option<Value>, id: i64) -> Self {
        Self::new(method, params, Value::Number(id.into()))
    }

    /// Create a message/send request
    pub fn message_send(params: MessageSendParams, id: Value) -> Self {
        Self::new(
            METHOD_MESSAGE_SEND,
            Some(serde_json::to_value(params).unwrap_or_default()),
            id,
        )
    }

    /// Create a tasks/get request
    pub fn tasks_get(task_id: impl Into<String>, id: Value) -> Self {
        Self::new(
            METHOD_TASKS_GET,
            Some(serde_json::json!({ "id": task_id.into() })),
            id,
        )
    }
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Protocol version (always "2.0")
    pub jsonrpc: String,
    /// Result (present on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error (present on failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// Request ID (matches the request)
    pub id: Value,
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(result: Value, id: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(error: JsonRpcError, id: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }

    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        self.result.is_some() && self.error.is_none()
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// Create a new error
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create an error with additional data
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    /// Create an error from A2aErrorCode
    pub fn from_code(code: A2aErrorCode, message: impl Into<String>) -> Self {
        Self::new(code.into(), message)
    }

    /// Create a parse error
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::from_code(A2aErrorCode::JsonParseError, message)
    }

    /// Create an invalid request error
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::from_code(A2aErrorCode::InvalidRequest, message)
    }

    /// Create a method not found error
    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::from_code(
            A2aErrorCode::MethodNotFound,
            format!("Method not found: {}", method.into()),
        )
    }

    /// Create an invalid params error
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::from_code(A2aErrorCode::InvalidParams, message)
    }

    /// Create an internal error
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::from_code(A2aErrorCode::InternalError, message)
    }

    /// Create a task not found error
    pub fn task_not_found(task_id: impl Into<String>) -> Self {
        Self::from_code(
            A2aErrorCode::TaskNotFound,
            format!("Task not found: {}", task_id.into()),
        )
    }
}

// ============================================================================
// Request Parameters
// ============================================================================

/// Parameters for message/send and message/stream methods
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSendParams {
    /// Message to send
    pub message: super::types::Message,
    /// Optional task ID (to continue existing task)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Optional context ID (for conversational context)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    /// Optional configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<MessageConfiguration>,
}

impl MessageSendParams {
    /// Create new message send params
    pub fn new(message: super::types::Message) -> Self {
        Self {
            message,
            task_id: None,
            context_id: None,
            configuration: None,
        }
    }

    /// Set the task ID
    pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Set the context ID
    pub fn with_context_id(mut self, context_id: impl Into<String>) -> Self {
        self.context_id = Some(context_id.into());
        self
    }
}

/// Configuration for message sending
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageConfiguration {
    /// Accepted input MIME types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_input_modes: Option<Vec<String>>,
    /// Accepted output MIME types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_output_modes: Option<Vec<String>>,
    /// History length to include
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
    /// Push notification configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_notification_config: Option<PushNotificationConfig>,
}

/// Push notification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotificationConfig {
    /// Webhook URL to receive notifications
    pub url: String,
    /// Optional authentication header value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<String>,
}

/// Parameters for tasks/get method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQueryParams {
    /// Task ID
    pub id: String,
    /// Optional history length
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
}

/// Parameters for tasks/list method
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksParams {
    /// Filter by context ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    /// Filter by status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<super::types::TaskState>,
    /// Page size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    /// Page token for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
    /// History length to include
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
    /// Filter by last updated timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated_after: Option<String>,
    /// Include artifacts in response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_artifacts: Option<bool>,
    /// Filter by metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Result for tasks/list method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResult {
    /// List of tasks
    pub tasks: Vec<super::types::Task>,
    /// Total number of tasks matching the query
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u32>,
    /// Page size used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    /// Token for next page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Parameters for tasks/cancel method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdParams {
    /// Task ID
    pub id: String,
}

/// Configuration for push notification delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPushNotificationConfig {
    /// Task ID to configure
    pub task_id: String,
    /// Webhook URL for notifications
    pub url: String,
    /// Optional authentication header value (Bearer token, API key, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<String>,
}

// ============================================================================
// Streaming Events
// ============================================================================

// ============================================================================
// Streaming Events (per A2A Specification)
// ============================================================================

/// Base wrapper for streaming message response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendStreamingMessageResponse {
    /// Event data (one of MessageEvent, TaskStatusUpdateEvent, TaskArtifactUpdateEvent)
    #[serde(flatten)]
    pub event: StreamingEvent,
}

/// Streaming event types (discriminated by 'type' field)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StreamingEvent {
    /// Message event from agent
    #[serde(rename = "message")]
    Message {
        /// The message content
        message: super::types::Message,
        /// Context identifier the message is associated with
        #[serde(skip_serializing_if = "Option::is_none")]
        context_id: Option<String>,
        /// Type discriminator
        #[serde(default = "default_message_kind")]
        kind: String,
        /// True if this is the final message for the task
        #[serde(default)]
        r#final: bool,
    },
    /// Task status update event
    #[serde(rename = "task-status")]
    TaskStatus {
        /// Task identifier
        task_id: String,
        /// Context identifier the task is associated with
        #[serde(skip_serializing_if = "Option::is_none")]
        context_id: Option<String>,
        /// The new status
        status: super::types::TaskStatus,
        /// Type discriminator
        #[serde(default = "default_status_kind")]
        kind: String,
        /// True if this is the terminal update for the task
        #[serde(default)]
        r#final: bool,
    },
    /// Task artifact update event
    #[serde(rename = "task-artifact")]
    TaskArtifact {
        /// Task identifier
        task_id: String,
        /// The artifact data
        artifact: super::types::Artifact,
        /// If true, append parts to existing artifact; if false, replace
        #[serde(default)]
        append: bool,
        /// If true, indicates this is the final update for the artifact
        #[serde(default)]
        last_chunk: bool,
        /// Usually false for artifacts; can signal end concurrently with status
        #[serde(default)]
        r#final: bool,
    },
}

fn default_message_kind() -> String {
    "streaming-response".to_string()
}

fn default_status_kind() -> String {
    "status-update".to_string()
}

impl StreamingEvent {
    /// Check if this is a final event
    pub fn is_final(&self) -> bool {
        match self {
            StreamingEvent::Message { r#final, .. } => *r#final,
            StreamingEvent::TaskStatus { r#final, .. } => *r#final,
            StreamingEvent::TaskArtifact { r#final, .. } => *r#final,
        }
    }

    /// Get the task ID if present
    pub fn task_id(&self) -> Option<&str> {
        match self {
            StreamingEvent::Message { .. } => None,
            StreamingEvent::TaskStatus { task_id, .. } => Some(task_id),
            StreamingEvent::TaskArtifact { task_id, .. } => Some(task_id),
        }
    }

    /// Get the context ID if present
    pub fn context_id(&self) -> Option<&str> {
        match self {
            StreamingEvent::Message { context_id, .. } => context_id.as_deref(),
            StreamingEvent::TaskStatus { context_id, .. } => context_id.as_deref(),
            StreamingEvent::TaskArtifact { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_creation() {
        let request = JsonRpcRequest::with_string_id(
            METHOD_MESSAGE_SEND,
            Some(serde_json::json!({"message": {}})),
            "req-1",
        );
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "message/send");
        assert_eq!(request.id, Value::String("req-1".to_string()));
    }

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(
            serde_json::json!({"status": "ok"}),
            Value::String("req-1".to_string()),
        );
        assert!(response.is_success());
        assert!(!response.is_error());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let error = JsonRpcError::task_not_found("task-123");
        let response = JsonRpcResponse::error(error, Value::String("req-1".to_string()));
        assert!(!response.is_success());
        assert!(response.is_error());
    }

    #[test]
    fn test_error_code_serialization() {
        let error = JsonRpcError::from_code(A2aErrorCode::TaskNotFound, "Task not found");
        assert_eq!(error.code, -32001);
    }

    #[test]
    fn test_streaming_event_message() {
        let event = StreamingEvent::Message {
            message: super::super::types::Message::agent_text("Response"),
            context_id: Some("ctx-1".to_string()),
            kind: "streaming-response".to_string(),
            r#final: false,
        };
        assert!(!event.is_final());
        assert_eq!(event.context_id(), Some("ctx-1"));
    }

    #[test]
    fn test_streaming_event_task_status() {
        let event = StreamingEvent::TaskStatus {
            task_id: "task-1".to_string(),
            context_id: None,
            status: super::super::types::TaskStatus::new(super::super::types::TaskState::Completed),
            kind: "status-update".to_string(),
            r#final: true,
        };
        assert!(event.is_final());
        assert_eq!(event.task_id(), Some("task-1"));
    }

    #[test]
    fn test_streaming_event_artifact() {
        let artifact = super::super::types::Artifact::text("art-1", "Output");
        let event = StreamingEvent::TaskArtifact {
            task_id: "task-1".to_string(),
            artifact,
            append: false,
            last_chunk: true,
            r#final: false,
        };
        assert!(!event.is_final());
        assert_eq!(event.task_id(), Some("task-1"));
    }

    #[test]
    fn test_send_streaming_message_response_serialization() {
        let msg = super::super::types::Message::agent_text("Hello");
        let response = SendStreamingMessageResponse {
            event: StreamingEvent::Message {
                message: msg,
                context_id: Some("ctx-1".to_string()),
                kind: "streaming-response".to_string(),
                r#final: false,
            },
        };

        let json = serde_json::to_string(&response).expect("serialize");
        assert!(json.contains("streaming-response"));
        assert!(json.contains("message"));

        let deserialized: SendStreamingMessageResponse =
            serde_json::from_str(&json).expect("deserialize");
        match deserialized.event {
            StreamingEvent::Message { ref kind, .. } => {
                assert_eq!(kind, "streaming-response");
            }
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn test_task_push_notification_config() {
        let config = TaskPushNotificationConfig {
            task_id: "task-1".to_string(),
            url: "https://example.com/webhook".to_string(),
            authentication: Some("Bearer token123".to_string()),
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: TaskPushNotificationConfig =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.task_id, "task-1");
        assert_eq!(deserialized.url, "https://example.com/webhook");
        assert!(deserialized.authentication.is_some());
    }
}
