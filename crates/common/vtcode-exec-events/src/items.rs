//! Timeline item payloads carried by thread item events.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Diagnostic, ErrorItem, HarnessEventItem, ItemContext};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ThreadItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Box<ItemContext>>,
    /// Stable identifier associated with the item.
    pub id: String,
    /// Embedded event details for the item type.
    #[serde(flatten)]
    pub details: ThreadItemDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadItemDetails {
    /// Message authored by the agent.
    AgentMessage(AgentMessageItem),
    /// Structured plan content authored by the agent in Planning workflow.
    Plan(PlanItem),
    /// Free-form reasoning text produced during a turn.
    Reasoning(Box<ReasoningItem>),
    /// Public rationale explicitly recorded by the agent; never private reasoning.
    Decision(Box<DecisionItem>),
    /// Command execution lifecycle update for an actual shell/PTY process.
    CommandExecution(Box<CommandExecutionItem>),
    /// Tool invocation lifecycle update.
    ToolInvocation(Box<ToolInvocationItem>),
    /// Tool output lifecycle update tied to a tool invocation.
    ToolOutput(Box<ToolOutputItem>),
    /// File change summary associated with the turn.
    FileChange(Box<FileChangeItem>),
    /// MCP tool invocation status.
    McpToolCall(Box<McpToolCallItem>),
    /// Web search event emitted by a registered search provider.
    WebSearch(Box<WebSearchItem>),
    /// Harness-managed continuation or verification lifecycle event.
    Harness(Box<HarnessEventItem>),
    /// General error captured for auditing.
    Error(ErrorItem),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct AgentMessageItem {
    /// Textual content of the agent message.
    pub text: String,
}

/// A consequential choice and the agent's reported public rationale.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct DecisionItem {
    pub summary: String,
    pub rationale: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct PlanItem {
    /// Plan markdown content.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ReasoningItem {
    /// Operational diagnostic independent of private model reasoning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Box<Diagnostic>>,
    /// Free-form reasoning content captured during planning.
    pub text: String,
    /// Optional stage of reasoning (e.g., "analysis", "plan", "verification",
    /// or the bounded evidence-only "diagnosis" stage).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CommandExecutionStatus {
    /// Command finished successfully.
    #[default]
    Completed,
    /// Command failed (non-zero exit code or runtime error).
    Failed,
    /// Command is still running and may emit additional output.
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct CommandExecutionItem {
    /// Tool or command identifier executed by the runner.
    pub command: String,
    /// Arguments passed to the tool invocation, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// Aggregated output emitted by the command.
    #[serde(default)]
    pub aggregated_output: String,
    /// Exit code reported by the process, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Current status of the command execution.
    pub status: CommandExecutionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Tool finished successfully.
    #[default]
    Completed,
    /// Tool failed.
    Failed,
    /// Tool is still running and may emit additional output.
    InProgress,
}

/// Fine-grained outcome of a tool invocation lifecycle.
///
/// Mirrors the outcome taxonomy used by the runtime: `status` remains the
/// coarse lifecycle signal (`Completed` / `Failed` / `InProgress`), while
/// `outcome` captures *why* the invocation terminated. Consumers that only
/// need success/failure can continue to read `status`; analytics and the UI
/// layer use `outcome` for richer classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    /// Tool executed and returned a result.
    #[default]
    Success,
    /// Tool executed but returned an error.
    Error,
    /// User rejected the permission prompt.
    PermissionRejected,
    /// User cancelled the permission prompt (e.g. Ctrl+C / Esc).
    PermissionCancelled,
    /// User provided a followup message instead of approving.
    Followup,
    /// A user-configured hook blocked execution.
    HookDenied,
    /// Tool not found or arguments couldn't be parsed.
    InvalidTool,
    /// Tool was cancelled during execution or closed before dispatch when its turn ended.
    Cancelled,
}

impl ToolOutcome {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Followup)
    }
}

