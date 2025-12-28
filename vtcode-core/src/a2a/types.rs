//! A2A Protocol core data types
//!
//! Implements the core data structures as defined in the A2A specification:
//! - Task and TaskStatus for task lifecycle management
//! - Message and Part types for communication
//! - Artifact for task outputs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Task State & Status
// ============================================================================

/// Task lifecycle states as defined in the A2A specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    /// Task has been submitted but not yet started
    #[default]
    Submitted,
    /// Task is actively being processed
    Working,
    /// Task requires additional input from the user
    InputRequired,
    /// Task completed successfully
    Completed,
    /// Task failed with an error
    Failed,
    /// Task was canceled by request
    Canceled,
    /// Task was rejected by the agent
    Rejected,
    /// Task requires authentication
    AuthRequired,
    /// Task state is unknown
    Unknown,
}

impl TaskState {
    /// Check if this is a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskState::Completed
                | TaskState::Failed
                | TaskState::Canceled
                | TaskState::Rejected
        )
    }

    /// Check if the task can be canceled from this state
    pub fn is_cancelable(&self) -> bool {
        matches!(
            self,
            TaskState::Submitted | TaskState::Working | TaskState::InputRequired
        )
    }
}

/// Task status with state, optional message, and timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    /// Current lifecycle state
    pub state: TaskState,
    /// Optional message from the agent providing status details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    /// ISO-8601 timestamp of the status update
    pub timestamp: DateTime<Utc>,
}

impl TaskStatus {
    /// Create a new task status
    pub fn new(state: TaskState) -> Self {
        Self {
            state,
            message: None,
            timestamp: Utc::now(),
        }
    }

    /// Create a new task status with a message
    pub fn with_message(state: TaskState, message: Message) -> Self {
        Self {
            state,
            message: Some(message),
            timestamp: Utc::now(),
        }
    }
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::new(TaskState::Submitted)
    }
}

// ============================================================================
// Message & Parts
// ============================================================================

/// Role of the message sender
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// Message from the user/client
    User,
    /// Message from the agent
    Agent,
}

/// A single unit of communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Sender's role
    pub role: MessageRole,
    /// Content of the message
    pub parts: Vec<Part>,
    /// Unique message identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Associated task ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Conversation context ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    /// List of prior task IDs for context
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reference_task_ids: Vec<String>,
    /// Custom metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl Message {
    /// Create a new message
    pub fn new(role: MessageRole, parts: Vec<Part>) -> Self {
        Self {
            role,
            parts,
            message_id: None,
            task_id: None,
            context_id: None,
            reference_task_ids: Vec::new(),
            metadata: None,
        }
    }

    /// Create a text message from the agent
    pub fn agent_text(text: impl Into<String>) -> Self {
        Self::new(MessageRole::Agent, vec![Part::text(text)])
    }

    /// Create a text message from the user
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::new(MessageRole::User, vec![Part::text(text)])
    }

    /// Set the message ID
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.message_id = Some(id.into());
        self
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

/// Content part types: text, file, or structured data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Part {
    /// Plain text content
    #[serde(rename = "text")]
    Text {
        /// The text content
        text: String,
    },
    /// File content (URI or inline bytes)
    #[serde(rename = "file")]
    File {
        /// File content details
        file: FileContent,
    },
    /// Structured JSON data
    #[serde(rename = "data")]
    Data {
        /// The structured data
        data: serde_json::Value,
    },
}

impl Part {
    /// Create a text part
    pub fn text(text: impl Into<String>) -> Self {
        Part::Text { text: text.into() }
    }

    /// Create a file part from URI
    pub fn file_uri(uri: impl Into<String>, mime_type: Option<String>) -> Self {
        Part::File {
            file: FileContent::Uri {
                uri: uri.into(),
                mime_type,
            },
        }
    }

    /// Create a file part from inline bytes
    pub fn file_bytes(bytes: Vec<u8>, mime_type: Option<String>, name: Option<String>) -> Self {
        Part::File {
            file: FileContent::Bytes {
                bytes: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
                mime_type,
                name,
            },
        }
    }

    /// Create a data part
    pub fn data(data: serde_json::Value) -> Self {
        Part::Data { data }
    }

