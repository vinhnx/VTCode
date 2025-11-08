//! ACP message types and serialization

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Core ACP message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpMessage {
    /// Unique message ID
    pub id: String,

    /// Message type (request, response, etc.)
    #[serde(rename = "type")]
    pub message_type: MessageType,

    /// Sender agent ID
    pub sender: String,

    /// Recipient agent ID
    pub recipient: String,

    /// Message content
    pub content: MessageContent,

    /// Timestamp (ISO 8601)
    pub timestamp: String,

    /// Optional correlation ID for request/response pairs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

/// Message type enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Request,
    Response,
    Error,
    Notification,
}

/// Message content payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Request to execute a tool or action
    Request(AcpRequest),

    /// Response with results
    Response(AcpResponse),

    /// Error response
    Error(ErrorPayload),

    /// Generic notification
    Notification(NotificationPayload),
}

/// ACP request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRequest {
    /// Action/tool name to execute
    pub action: String,

    /// Arguments for the action (any JSON-serializable data)
    pub args: Value,

    /// Optional timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,

    /// Whether to await response synchronously
    #[serde(default)]
    pub sync: bool,
}

/// ACP response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpResponse {
    /// Execution status
    pub status: ResponseStatus,

    /// Result data (on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Error details (on failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDetails>,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

/// Response status enum
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResponseStatus {
    Success,
    Failed,
    Timeout,
    Partial,
}

/// Error response payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Error code (ACP standard or custom)
    pub code: String,

    /// Human-readable error message
    pub message: String,

    /// Additional error details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Error details in response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetails {
    /// Error code
    pub code: String,

    /// Error message
    pub message: String,

    /// Additional context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

/// Notification payload for one-way messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPayload {
    /// Notification type
    pub event: String,

    /// Event-specific data
    pub data: Value,
}

impl AcpMessage {
    /// Create a new ACP request message
    pub fn request(sender: String, recipient: String, action: String, args: Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            message_type: MessageType::Request,
            sender,
            recipient,
            content: MessageContent::Request(AcpRequest {
                action,
                args,
                timeout_secs: None,
                sync: true,
            }),
            timestamp: chrono::Utc::now().to_rfc3339(),
            correlation_id: None,
        }
    }

    /// Create a new ACP response message
    pub fn response(
        sender: String,
        recipient: String,
        result: Value,
        correlation_id: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            message_type: MessageType::Response,
            sender,
            recipient,
            content: MessageContent::Response(AcpResponse {
                status: ResponseStatus::Success,
                result: Some(result),
                error: None,
                execution_time_ms: 0,
            }),
            timestamp: chrono::Utc::now().to_rfc3339(),
            correlation_id: Some(correlation_id),
        }
    }

    /// Create an error response
    pub fn error_response(
        sender: String,
        recipient: String,
        code: String,
        message: String,
        correlation_id: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            message_type: MessageType::Error,
            sender,
            recipient,
            content: MessageContent::Error(ErrorPayload {
                code,
                message,
                details: None,
            }),
            timestamp: chrono::Utc::now().to_rfc3339(),
            correlation_id: Some(correlation_id),
        }
    }

    /// Convert to JSON for transmission
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Parse from JSON
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_message_creation() {
        let msg = AcpMessage::request(
            "agent-1".to_string(),
            "agent-2".to_string(),
            "execute_tool".to_string(),
            json!({"tool": "bash", "command": "ls"}),
        );

        assert_eq!(msg.message_type, MessageType::Request);
        assert_eq!(msg.sender, "agent-1");
        assert_eq!(msg.recipient, "agent-2");
    }

    #[test]
    fn test_message_serialization() {
        let msg = AcpMessage::request(
            "agent-1".to_string(),
            "agent-2".to_string(),
            "test".to_string(),
            json!({}),
        );

        let json = msg.to_json().unwrap();
        let restored = AcpMessage::from_json(&json).unwrap();

        assert_eq!(msg.id, restored.id);
        assert_eq!(msg.sender, restored.sender);
    }
}
