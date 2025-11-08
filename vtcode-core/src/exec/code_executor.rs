//! Code execution environment for agents using MCP tools programmatically.
//!
//! This module allows agents to write and execute code snippets that interact with
//! MCP tools as library functions, rather than making individual tool calls. This
//! improves efficiency through:
//!
//! - Control flow: loops, conditionals, error handling without repeated model calls
//! - Data filtering: process results before returning to model
//! - Latency: code runs locally in sandbox environment
//! - Context: intermediate results stay local unless explicitly logged
//!
//! # Example
//!
//! ```ignore
//! let executor = CodeExecutor::new(
//!     Language::Python3,
//!     sandbox_profile,
//!     Arc::new(mcp_client),
//!     PathBuf::from("/workspace"),
//! );
//!
//! // Agent writes Python code
//! let code = r#"
//! files = list_files(path="/workspace", recursive=True)
//! filtered = [f for f in files if "test" in f]
//! result = {"count": len(filtered), "files": filtered[:10]}
//! "#;
//!
//! let result = executor.execute(code).await?;
//! ```

use crate::exec::async_command::{AsyncProcessRunner, ProcessOptions, StreamCaptureConfig};
use crate::exec::sdk_ipc::{ToolIpcHandler, ToolResponse};
use crate::mcp::McpToolExecutor;
use crate::sandbox::SandboxProfile;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tracing::{debug, info};

/// Supported languages for code execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python3,
    JavaScript,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Python3 => "python3",
            Self::JavaScript => "javascript",
        }
    }

    pub fn interpreter(&self) -> &'static str {
        match self {
            Self::Python3 => "python3",
            Self::JavaScript => "node",
        }
    }
}

/// Result of code execution in the sandbox.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Exit code from the execution environment
    pub exit_code: i32,
    /// Standard output from the code
    pub stdout: String,
    /// Standard error output
    pub stderr: String,
    /// Parsed JSON result if available (from `result = {...}` in code)
    pub json_result: Option<Value>,
    /// Total execution time in milliseconds
    pub duration_ms: u128,
}

/// Configuration for code execution.
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Maximum execution time in seconds
    pub timeout_secs: u64,
    /// Maximum memory in MB
    pub memory_limit_mb: u64,
    /// Maximum output size in bytes
    pub max_output_bytes: usize,
    /// Enable network access in sandbox
    pub allow_network: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            memory_limit_mb: 256,
            max_output_bytes: 10 * 1024 * 1024, // 10 MB
            allow_network: false,
        }
    }
}

/// Code executor for running agent code in sandboxed environment.
pub struct CodeExecutor {
    language: Language,
    #[allow(dead_code)]
    sandbox_profile: SandboxProfile,
    mcp_client: Arc<dyn McpToolExecutor>,
    config: ExecutionConfig,
    workspace_root: PathBuf,
}

