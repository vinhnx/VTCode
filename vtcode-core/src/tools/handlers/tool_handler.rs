//! Codex-compatible ToolHandler trait and types
//!
//! This module implements the handler pattern from OpenAI's Codex project,
//! providing a more modular and composable approach to tool execution.
//!
//! Key patterns from Codex:
//! - `ToolHandler` trait with kind/matches_kind/is_mutating/handle methods
//! - `ToolKind` enum for categorizing tool types
//! - `ToolPayload` for typed tool arguments
//! - `ToolOutput` for structured tool results
//! - `ToolInvocation` for execution context

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tool kind classification (from Codex)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    /// Standard function call tool
    Function,
    /// MCP (Model Context Protocol) tool
    Mcp,
    /// Custom/freeform tool (e.g., apply_patch with custom format)
    Custom,
}

/// Payload types for tool invocations (from Codex)
#[derive(Clone, Debug)]
pub enum ToolPayload {
    /// Standard function call with JSON arguments
    Function { arguments: String },
    /// Custom tool with freeform input (e.g., apply_patch)
    Custom { input: String },
    /// MCP tool call
    Mcp { arguments: Option<Value> },
    /// Local shell execution
    LocalShell { params: ShellToolCallParams },
}

/// Shell command parameters (from Codex)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ShellToolCallParams {
    pub command: Vec<String>,
    pub workdir: Option<String>,
    pub timeout_ms: Option<u64>,
    pub sandbox_permissions: Option<SandboxPermissions>,
    pub justification: Option<String>,
}

/// Sandbox permission levels (from Codex)
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPermissions {
    #[default]
    UseDefault,
    RequireEscalated,
}

/// Tool output types (from Codex)
#[derive(Clone, Debug)]
pub enum ToolOutput {
    /// Function call result
    Function {
        content: String,
        content_items: Option<Vec<ContentItem>>,
        success: Option<bool>,
    },
    /// MCP tool result
    Mcp { result: McpToolResult },
}

impl ToolOutput {
    /// Create a simple function output with just content
    pub fn simple(content: impl Into<String>) -> Self {
        Self::Function {
            content: content.into(),
            content_items: None,
            success: Some(true),
        }
    }

    /// Create a function output with success status
    pub fn with_success(content: impl Into<String>, success: bool) -> Self {
        Self::Function {
            content: content.into(),
            content_items: None,
            success: Some(success),
        }
    }

    /// Create an error output
    pub fn error(message: impl Into<String>) -> Self {
        Self::Function {
            content: message.into(),
            content_items: None,
            success: Some(false),
        }
    }

    /// Get the content string if this is a Function output
    pub fn content(&self) -> Option<&str> {
        match self {
            Self::Function { content, .. } => Some(content),
            Self::Mcp { result } => result.content.first().and_then(|c| c.as_text()),
        }
    }

    /// Check if the output indicates success
    pub fn is_success(&self) -> bool {
        match self {
            Self::Function { success, .. } => success.unwrap_or(true),
            Self::Mcp { result } => !result.is_error.unwrap_or(false),
        }
    }
}

/// Content item for multi-part responses (from Codex)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentItem {
    Text {
        text: String,
    },
    Image {
        data: String,
        mime_type: String,
    },
    Resource {
        uri: String,
        mime_type: Option<String>,
    },
}

impl ContentItem {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentItem::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// MCP tool result (from Codex)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpToolResult {
    pub content: Vec<ContentItem>,
    pub is_error: Option<bool>,
}

/// Context for tool invocation (from Codex)
pub struct ToolInvocation {
    pub session: Arc<dyn ToolSession>,
    pub turn: Arc<TurnContext>,
    pub tracker: Option<SharedDiffTracker>,
    pub call_id: String,
    pub tool_name: String,
    pub payload: ToolPayload,
}

/// Shared diff tracker type alias
pub type SharedDiffTracker = Arc<tokio::sync::Mutex<DiffTracker>>;

/// Session trait for tool execution context
#[async_trait]
pub trait ToolSession: Send + Sync {
    /// Get the current working directory
    fn cwd(&self) -> &PathBuf;

    /// Get workspace root
    fn workspace_root(&self) -> &PathBuf;

