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
//!     sandbox_profile,
//!     Arc::new(mcp_client),
//!     Language::Python3,
//! );
//!
//! // Agent writes Python code
//! let code = r#"
//! files = search_files("*.rs", max_results=1000)
//! filtered = [f for f in files if "test" in f]
//! result = {"count": len(filtered), "files": filtered[:10]}
//! "#;
//!
//! let result = executor.execute(code).await?;
//! ```

use crate::mcp::McpToolExecutor;
use crate::sandbox::SandboxProfile;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
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
#[allow(dead_code)]
pub struct CodeExecutor {
    language: Language,
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
    /// * `code` - Code snippet to execute
    ///
    /// # Returns
    /// Execution result with output, exit code, and optional JSON result
    #[allow(unused_variables)]
    pub async fn execute(&self, code: &str) -> Result<ExecutionResult> {
        info!(
            language = self.language.as_str(),
            timeout_secs = self.config.timeout_secs,
            "Executing code snippet"
        );

        // TODO: Generate SDK wrapper with MCP tools
        // TODO: Create temporary file in sandbox
        // TODO: Execute code using PTY manager with sandbox profile
        // TODO: Parse output and extract JSON result
        // TODO: Validate resource usage

        // Placeholder: return error indicating not yet implemented
        Err(anyhow!(
            "Code execution not yet implemented. Step 2 in progress."
        ))
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
from typing import Any, Dict, Optional

class MCPTools:
    """Interface to MCP tools from agent code."""
    
    def __init__(self):
        self._call_count = 0
        self._results = []
    
    def _call_tool(self, name: str, args: Dict[str, Any]) -> Any:
        """Call an MCP tool and track execution."""
        # TODO: Implement tool invocation
        # Should use a side-channel (e.g., file-based IPC) to call tools
        raise NotImplementedError(f"Tool {name} not available")
    
    def log(self, message: str) -> None:
        """Log a message that will be captured."""
        print(f"[LOG] {message}")
    
    def set_result(self, data: Any) -> None:
        """Set the result to be returned to the agent."""
        self._results.append(data)

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
class MCPTools {
  constructor() {
    this.callCount = 0;
    this.results = [];
  }

  async callTool(name, args = {}) {
    // TODO: Implement tool invocation via side-channel
    throw new Error(`Tool ${name} not available`);
  }

  log(message) {
    console.log(`[LOG] ${message}`);
  }

  setResult(data) {
    this.results.push(data);
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