impl CodeExecutor {
    /// Create a new code executor.
    pub fn new(
        language: Language,
        sandbox_profile: SandboxProfile,
        mcp_client: Arc<dyn McpToolExecutor>,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            language,
            sandbox_profile,
            mcp_client,
            config: ExecutionConfig::default(),
            workspace_root,
        }
    }

    /// Set custom execution configuration.
    pub fn with_config(mut self, config: ExecutionConfig) -> Self {
        self.config = config;
        self
    }

    /// Execute code snippet and return result.
    ///
    /// # Arguments
    /// * `code` - Code snippet to execute (Python 3 or JavaScript)
    ///
    /// # Returns
    /// Execution result with output, exit code, and optional JSON result
    ///
    /// The code can access MCP tools as library functions. Any `result = {...}`
    /// assignment at the module level will be captured as JSON output.
    pub async fn execute(&self, code: &str) -> Result<ExecutionResult> {
        info!(
            language = self.language.as_str(),
            timeout_secs = self.config.timeout_secs,
            "Executing code snippet"
        );

        let start = Instant::now();

        // Set up IPC directory for tool invocation
        let ipc_dir = self.workspace_root.join(".vtcode").join("ipc");
        tokio::fs::create_dir_all(&ipc_dir).await
            .context("failed to create IPC directory")?;

        // Generate the SDK wrapper
        let sdk = self.generate_sdk().await
            .context("failed to generate SDK")?;

        // Prepare the complete code with SDK
        let complete_code = match self.language {
            Language::Python3 => self.prepare_python_code(&sdk, code)?,
            Language::JavaScript => self.prepare_javascript_code(&sdk, code)?,
        };

        // Write code to temporary file in workspace
        let code_file = self.workspace_root.join(".vtcode").join("code_temp");
        tokio::fs::create_dir_all(self.workspace_root.join(".vtcode")).await
            .context("failed to create .vtcode directory")?;
        tokio::fs::write(&code_file, &complete_code).await
            .context("failed to write code file")?;

        debug!(
            language = self.language.as_str(),
            code_file = ?code_file,
            "Wrote code to temporary file"
        );

        // Execute code via ProcessRunner with timeout
        let mut env = HashMap::new();
        
        // Set workspace path for scripts
        env.insert(
            OsString::from("VTCODE_WORKSPACE"),
            OsString::from(self.workspace_root.to_string_lossy().to_string()),
        );

        // Set IPC directory for tool invocation
        env.insert(
            OsString::from("VTCODE_IPC_DIR"),
            OsString::from(ipc_dir.to_string_lossy().to_string()),
        );

        // Spawn IPC handler task that will process tool requests from code
        let ipc_handler = ToolIpcHandler::new(ipc_dir.clone());
        let mcp_client = self.mcp_client.clone();
        let execution_timeout = Duration::from_secs(self.config.timeout_secs);
        
        let ipc_task: JoinHandle<Result<()>> = tokio::spawn(async move {
            let ipc_start = Instant::now();
            
            while ipc_start.elapsed() < execution_timeout {
                // Check for tool requests
                if let Some(request) = ipc_handler.read_request().await? {
                    debug!(
                        tool_name = %request.tool_name,
                        request_id = %request.id,
                        "Processing tool request from code"
                    );

                    // Execute the tool
                    let result = match mcp_client.execute_mcp_tool(&request.tool_name, request.args.clone()).await {
                        Ok(result) => {
                            debug!(tool_name = %request.tool_name, "Tool executed successfully");
                            ToolResponse {
                                id: request.id.clone(),
                                success: true,
                                result: Some(result),
                                error: None,
                            }
                        }
                        Err(e) => {
                            debug!(
                                tool_name = %request.tool_name,
                                error = %e,
                                "Tool execution failed"
                            );
                            ToolResponse {
                                id: request.id,
                                success: false,
                                result: None,
                                error: Some(e.to_string()),
                            }
                        }
                    };

                    // Write response
                    ipc_handler.write_response(result).await?;
                } else {
                    // No request yet, sleep and retry
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }

            Ok(())
        });

        let options = ProcessOptions {
            program: self.language.interpreter().to_string(),
            args: vec![code_file.to_string_lossy().to_string()],
            env,
            current_dir: Some(self.workspace_root.clone()),
            timeout: Some(Duration::from_secs(self.config.timeout_secs)),
            cancellation_token: None,
            stdout: StreamCaptureConfig {
                capture: true,
                max_bytes: self.config.max_output_bytes,
            },
            stderr: StreamCaptureConfig {
                capture: true,
                max_bytes: self.config.max_output_bytes,
            },
        };

        let process_output = AsyncProcessRunner::run(options).await
            .context("failed to execute code")?;

        let duration_ms = start.elapsed().as_millis();

        // Parse output
        let stdout = String::from_utf8_lossy(&process_output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&process_output.stderr).to_string();

        // Extract JSON result if present
        let json_result = self.extract_json_result(&stdout, self.language)?;

        // Clean up temp files
        let _ = tokio::fs::remove_file(&code_file).await;
        let _ = tokio::fs::remove_dir_all(&ipc_dir).await;

        // Wait for IPC task to complete (with timeout)
        let ipc_result = tokio::time::timeout(
            Duration::from_secs(1),
            ipc_task
        ).await;

        if let Err(e) = ipc_result {
            debug!(error = %e, "IPC handler did not complete in time");
        }

        debug!(
            exit_code = process_output.exit_status.code().unwrap_or(-1),
            duration_ms,
            has_json_result = json_result.is_some(),
            "Code execution completed"
        );

        Ok(ExecutionResult {
            exit_code: process_output.exit_status.code().unwrap_or(-1),
            stdout,
            stderr,
            json_result,
            duration_ms,
        })
    }

    /// Prepare Python code with SDK and user code.
    fn prepare_python_code(&self, sdk: &str, user_code: &str) -> Result<String> {
        Ok(format!(
            "{}\n\n# User code\n{}\n\n# Capture result\nimport json\nif 'result' in dir():\n    print('__JSON_RESULT__')\n    print(json.dumps(result, default=str))\n    print('__END_JSON__')",
            sdk, user_code
        ))
    }

    /// Prepare JavaScript code with SDK and user code.
    fn prepare_javascript_code(&self, sdk: &str, user_code: &str) -> Result<String> {
        Ok(format!(
            "{}\n\n// User code\n(async () => {{\n{}\n\n// Capture result\nif (typeof result !== 'undefined') {{\n  console.log('__JSON_RESULT__');\n  console.log(JSON.stringify(result, null, 2));\n  console.log('__END_JSON__');\n}}\n}})();\n",
            sdk, user_code
        ))
    }

    /// Extract JSON result from stdout between markers.
    fn extract_json_result(&self, stdout: &str, _language: Language) -> Result<Option<Value>> {
        if !stdout.contains("__JSON_RESULT__") {
            return Ok(None);
        }

        let start_marker = "__JSON_RESULT__";
        let end_marker = "__END_JSON__";

        let start = match stdout.find(start_marker) {
            Some(pos) => pos + start_marker.len(),
            None => return Ok(None),
        };

        let end = match stdout[start..].find(end_marker) {
            Some(pos) => start + pos,
            None => return Ok(None),
        };

        let json_str = stdout[start..end].trim();

        match serde_json::from_str::<Value>(json_str) {
            Ok(value) => {
                debug!("Extracted JSON result from code output");
                Ok(Some(value))
            }
            Err(e) => {
                debug!(error = %e, "Failed to parse JSON result");
                Ok(None)
            }
        }
    }

    /// Generate SDK module imports for the target language.
    pub async fn generate_sdk(&self) -> Result<String> {
        match self.language {
            Language::Python3 => self.generate_python_sdk().await,
            Language::JavaScript => self.generate_javascript_sdk().await,
        }
    }

    /// Generate Python SDK with MCP tool wrappers.
    async fn generate_python_sdk(&self) -> Result<String> {
        debug!("Generating Python SDK for MCP tools");

        let tools = self.mcp_client
            .list_mcp_tools()
            .await
            .context("failed to list MCP tools")?;

        let mut sdk = String::from(
            r#"# MCP Tools SDK - Auto-generated
import json
import sys
import os
import time
from typing import Any, Dict, Optional
from uuid import uuid4

class MCPTools:
    """Interface to MCP tools from agent code via file-based IPC."""
    
    IPC_DIR = os.environ.get("VTCODE_IPC_DIR", "/tmp/vtcode_ipc")
    
    def __init__(self):
        self._call_count = 0
        self._results = []
        os.makedirs(self.IPC_DIR, exist_ok=True)
    
    def _call_tool(self, name: str, args: Dict[str, Any]) -> Any:
        """Call an MCP tool via file-based IPC."""
        request_id = str(uuid4())
        
        # Write request
        request = {
            "id": request_id,
            "tool_name": name,
            "args": args
        }
        request_file = os.path.join(self.IPC_DIR, "request.json")
        with open(request_file, 'w') as f:
            json.dump(request, f)
        
        # Wait for response
        response_file = os.path.join(self.IPC_DIR, "response.json")
        timeout = 30
        start = time.time()
        while time.time() - start < timeout:
            if os.path.exists(response_file):
                with open(response_file, 'r') as f:
                    response = json.load(f)
                
                if response.get("id") == request_id:
                    # Clean up response
                    try:
                        os.remove(response_file)
                    except:
                        pass
                    
                    if response.get("success"):
                        return response.get("result")
                    else:
                        raise RuntimeError(f"Tool error: {response.get('error', 'unknown error')}")
            
            time.sleep(0.1)
        
        raise TimeoutError(f"Tool '{name}' timed out after {timeout}s")
    
    def log(self, message: str) -> None:
        """Log a message that will be captured."""
        print(f"[LOG] {message}")

# Initialize tools interface
mcp = MCPTools()
"#,
        );

        // Generate wrapper methods for each tool
        for tool in tools {
            sdk.push_str(&format!(
                "\ndef {}(**kwargs):\n    \"\"\"{}.\"\"\"\n    return mcp._call_tool('{}', kwargs)\n\n",
                sanitize_function_name(&tool.name), tool.description, tool.name
            ));
        }

        Ok(sdk)
    }

    /// Generate JavaScript SDK with MCP tool wrappers.
    async fn generate_javascript_sdk(&self) -> Result<String> {
        debug!("Generating JavaScript SDK for MCP tools");

        let tools = self.mcp_client
            .list_mcp_tools()
            .await
            .context("failed to list MCP tools")?;

        let mut sdk = String::from(
            r#"// MCP Tools SDK - Auto-generated
const fs = require('fs');
const path = require('path');
const { v4: uuid4 } = require('uuid');

class MCPTools {
  constructor() {
    this.callCount = 0;
    this.results = [];
    this.ipcDir = process.env.VTCODE_IPC_DIR || '/tmp/vtcode_ipc';
    if (!fs.existsSync(this.ipcDir)) {
      fs.mkdirSync(this.ipcDir, { recursive: true });
    }
  }

  async callTool(name, args = {}) {
    const requestId = uuid4();
    const request = {
      id: requestId,
      tool_name: name,
      args: args
    };

    const requestFile = path.join(this.ipcDir, 'request.json');
    fs.writeFileSync(requestFile, JSON.stringify(request, null, 2));

    // Wait for response
    const responseFile = path.join(this.ipcDir, 'response.json');
    const timeout = 30000; // 30s
    const start = Date.now();

    while (Date.now() - start < timeout) {
      try {
        if (fs.existsSync(responseFile)) {
          const response = JSON.parse(fs.readFileSync(responseFile, 'utf-8'));
          
          if (response.id === requestId) {
            // Clean up response
            try {
              fs.unlinkSync(responseFile);
            } catch (e) {}

            if (response.success) {
              return response.result;
            } else {
              throw new Error(`Tool error: ${response.error || 'unknown error'}`);
            }
          }
        }
      } catch (e) {
        if (e.code !== 'ENOENT') throw e;
      }

      await new Promise(r => setTimeout(r, 100));
    }

    throw new Error(`Tool '${name}' timed out after ${timeout}ms`);
  }

  log(message) {
    console.log(`[LOG] ${message}`);
  }
}

const mcp = new MCPTools();

"#,
        );

        // Generate wrapper functions for each tool
        for tool in tools {
            sdk.push_str(&format!(
                "async function {}(args = {{}}) {{\n  // {}\n  return await mcp.callTool('{}', args);\n}}\n\n",
                sanitize_function_name(&tool.name), tool.description, tool.name
            ));
        }

        Ok(sdk)
    }

    /// Get the workspace root path.
    pub fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    /// Get the MCP client.
    pub fn mcp_client(&self) -> &Arc<dyn McpToolExecutor> {
        &self.mcp_client
    }
}

/// Sanitize tool name to valid function name.
fn sanitize_function_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_function_name_handles_special_chars() {
        assert_eq!(sanitize_function_name("read_file"), "read_file");
        assert_eq!(sanitize_function_name("read-file"), "read_file");
        assert_eq!(sanitize_function_name("read.file"), "read_file");
        assert_eq!(sanitize_function_name("readFile123"), "readFile123");
    }

    #[test]
    fn language_as_str() {
        assert_eq!(Language::Python3.as_str(), "python3");
        assert_eq!(Language::JavaScript.as_str(), "javascript");
    }

    #[test]
    fn language_interpreter() {
        assert_eq!(Language::Python3.interpreter(), "python3");
        assert_eq!(Language::JavaScript.interpreter(), "node");
    }
}
