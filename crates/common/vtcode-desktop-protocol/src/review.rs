//! Workspace review snapshots and MCP app-resource preview DTOs.

use serde::{Deserialize, Serialize};

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

/// Maximum MCP app-resource body returned by one `mcp/app/read` call.
pub const MAX_MCP_APP_BODY_BYTES: usize = 64 * 1024;
