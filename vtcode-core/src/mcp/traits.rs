//! MCP trait abstractions for tool execution and elicitation handling.

use super::types::{McpClientStatus, McpElicitationRequest, McpElicitationResponse, McpToolInfo};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Callback interface used to resolve elicitation requests from MCP providers.
#[async_trait]
pub trait McpElicitationHandler: Send + Sync {
    async fn handle_elicitation(
        &self,
        provider: &str,
        request: McpElicitationRequest,
    ) -> Result<McpElicitationResponse>;
}

/// Trait abstraction used by the tool registry to talk to the MCP client.
#[async_trait]
pub trait McpToolExecutor: Send + Sync {
    async fn execute_mcp_tool(&self, tool_name: &str, args: &Value) -> Result<Value>;
    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>>;
    async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool>;
    fn get_status(&self) -> McpClientStatus;
}