    /// Get the text content if this is a text part
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Part::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// File content representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileContent {
    /// File referenced by URI
    Uri {
        /// The file URI
        uri: String,
        /// Optional MIME type
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    /// File with inline base64-encoded bytes
    Bytes {
        /// Base64-encoded file content
        bytes: String,
        /// Optional MIME type
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        /// Optional file name
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

// ============================================================================
// Artifact
// ============================================================================

/// A tangible output generated by a task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// Unique artifact identifier
    pub id: String,
    /// Human-readable name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Description of the artifact
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Content parts
    pub parts: Vec<Part>,
    /// Index for ordering multiple artifacts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    /// Custom metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl Artifact {
    /// Create a new artifact with text content
    pub fn text(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
            description: None,
            parts: vec![Part::text(text)],
            index: None,
            metadata: None,
        }
    }

    /// Create a new artifact with file content
    pub fn file(id: impl Into<String>, file: FileContent) -> Self {
        Self {
            id: id.into(),
            name: None,
            description: None,
            parts: vec![Part::File { file }],
            index: None,
            metadata: None,
        }
    }

    /// Add a name to the artifact
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Add a description to the artifact
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

// ============================================================================
// Task
// ============================================================================

/// Represents a stateful unit of work
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Unique task identifier
    pub id: String,
    /// Conversational context identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    /// Current status
    pub status: TaskStatus,
    /// Outputs produced by the agent
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub artifacts: Vec<Artifact>,
    /// Conversation history (if enabled)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub history: Vec<Message>,
    /// Custom metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    /// Type discriminator
    #[serde(default = "default_task_kind")]
    pub kind: String,
}

fn default_task_kind() -> String {
    "task".to_string()
}

impl Task {
    /// Create a new task with a generated ID
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            context_id: None,
            status: TaskStatus::default(),
            artifacts: Vec::new(),
            history: Vec::new(),
            metadata: None,
            kind: "task".to_string(),
        }
    }

    /// Create a new task with a specific ID
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            context_id: None,
            status: TaskStatus::default(),
            artifacts: Vec::new(),
            history: Vec::new(),
            metadata: None,
            kind: "task".to_string(),
        }
    }

    /// Set the context ID
    pub fn with_context_id(mut self, context_id: impl Into<String>) -> Self {
        self.context_id = Some(context_id.into());
        self
    }

    /// Get the current state
    pub fn state(&self) -> TaskState {
        self.status.state
    }

    /// Check if the task is in a terminal state
    pub fn is_terminal(&self) -> bool {
        self.status.state.is_terminal()
    }

    /// Check if the task can be canceled
    pub fn is_cancelable(&self) -> bool {
        self.status.state.is_cancelable()
    }

    /// Update the task status
    pub fn update_status(&mut self, state: TaskState, message: Option<Message>) {
        self.status = match message {
            Some(msg) => TaskStatus::with_message(state, msg),
            None => TaskStatus::new(state),
        };
    }

    /// Add an artifact to the task
    pub fn add_artifact(&mut self, artifact: Artifact) {
        self.artifacts.push(artifact);
    }

    /// Add a message to the history
    pub fn add_message(&mut self, message: Message) {
        self.history.push(message);
    }
}

impl Default for Task {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_state_terminal() {
        assert!(!TaskState::Submitted.is_terminal());
        assert!(!TaskState::Working.is_terminal());
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Canceled.is_terminal());
    }

    #[test]
    fn test_task_state_cancelable() {
        assert!(TaskState::Submitted.is_cancelable());
        assert!(TaskState::Working.is_cancelable());
        assert!(TaskState::InputRequired.is_cancelable());
        assert!(!TaskState::Completed.is_cancelable());
        assert!(!TaskState::Failed.is_cancelable());
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::agent_text("Hello, world!");
        assert_eq!(msg.role, MessageRole::Agent);
        assert_eq!(msg.parts.len(), 1);
        assert_eq!(msg.parts[0].as_text(), Some("Hello, world!"));
    }

    #[test]
    fn test_task_lifecycle() {
        let mut task = Task::new();
        assert_eq!(task.state(), TaskState::Submitted);
        assert!(!task.is_terminal());
        assert!(task.is_cancelable());

        task.update_status(TaskState::Working, None);
        assert_eq!(task.state(), TaskState::Working);

        task.update_status(
            TaskState::Completed,
            Some(Message::agent_text("Task completed")),
        );
        assert!(task.is_terminal());
        assert!(!task.is_cancelable());
    }

    #[test]
    fn test_part_serialization() {
        let text_part = Part::text("Hello");
        let json = serde_json::to_string(&text_part).expect("serialize");
        assert!(json.contains("\"type\":\"text\""));

        let data_part = Part::data(serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&data_part).expect("serialize");
        assert!(json.contains("\"type\":\"data\""));
    }

    #[test]
    fn test_artifact_creation() {
        let artifact = Artifact::text("art-1", "Generated content")
            .with_name("output.txt")
            .with_description("The generated output file");

        assert_eq!(artifact.id, "art-1");
        assert_eq!(artifact.name, Some("output.txt".to_string()));
        assert_eq!(artifact.parts.len(), 1);
    }
}