    /// Record a warning message
    async fn record_warning(&self, message: String);

    /// Get user's configured shell
    fn user_shell(&self) -> &str;

    /// Send an event
    async fn send_event(&self, event: ToolEvent);
}

/// Turn context for tool execution
#[derive(Clone, Debug)]
pub struct TurnContext {
    pub cwd: PathBuf,
    pub turn_id: String,
    pub sub_id: Option<String>,
    pub shell_environment_policy: ShellEnvironmentPolicy,
    pub approval_policy: ApprovalPolicy,
    pub codex_linux_sandbox_exe: Option<PathBuf>,
    /// Sandbox policy from Codex (for orchestrator integration)
    pub sandbox_policy: super::sandboxing::SandboxPolicy,
}

impl TurnContext {
    /// Resolve a path relative to the current working directory
    pub fn resolve_path(&self, path: Option<String>) -> PathBuf {
        match path {
            Some(p) => {
                let path = PathBuf::from(p);
                if path.is_absolute() {
                    path
                } else {
                    self.cwd.join(path)
                }
            }
            None => self.cwd.clone(),
        }
    }
}

/// Shell environment policy
#[derive(Clone, Debug, Default)]
pub enum ShellEnvironmentPolicy {
    #[default]
    Inherit,
    Clean,
    Custom(HashMap<String, String>),
}

/// Approval policy for tool execution
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ApprovalPolicy {
    #[default]
    Never,
    OnMutation,
    Always,
}

/// Diff tracker for file changes
#[derive(Default, Debug)]
pub struct DiffTracker {
    pub changes: HashMap<PathBuf, FileChange>,
}

impl DiffTracker {
    pub fn on_patch_begin(&mut self, changes: &HashMap<PathBuf, FileChange>) {
        self.changes.extend(changes.clone());
    }

    pub fn on_patch_end(&mut self, success: bool) {
        if !success {
            self.changes.clear();
        }
    }
}

/// File change types (from Codex protocol)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileChange {
    Add {
        content: String,
    },
    Delete,
    Update {
        old_content: String,
        new_content: String,
    },
    Rename {
        new_path: PathBuf,
        content: Option<String>,
    },
}

/// Tool execution events (from Codex)
#[derive(Clone, Debug)]
pub enum ToolEvent {
    Begin(ToolEventBegin),
    Success(ToolEventSuccess),
    Failure(ToolEventFailure),
    PatchApplyBegin(PatchApplyBeginEvent),
    PatchApplyEnd(PatchApplyEndEvent),
}

#[derive(Clone, Debug)]
pub struct ToolEventBegin {
    pub call_id: String,
    pub tool_name: String,
    pub turn_id: String,
}

#[derive(Clone, Debug)]
pub struct ToolEventSuccess {
    pub call_id: String,
    pub output: String,
}

#[derive(Clone, Debug)]
pub struct ToolEventFailure {
    pub call_id: String,
    pub error: String,
}

#[derive(Clone, Debug)]
pub struct PatchApplyBeginEvent {
    pub call_id: String,
    pub turn_id: String,
    pub changes: HashMap<PathBuf, FileChange>,
    pub auto_approved: bool,
}

#[derive(Clone, Debug)]
pub struct PatchApplyEndEvent {
    pub call_id: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Error type for tool execution (from Codex)
#[derive(Debug, thiserror::Error)]
pub enum ToolCallError {
    /// Error that should be sent back to the model
    #[error("Tool error: {0}")]
    RespondToModel(String),

    /// Internal error that should not be sent to the model
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),

    /// Tool was rejected by approval policy
    #[error("Tool rejected: {0}")]
    Rejected(String),

    /// Tool timed out
    #[error("Tool timed out after {0}ms")]
    Timeout(u64),
}

impl ToolCallError {
    /// Create an error to respond to the model
    pub fn respond(message: impl Into<String>) -> Self {
        Self::RespondToModel(message.into())
    }
}

