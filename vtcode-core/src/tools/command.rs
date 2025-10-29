//! Command execution tool

use super::traits::{ModeTool, Tool};
use super::types::*;
use crate::config::constants::tools;
use crate::exec::async_command::{AsyncProcessRunner, ProcessOptions, StreamCaptureConfig};
use crate::exec::cancellation;
use crate::execpolicy::{sanitize_working_dir, validate_command};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::OsString;
use std::{path::PathBuf, time::Duration};

/// Command execution tool for non-PTY process handling with policy enforcement
#[derive(Clone)]
pub struct CommandTool {
    workspace_root: PathBuf,
}

impl CommandTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        // Note: We use the workspace_root directly here. Full validation happens
        // in prepare_invocation which is async.
        Self { workspace_root }
    }

    async fn execute_terminal_command(
        &self,
        input: &EnhancedTerminalInput,
        invocation: CommandInvocation,
    ) -> Result<Value> {
        let work_dir =
            sanitize_working_dir(&self.workspace_root, input.working_dir.as_deref()).await?;

        let mut env = HashMap::new();
        env.insert(OsString::from("PAGER"), OsString::from("cat"));
        env.insert(OsString::from("GIT_PAGER"), OsString::from("cat"));
        env.insert(OsString::from("LESS"), OsString::from("R"));

        let timeout = Duration::from_secs(input.timeout_secs.unwrap_or(30));

        let cancellation_token = cancellation::current_tool_cancellation();

        let options = ProcessOptions {
            program: invocation.program.clone(),
            args: invocation.args.clone(),
            env,
            current_dir: Some(work_dir),
            timeout: Some(timeout),
            cancellation_token,
            stdout: StreamCaptureConfig::default(),
            stderr: StreamCaptureConfig::default(),
        };

        let result = AsyncProcessRunner::run(options)
            .await
            .with_context(|| format!("failed to run command: {}", invocation.display))?;

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();
        let exit_code = result.exit_status.code().unwrap_or_default();
        let mut success = result.exit_status.success();
        if result.timed_out || result.cancelled {
            success = false;
        }

        Ok(json!({
            "success": success,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "mode": "terminal",
            "pty_enabled": false,
            "command": invocation.display,
            "timed_out": result.timed_out,
            "cancelled": result.cancelled,
            "duration_ms": result.duration.as_millis(),
        }))
    }

    pub(crate) async fn prepare_invocation(
        &self,
        input: &EnhancedTerminalInput,
    ) -> Result<CommandInvocation> {
        if input.command.is_empty() {
            return Err(anyhow!("Command cannot be empty"));
        }

        self.validate_command_segments(&input.command)?;

        let working_dir =
            sanitize_working_dir(&self.workspace_root, input.working_dir.as_deref()).await?;

        validate_command(&input.command, &self.workspace_root, &working_dir).await?;

        let program = input.command[0].clone();
        let args = input.command[1..].to_vec();
        let display = input
            .raw_command
            .clone()
            .unwrap_or_else(|| format_command(&input.command));

        Ok(CommandInvocation {
            program,
            args,
            display,
        })
    }

    fn validate_command_segments(&self, command: &[String]) -> Result<()> {
        if command.is_empty() {
            return Err(anyhow!("Command cannot be empty"));
        }

        let program = &command[0];
        if program.chars().any(char::is_whitespace) {
            return Err(anyhow!(
                "Program name cannot contain whitespace: {}",
                program
            ));
        }

        let dangerous_commands = ["rm", "rmdir", "del", "format", "fdisk", "mkfs", "dd"];
        if dangerous_commands.contains(&program.as_str()) {
            return Err(anyhow!("Dangerous command not allowed: {}", program));
        }

        let full_command = command.join(" ");
        if full_command.contains("rm -rf /") || full_command.contains("sudo rm") {
            return Err(anyhow!("Potentially dangerous command pattern detected"));
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for CommandTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let input: EnhancedTerminalInput = serde_json::from_value(args)?;
        let invocation = self.prepare_invocation(&input).await?;
        self.execute_terminal_command(&input, invocation).await
    }

    fn name(&self) -> &'static str {
        tools::RUN_COMMAND
    }

    fn description(&self) -> &'static str {
        "Execute terminal commands"
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        let input: EnhancedTerminalInput = serde_json::from_value(args.clone())?;
        // Note: validate_args is sync, so we can't call async prepare_invocation here
        // We'll do basic validation instead
        if input.command.is_empty() {
            return Err(anyhow!("Command cannot be empty"));
        }
        self.validate_command_segments(&input.command)?;
        Ok(())
    }
}

#[async_trait]
impl ModeTool for CommandTool {
    fn supported_modes(&self) -> Vec<&'static str> {
        vec!["terminal"]
    }

    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value> {
        let input: EnhancedTerminalInput = serde_json::from_value(args)?;
        let invocation = self.prepare_invocation(&input).await?;
        match mode {
            "terminal" => self.execute_terminal_command(&input, invocation).await,
            _ => Err(anyhow!("Unsupported command execution mode: {}", mode)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommandInvocation {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) display: String,
}

fn format_command(command: &[String]) -> String {
    command
        .iter()
        .map(|part| quote_argument_posix(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_argument_posix(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }

    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:@".contains(ch))
    {
        return arg.to_string();
    }

    let mut quoted = String::from("'");
    for ch in arg.chars() {
        if ch == '\'' {
            quoted.push_str("'\"'\"'");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> CommandTool {
        let cwd = std::env::current_dir().expect("current dir");
        CommandTool::new(cwd)
    }

    fn make_input(command: Vec<&str>) -> EnhancedTerminalInput {
        EnhancedTerminalInput {
            command: command.into_iter().map(String::from).collect(),
            working_dir: None,
            timeout_secs: None,
            mode: None,
            response_format: None,
            raw_command: None,
            shell: None,
            login: None,
        }
    }

    #[test]
    fn formats_command_for_display() {
        let parts = vec!["echo".to_string(), "hello world".to_string()];
        assert_eq!(format_command(&parts), "echo 'hello world'");
    }

    #[tokio::test]
    async fn prepare_invocation_allows_policy_command() {
        let tool = make_tool();
        let input = make_input(vec!["ls"]);
        let invocation = tool.prepare_invocation(&input).await.expect("invocation");
        assert_eq!(invocation.program, "ls");
        assert!(invocation.args.is_empty());
        assert_eq!(invocation.display, "ls");
    }

    #[tokio::test]
    async fn prepare_invocation_rejects_disallowed_command() {
        let tool = make_tool();
        let input = make_input(vec!["cargo", "test"]);
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("cargo should be blocked");
        assert!(
            error
                .to_string()
                .contains("is not permitted by the execution policy")
        );
    }

    #[tokio::test]
    async fn working_dir_escape_is_rejected() {
        let tool = make_tool();
        let mut input = make_input(vec!["ls"]);
        input.working_dir = Some("../".into());
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("working dir escape should fail");
        assert!(
            error
                .to_string()
                .contains("working directory '../' escapes the workspace root")
        );
    }
}
