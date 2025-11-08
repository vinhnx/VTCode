//! Inter-process communication for calling MCP tools from sandboxed code.
//!
//! This module provides a file-based IPC mechanism that allows code running in
//! a sandbox to call MCP tools. The code writes tool requests to a file, and
//! the executor reads and processes them, writing back results.
//!
//! # Protocol
//!
//! Requests (code → executor):
//! ```json
//! {
//!   "id": "uuid",
//!   "tool_name": "search_tools",
//!   "args": {"keyword": "file"}
//! }
//! ```
//!
//! Responses (executor → code):
//! ```json
//! {
//!   "id": "uuid",
//!   "success": true,
//!   "result": {...}
//! }
//! ```
//! or
//! ```json
//! {
//!   "id": "uuid",
//!   "success": false,
//!   "error": "Tool not found"
//! }
//! ```

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::time::sleep;
use uuid::Uuid;

/// IPC request from sandboxed code to executor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolRequest {
    pub id: String,
    pub tool_name: String,
    pub args: Value,
}

/// IPC response from executor to sandboxed code.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResponse {
    pub id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// IPC handler for tool invocation between code and executor.
pub struct ToolIpcHandler {
    ipc_dir: PathBuf,
}

impl ToolIpcHandler {
    /// Create a new IPC handler with the given directory.
    pub fn new(ipc_dir: PathBuf) -> Self {
        Self { ipc_dir }
    }

    /// Read a tool request from the code.
    pub async fn read_request(&self) -> Result<Option<ToolRequest>> {
        let request_file = self.ipc_dir.join("request.json");

        if !request_file.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&request_file)
            .await
            .context("failed to read request file")?;

        let request: ToolRequest = serde_json::from_str(&content)
            .context("failed to parse request JSON")?;

        // Clean up request file
        let _ = fs::remove_file(&request_file).await;

        Ok(Some(request))
    }

    /// Write a tool response back to the code.
    pub async fn write_response(&self, response: ToolResponse) -> Result<()> {
        let response_file = self.ipc_dir.join("response.json");

        let json = serde_json::to_string(&response)
            .context("failed to serialize response")?;

        fs::write(&response_file, json)
            .await
            .context("failed to write response file")?;

        Ok(())
    }

    /// Wait for a request with timeout.
    pub async fn wait_for_request(&self, timeout: Duration) -> Result<Option<ToolRequest>> {
        let start = std::time::Instant::now();

        loop {
            if let Some(request) = self.read_request().await? {
                return Ok(Some(request));
            }

            if start.elapsed() > timeout {
                return Ok(None);
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Create a request ID.
    pub fn new_request_id() -> String {
        Uuid::new_v4().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialize_tool_request() {
        let request = ToolRequest {
            id: "test-id".to_string(),
            tool_name: "read_file".to_string(),
            args: json!({"path": "/test"}),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test-id"));
        assert!(json.contains("read_file"));
    }

    #[test]
    fn serialize_success_response() {
        let response = ToolResponse {
            id: "test-id".to_string(),
            success: true,
            result: Some(json!({"data": "test"})),
            error: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test-id"));
        assert!(json.contains("true"));
        assert!(!json.contains("error"));
    }

    #[test]
    fn serialize_error_response() {
        let response = ToolResponse {
            id: "test-id".to_string(),
            success: false,
            result: None,
            error: Some("File not found".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test-id"));
        assert!(json.contains("false"));
        assert!(json.contains("File not found"));
    }
}
