//! Inter-process communication for calling MCP tools from sandboxed code.
//!
//! This module provides a file-based IPC mechanism that allows code running in
//! a sandbox to call MCP tools. The code writes tool requests to a file, and
//! the executor reads and processes them, writing back results.
//!
//! Optionally supports PII (Personally Identifiable Information) protection by
//! tokenizing sensitive data in requests before tool execution and de-tokenizing
//! responses before returning to the code.
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
//!
//! # PII Protection
//!
//! When enabled, the handler automatically:
//! 1. Detects PII patterns in request arguments
//! 2. Tokenizes sensitive data before tool execution
//! 3. De-tokenizes responses before returning to code
//! 4. Maintains token mapping for the session

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
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
    pii_tokenizer: Option<Arc<crate::exec::PiiTokenizer>>,
}

impl ToolIpcHandler {
    /// Create a new IPC handler with the given directory.
    pub fn new(ipc_dir: PathBuf) -> Self {
        Self {
            ipc_dir,
            pii_tokenizer: None,
        }
    }

    /// Create a new IPC handler with PII protection enabled.
    pub fn with_pii_protection(ipc_dir: PathBuf) -> Self {
        Self {
            ipc_dir,
            pii_tokenizer: Some(Arc::new(crate::exec::PiiTokenizer::new())),
        }
    }

    /// Enable PII protection on existing handler.
    pub fn enable_pii_protection(&mut self) {
        self.pii_tokenizer = Some(Arc::new(crate::exec::PiiTokenizer::new()));
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

        let request: ToolRequest =
            serde_json::from_str(&content).context("failed to parse request JSON")?;

        // Clean up request file
        let _ = fs::remove_file(&request_file).await;

        Ok(Some(request))
    }

    /// Process request for PII (tokenize if enabled).
    pub fn process_request_for_pii(&self, request: &mut ToolRequest) -> Result<()> {
        if let Some(tokenizer) = &self.pii_tokenizer {
            let args_str =
                serde_json::to_string(&request.args).context("failed to serialize request args")?;
            let (tokenized, _) = tokenizer
                .tokenize_string(&args_str)
                .context("PII tokenization failed")?;
            request.args =
                serde_json::from_str(&tokenized).context("failed to parse tokenized args")?;
        }
        Ok(())
    }

    /// Process response for PII (de-tokenize if enabled).
    pub fn process_response_for_pii(&self, response: &mut ToolResponse) -> Result<()> {
        if let Some(tokenizer) = &self.pii_tokenizer {
            if let Some(result) = &response.result {
                let result_str =
                    serde_json::to_string(result).context("failed to serialize response result")?;
                let detokenized = tokenizer
                    .detokenize_string(&result_str)
                    .context("PII de-tokenization failed")?;
                response.result = Some(
                    serde_json::from_str(&detokenized)
                        .context("failed to parse de-tokenized result")?,
                );
            }
        }
        Ok(())
    }

    /// Write a tool response back to the code.
    pub async fn write_response(&self, mut response: ToolResponse) -> Result<()> {
        // De-tokenize response before writing back to code
        self.process_response_for_pii(&mut response)?;

        let response_file = self.ipc_dir.join("response.json");

        let json = serde_json::to_string(&response).context("failed to serialize response")?;

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
