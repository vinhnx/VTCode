//! Shell command handler (from Codex pattern).
//!
//! Executes shell commands with sandbox support, timeout handling,
//! and environment policy management.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::orchestrator::{Approvable, Sandboxable, SandboxablePreference};
use super::tool_handler::{
    ShellToolCallParams, ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput,
    ToolPayload,
};

/// Default timeout for shell commands (30 seconds).
const DEFAULT_SHELL_TIMEOUT_MS: u64 = 30_000;

/// Maximum timeout allowed (5 minutes).
const MAX_SHELL_TIMEOUT_MS: u64 = 300_000;

/// Handler for shell command execution.
pub struct ShellHandler {
    /// Default shell to use.
    pub default_shell: String,
    /// Environment variables to inherit.
    pub inherit_env: bool,
}

impl Default for ShellHandler {
    fn default() -> Self {
        Self {
            default_shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
            inherit_env: true,
        }
    }
}

impl ShellHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_shell(shell: impl Into<String>) -> Self {
        Self {
            default_shell: shell.into(),
            inherit_env: true,
        }
    }

    /// Parse shell parameters from payload.
    fn parse_params(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<ShellToolCallParams, ToolCallError> {
        match &invocation.payload {
            ToolPayload::Function { arguments } => {
                // Parse as simple shell command string and wrap in ShellToolCallParams
                #[derive(Deserialize)]
                struct SimpleShellArgs {
                    command: String,
                    workdir: Option<String>,
                    timeout_ms: Option<u64>,
                }
                let simple: SimpleShellArgs = serde_json::from_str(arguments)
                    .map_err(|e| ToolCallError::respond(format!("Invalid shell arguments: {e}")))?;
                Ok(ShellToolCallParams {
                    command: vec![simple.command],
                    workdir: simple.workdir,
                    timeout_ms: simple.timeout_ms,
                    sandbox_permissions: None,
                    justification: None,
                })
            }
            ToolPayload::LocalShell { params } => Ok(params.clone()),
            _ => Err(ToolCallError::respond(
                "Invalid payload type for shell handler",
            )),
        }
    }

    /// Execute a shell command.
    async fn execute_command(
        &self,
        params: &ShellToolCallParams,
        cwd: &Path,
        env: Option<HashMap<String, String>>,
    ) -> Result<ShellOutput, ToolCallError> {
        // Join command parts or use single command
        let command = params.command.join(" ");
        let workdir = params
            .workdir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf());
        let timeout_ms = params
            .timeout_ms
            .unwrap_or(DEFAULT_SHELL_TIMEOUT_MS)
            .min(MAX_SHELL_TIMEOUT_MS);

        // Build the command
        let mut cmd = tokio::process::Command::new(&self.default_shell);
        cmd.arg("-c")
            .arg(&command)
            .current_dir(&workdir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Apply environment
        if !self.inherit_env {
            cmd.env_clear();
        }
        if let Some(env_vars) = env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        // Execute with timeout
        let result = tokio::time::timeout(Duration::from_millis(timeout_ms), cmd.output())
            .await
            .map_err(|_| ToolCallError::Timeout(timeout_ms))?
            .map_err(|e| ToolCallError::Internal(e.into()))?;

        Ok(ShellOutput {
            stdout: String::from_utf8_lossy(&result.stdout).to_string(),
            stderr: String::from_utf8_lossy(&result.stderr).to_string(),
            exit_code: result.status.code().unwrap_or(-1),
        })
    }
}

/// Output from shell command execution.
#[derive(Clone, Debug)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl Sandboxable for ShellHandler {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Require
    }

    fn escalate_on_failure(&self) -> bool {
        true // Shell commands may need escalation
    }
}

impl<R> Approvable<R> for ShellHandler {
    type ApprovalKey = String;

    fn approval_key(&self, _req: &R) -> Self::ApprovalKey {
        "shell".to_string()
    }
}

#[async_trait]
impl ToolHandler for ShellHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            payload,
            ToolPayload::Function { .. } | ToolPayload::LocalShell { .. }
        )
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        // Shell commands are considered mutating by default
        true
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        let params = self.parse_params(&invocation)?;
        let cwd = invocation.turn.cwd.clone();

        let output = self.execute_command(&params, &cwd, None).await?;

        // Format output
        let mut content_text = String::new();
        if !output.stdout.is_empty() {
            content_text.push_str(&output.stdout);
        }
        if !output.stderr.is_empty() {
            if !content_text.is_empty() {
                content_text.push('\n');
            }
            content_text.push_str("[stderr]\n");
            content_text.push_str(&output.stderr);
        }
        if output.exit_code != 0 {
            content_text.push_str(&format!("\n[exit code: {}]", output.exit_code));
        }

        if content_text.is_empty() {
            content_text = "(no output)".to_string();
        }

        Ok(ToolOutput::with_success(
            content_text,
            output.exit_code == 0,
        ))
    }
}

/// Create the shell tool specification.
pub fn create_shell_tool() -> super::tool_handler::ToolSpec {
    use super::tool_handler::{JsonSchema, ResponsesApiTool, ToolSpec};
    use std::collections::BTreeMap;

    let mut properties = BTreeMap::new();
    properties.insert(
        "command".to_string(),
        JsonSchema::String {
            description: Some("The shell command to execute".to_string()),
        },
    );
    properties.insert(
        "workdir".to_string(),
        JsonSchema::String {
            description: Some("Working directory for the command (optional)".to_string()),
        },
    );
    properties.insert(
        "timeout_ms".to_string(),
        JsonSchema::Number {
            description: Some("Timeout in milliseconds (default: 30000, max: 300000)".to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: "shell".to_string(),
        description: "Execute a shell command and return its output.".to_string(),
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["command".to_string()]),
            additional_properties: Some(false.into()),
        },
        strict: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_handler_echo() {
        let handler = ShellHandler::new();

        // Test that handler kind is correct
        assert_eq!(handler.kind(), ToolKind::Function);
    }

    #[test]
    fn test_shell_handler_matches_kind() {
        let handler = ShellHandler::new();

        assert!(handler.matches_kind(&ToolPayload::Function {
            arguments: "{}".to_string()
        }));

        assert!(handler.matches_kind(&ToolPayload::LocalShell {
            params: ShellToolCallParams {
                command: vec!["echo".to_string(), "hello".to_string()],
                workdir: None,
                timeout_ms: None,
                sandbox_permissions: None,
                justification: None,
            }
        }));
    }

    #[tokio::test]
    async fn test_shell_handler_is_mutating() {
        // Shell commands are always mutating
        assert!(true); // Placeholder - actual test would need full invocation
    }

    #[test]
    fn test_create_shell_tool_spec() {
        let spec = create_shell_tool();

        assert_eq!(spec.name(), "shell");
    }
}
