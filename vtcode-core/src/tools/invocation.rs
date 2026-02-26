//! Unified tool invocation tracking
//!
//! Provides a unique `ToolInvocationId` that flows through the entire tool execution
//! pipeline, enabling correlation of logs, metrics, and state across different tracking
//! mechanisms (execution_context, execution_tracker, tool_ledger, execution_history).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::time::Instant;
use uuid::Uuid;

/// Unique identifier for a tool invocation.
///
/// UUID-based for global uniqueness across sessions and processes.
/// Implements Display for logging and correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolInvocationId(Uuid);

impl ToolInvocationId {
    /// Create a new unique invocation ID.
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID.
    #[inline]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Parse from a string representation.
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(Self)
    }

    /// Get the underlying UUID.
    #[inline]
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Convert to a hyphenated string (standard UUID format).
    #[inline]
    pub fn to_string_hyphenated(&self) -> String {
        self.0.hyphenated().to_string()
    }

    /// Convert to a short 8-character prefix for compact logging.
    #[inline]
    pub fn short(&self) -> String {
        self.0.hyphenated().to_string()[..8].to_string()
    }
}

impl Default for ToolInvocationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ToolInvocationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl From<Uuid> for ToolInvocationId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// Complete context for a single tool invocation.
///
/// Tracks all metadata needed for correlation, retry handling,
/// and hierarchical execution (subagents, nested calls).
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    /// Unique identifier for this invocation
    pub id: ToolInvocationId,
    /// Name of the tool being invoked
    pub tool_name: String,
    /// Arguments passed to the tool
    pub args: Value,
    /// Session identifier for grouping related invocations
    pub session_id: String,
    /// Attempt number (1-based, incremented on retry)
    pub attempt: u32,
    /// Parent invocation ID for nested/subagent calls
    pub parent_id: Option<ToolInvocationId>,
    /// Timestamp when invocation was created
    pub created_at: Instant,
}

impl ToolInvocation {
    /// Create a new tool invocation with generated ID.
    pub fn new(tool_name: impl Into<String>, args: Value, session_id: impl Into<String>) -> Self {
        Self {
            id: ToolInvocationId::new(),
            tool_name: tool_name.into(),
            args,
            session_id: session_id.into(),
            attempt: 1,
            parent_id: None,
            created_at: Instant::now(),
        }
    }

    /// Create a retry of this invocation with incremented attempt.
    pub fn retry(&self) -> Self {
        Self {
            id: ToolInvocationId::new(),
            tool_name: self.tool_name.clone(),
            args: self.args.clone(),
            session_id: self.session_id.clone(),
            attempt: self.attempt + 1,
            parent_id: self.parent_id,
            created_at: Instant::now(),
        }
    }

    /// Create a child invocation for nested/subagent calls.
    pub fn child(&self, tool_name: impl Into<String>, args: Value) -> Self {
        Self {
            id: ToolInvocationId::new(),
            tool_name: tool_name.into(),
            args,
            session_id: self.session_id.clone(),
            attempt: 1,
            parent_id: Some(self.id),
            created_at: Instant::now(),
        }
    }

    /// Get elapsed time since creation.
    #[inline]
    pub fn elapsed(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Check if this is a retry attempt.
    #[inline]
    pub fn is_retry(&self) -> bool {
        self.attempt > 1
    }

    /// Check if this is a nested/child invocation.
    #[inline]
    pub fn is_nested(&self) -> bool {
        self.parent_id.is_some()
    }
}

/// Builder for ergonomic ToolInvocation construction.
#[derive(Debug, Clone)]
pub struct InvocationBuilder {
    tool_name: String,
    args: Value,
    session_id: String,
    attempt: u32,
    parent_id: Option<ToolInvocationId>,
    id: Option<ToolInvocationId>,
}

impl InvocationBuilder {
    /// Start building a new invocation.
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            args: Value::Null,
            session_id: String::new(),
            attempt: 1,
            parent_id: None,
            id: None,
        }
    }

    /// Set the tool arguments.
    pub fn args(mut self, args: Value) -> Self {
        self.args = args;
        self
    }

    /// Set the session ID.
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    /// Set the attempt number.
    pub fn attempt(mut self, attempt: u32) -> Self {
        self.attempt = attempt.max(1);
        self
    }

    /// Set the parent invocation ID.
    pub fn parent_id(mut self, parent_id: ToolInvocationId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Set a specific invocation ID (for reconstruction).
    pub fn id(mut self, id: ToolInvocationId) -> Self {
        self.id = Some(id);
        self
    }

    /// Build the ToolInvocation.
    pub fn build(self) -> ToolInvocation {
        ToolInvocation {
            id: self.id.unwrap_or_default(),
            tool_name: self.tool_name,
            args: self.args,
            session_id: self.session_id,
            attempt: self.attempt,
            parent_id: self.parent_id,
            created_at: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_invocation_id_display() {
        let id = ToolInvocationId::new();
        let display = id.to_string();
        assert_eq!(display.len(), 36); // UUID hyphenated format
        assert!(display.contains('-'));
    }

    #[test]
    fn test_invocation_id_short() {
        let id = ToolInvocationId::new();
        let short = id.short();
        assert_eq!(short.len(), 8);
    }

    #[test]
    fn test_invocation_id_parse() {
        let id = ToolInvocationId::new();
        let s = id.to_string();
        let parsed = ToolInvocationId::parse(&s).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_invocation_creation() {
        let inv = ToolInvocation::new("read_file", json!({"path": "/tmp/test"}), "session-123");
        assert_eq!(inv.tool_name, "read_file");
        assert_eq!(inv.session_id, "session-123");
        assert_eq!(inv.attempt, 1);
        assert!(inv.parent_id.is_none());
    }

    #[test]
    fn test_invocation_retry() {
        let inv = ToolInvocation::new("grep_file", json!({"pattern": "TODO"}), "session-456");
        let retry = inv.retry();

        assert_ne!(inv.id, retry.id);
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.tool_name, inv.tool_name);
        assert_eq!(retry.args, inv.args);
    }

    #[test]
    fn test_invocation_child() {
        let parent = ToolInvocation::new("spawn_subagent", json!({}), "session-789");
        let child = parent.child("read_file", json!({"path": "/src/main.rs"}));

        assert_eq!(child.parent_id, Some(parent.id));
        assert_eq!(child.session_id, parent.session_id);
        assert_eq!(child.attempt, 1);
    }

    #[test]
    fn test_builder() {
        let inv = InvocationBuilder::new("write_file")
            .args(json!({"path": "/out.txt", "content": "hello"}))
            .session_id("builder-session")
            .attempt(3)
            .build();

        assert_eq!(inv.tool_name, "write_file");
        assert_eq!(inv.session_id, "builder-session");
        assert_eq!(inv.attempt, 3);
    }

    #[test]
    fn test_builder_with_parent() {
        let parent_id = ToolInvocationId::new();
        let inv = InvocationBuilder::new("nested_tool")
            .session_id("test")
            .parent_id(parent_id)
            .build();

        assert_eq!(inv.parent_id, Some(parent_id));
        assert!(inv.is_nested());
    }

    #[test]
    fn test_serde_roundtrip() {
        let id = ToolInvocationId::new();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: ToolInvocationId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }
}
