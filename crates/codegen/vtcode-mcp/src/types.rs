//! Core MCP data types for tool, resource, and prompt information.

use rmcp::model::{ElicitationAction, PromptArgument, PromptMessage, ResourceContents};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Information about an MCP tool exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub provider: String,
    pub input_schema: Value,
}

/// Summary of an MCP resource exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpResourceInfo {
    pub provider: String,
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
}

/// Resource contents fetched from an MCP provider.
#[derive(Debug, Clone)]
pub struct McpResourceData {
    pub provider: String,
    pub uri: String,
    pub contents: Vec<ResourceContents>,
    pub meta: Map<String, Value>,
}

/// Summary of an MCP prompt exposed by a provider.
#[derive(Debug, Clone)]
pub struct McpPromptInfo {
    pub provider: String,
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<PromptArgument>,
}

/// Fully rendered MCP prompt ready for use.
#[derive(Debug, Clone)]
pub struct McpPromptDetail {
    pub provider: String,
    pub name: String,
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
    pub meta: Map<String, Value>,
}

/// Snapshot describing the MCP client at runtime.
#[derive(Debug, Clone)]
pub struct McpClientStatus {
    pub enabled: bool,
    pub provider_count: usize,
    pub active_connections: usize,
    pub configured_providers: Vec<String>,
}

/// Request payload for handling elicitation prompts from MCP providers.
#[derive(Debug, Clone)]
pub struct McpElicitationRequest {
    pub message: String,
    pub requested_schema: Value,
    pub meta: Option<Value>,
}

/// Result returned by an elicitation handler after interacting with the user.
#[derive(Debug, Clone)]
pub struct McpElicitationResponse {
    pub action: ElicitationAction,
    pub content: Option<Value>,
    pub meta: Option<Value>,
}

// === File Upload Types (PR #15197) ===

/// Metadata key used in tool input schemas to indicate file upload parameters.
pub const OPENAI_FILE_PARAMS_META_KEY: &str = "_meta";
pub const OPENAI_FILE_PARAMS_VALUE: &str = "openai/fileParams";

/// Shape of a file payload provided by the client after upload.
///
/// This is the object shape that MCP tools expecting file uploads expect to receive.
/// The client uploads local files to the backend and replaces the local file path
/// with this structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidedFilePayload {
    /// The file ID returned from the upload (maps to backend file reference)
    #[serde(rename = "fileId")]
    pub file_id: String,
    /// The original file name
    #[serde(rename = "fileName")]
    pub file_name: String,
    /// MIME type of the uploaded file
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// File size in bytes
    #[serde(rename = "fileSize")]
    pub file_size: i64,
}

/// Result of uploading a local file to the backend.
#[derive(Debug, Clone)]
pub struct FileUploadResult {
    /// The file ID from the backend upload
    pub file_id: String,
    /// The payload to substitute in the tool call arguments
    pub payload: ProvidedFilePayload,
}

/// Masked schema entry for a file parameter in a tool's input schema.
///
/// When a tool declares `_meta["openai/fileParams"]` in its input schema,
/// we mask the raw file payload object and instead instruct the model to
/// provide an absolute local file path. The client then handles the upload
/// and argument rewriting transparently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileParamSchemaEntry {
    /// The original property name in the tool's input schema
    pub property_name: String,
    /// Description shown to the model for this file parameter
    pub description: String,
    /// Whether this file parameter is required
    pub required: bool,
}