/// Core trait for tool handlers (from Codex)
///
/// This trait provides a modular approach to tool execution, separating
/// concerns like kind matching, mutation detection, and actual execution.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Get the kind of tool this handler supports
    fn kind(&self) -> ToolKind;

    /// Check if the handler can process the given payload type
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            (self.kind(), payload),
            (ToolKind::Function, ToolPayload::Function { .. })
                | (ToolKind::Mcp, ToolPayload::Mcp { .. })
                | (ToolKind::Custom, ToolPayload::Custom { .. })
        )
    }

    /// Check if this invocation would mutate state
    ///
    /// Used for approval policies - read-only tools can often be auto-approved
    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false
    }

    /// Execute the tool and return the output
    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError>;
}

/// Tool spec types (from Codex)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolSpec {
    Function(ResponsesApiTool),
    Freeform(FreeformTool),
    WebSearch {},
    LocalShell {},
}

impl ToolSpec {
    pub fn name(&self) -> &str {
        match self {
            ToolSpec::Function(tool) => &tool.name,
            ToolSpec::Freeform(tool) => &tool.name,
            ToolSpec::WebSearch {} => "web_search",
            ToolSpec::LocalShell {} => "local_shell",
        }
    }
}

/// OpenAI Responses API tool definition (from Codex)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResponsesApiTool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub strict: bool,
    pub parameters: JsonSchema,
}

/// Freeform tool definition (from Codex)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeformTool {
    pub name: String,
    pub description: String,
    pub format: FreeformToolFormat,
}

/// Freeform tool format
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeformToolFormat {
    pub lark_grammar: Option<String>,
    pub examples: Vec<String>,
}

/// JSON Schema for tool parameters (from Codex)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum JsonSchema {
    Object {
        #[serde(default)]
        properties: std::collections::BTreeMap<String, JsonSchema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        required: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        additional_properties: Option<AdditionalProperties>,
    },
    String {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Number {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Boolean {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Array {
        items: Box<JsonSchema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Null,
}

/// Additional properties configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AdditionalProperties {
    Boolean(bool),
    Schema(Box<JsonSchema>),
}

impl From<bool> for AdditionalProperties {
    fn from(value: bool) -> Self {
        AdditionalProperties::Boolean(value)
    }
}

/// Configured tool spec with parallel execution support
#[derive(Clone, Debug)]
pub struct ConfiguredToolSpec {
    pub spec: ToolSpec,
    pub supports_parallel_tool_calls: bool,
}

impl ConfiguredToolSpec {
    pub fn new(spec: ToolSpec, supports_parallel: bool) -> Self {
        Self {
            spec,
            supports_parallel_tool_calls: supports_parallel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_output_simple() {
        let output = ToolOutput::simple("Hello, world!");
        assert!(output.is_success());
        assert_eq!(output.content(), Some("Hello, world!"));
    }

    #[test]
    fn test_tool_output_error() {
        let output = ToolOutput::error("Something went wrong");
        assert!(!output.is_success());
        assert_eq!(output.content(), Some("Something went wrong"));
    }

    #[test]
    fn test_sandbox_permissions_default() {
        let perms = SandboxPermissions::default();
        assert_eq!(perms, SandboxPermissions::UseDefault);
    }

    #[test]
    fn test_turn_context_resolve_path_absolute() {
        let ctx = TurnContext {
            cwd: PathBuf::from("/workspace"),
            turn_id: "test".to_string(),
            sub_id: None,
            shell_environment_policy: ShellEnvironmentPolicy::default(),
            approval_policy: ApprovalPolicy::default(),
            codex_linux_sandbox_exe: None,
            sandbox_policy: Default::default(),
        };

        let resolved = ctx.resolve_path(Some("/absolute/path".to_string()));
        assert_eq!(resolved, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_turn_context_resolve_path_relative() {
        let ctx = TurnContext {
            cwd: PathBuf::from("/workspace"),
            turn_id: "test".to_string(),
            sub_id: None,
            shell_environment_policy: ShellEnvironmentPolicy::default(),
            approval_policy: ApprovalPolicy::default(),
            codex_linux_sandbox_exe: None,
            sandbox_policy: Default::default(),
        };

        let resolved = ctx.resolve_path(Some("relative/path".to_string()));
        assert_eq!(resolved, PathBuf::from("/workspace/relative/path"));
    }
}