/// Map a terminal [`ToolCallStatus`] to its corresponding [`ToolOutcome`].
///
/// # Panics
///
/// Panics if `status` is [`ToolCallStatus::InProgress`], which is a non-terminal
/// state and must never be passed to a completion-event emitter.
#[must_use]
#[allow(
    clippy::unreachable,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub fn tool_outcome_from_status(status: &ToolCallStatus) -> ToolOutcome {
    match status {
        ToolCallStatus::Completed => ToolOutcome::Success,
        ToolCallStatus::Failed => ToolOutcome::Error,
        ToolCallStatus::InProgress => unreachable!("InProgress status passed to completion event"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ToolInvocationItem {
    /// Name of the invoked tool.
    pub tool_name: String,
    /// Structured arguments passed to the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// Raw model-emitted tool call identifier, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Current lifecycle status of the invocation.
    pub status: ToolCallStatus,
    /// Fine-grained outcome of the invocation lifecycle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ToolOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct ToolOutputItem {
    /// Identifier of the related harness invocation item.
    pub call_id: String,
    /// Raw model-emitted tool call identifier, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Canonical spool file path when the full output was written to disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spool_path: Option<String>,
    /// Aggregated output emitted by the tool.
    #[serde(default)]
    pub output: String,
    /// Exit code reported by the tool, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Current lifecycle status of the output item.
    pub status: ToolCallStatus,
    /// MCP Apps referenced by this output, when the tool result was enriched
    /// with app disclosure. Surfaces render preview affordances from these;
    /// empty for non-app results. Schema 0.21.0; omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_previews: Vec<AppPreviewItem>,
}

/// Disclosure facts about one MCP Apps resource referenced by a tool output.
///
/// Mirrors the disclosure attached at execution time; surfaces must not
/// re-derive these by parsing rendered output text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct AppPreviewItem {
    /// Canonical `ui://` resource URI.
    pub uri: String,
    /// MIME type of the first content block, when the server provides one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Total body bytes across content blocks.
    #[serde(default)]
    pub bytes: u64,
    /// Number of content blocks.
    #[serde(default)]
    pub blocks: usize,
    /// Declared UI sandbox posture, when the server declares `_meta.ui`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<AppUiPosture>,
}

/// Declared sandbox posture of an MCP Apps UI resource (SEP-1865 `UIResourceMeta`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct AppUiPosture {
    /// Origins for network requests (`csp.connectDomains`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connect_domains: Vec<String>,
    /// Origins for static resources (`csp.resourceDomains`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_domains: Vec<String>,
    /// Origins for nested iframes (`csp.frameDomains`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_domains: Vec<String>,
    /// Allowed base URIs (`csp.baseUriDomains`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_uri_domains: Vec<String>,
    /// Requested permission-policy features (`permissions`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
    /// Dedicated sandbox origin for the view (`domain`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct FileChangeItem {
    /// Captured preview was truncated, suppressed, or unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_incomplete: Option<bool>,
    /// List of individual file updates included in the change set.
    pub changes: Vec<FileUpdateChange>,
    /// Whether the patch application succeeded.
    pub status: PatchApplyStatus,
    /// Optional precomputed unified diff for the change set.
    ///
    /// Populated by the turn diff tracker so consumers can render per-change
    /// previews without recomputation. Absent in older events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_diff: Option<String>,
    /// Optional added-line count for the change set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    /// Optional deleted-line count for the change set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct FileUpdateChange {
    /// Path of the file that was updated.
    pub path: String,
    /// Type of change applied to the file.
    pub kind: PatchChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PatchApplyStatus {
    /// Patch successfully applied.
    Completed,
    /// Patch application failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PatchChangeKind {
    /// File addition.
    Add,
    /// File deletion.
    Delete,
    /// File update in place.
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct McpToolCallItem {
    /// Name of the MCP tool invoked by the agent.
    pub tool_name: String,
    /// Arguments passed to the tool invocation, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// Result payload returned by the tool, if captured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Lifecycle status for the tool call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<McpToolCallStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum McpToolCallStatus {
    /// Tool invocation has started.
    Started,
    /// Tool invocation completed successfully.
    Completed,
    /// Tool invocation failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct WebSearchItem {
    /// Query that triggered the search.
    pub query: String,
    /// Search provider identifier, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional raw search results captured for auditing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<String>>,
}
