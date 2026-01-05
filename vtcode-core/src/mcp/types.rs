//! Core MCP data types for tool, resource, and prompt information.

use mcp_types::{PromptArgument, PromptMessage, ReadResourceResultContentsItem};
use rmcp::model::ElicitationAction;
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
    pub contents: Vec<ReadResourceResultContentsItem>,
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
}

/// Result returned by an elicitation handler after interacting with the user.
#[derive(Debug, Clone)]
pub struct McpElicitationResponse {
    pub action: ElicitationAction,
    pub content: Option<Value>,
}
