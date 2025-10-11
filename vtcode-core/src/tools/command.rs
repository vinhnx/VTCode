//! Command execution tool

use super::traits::{ModeTool, Tool};
use super::types::*;
use crate::config::constants::tools;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

use crate::utils::process::{ProcessRequest, run_process};

/// Command execution tool using standard process handling
#[derive(Clone)]
pub struct CommandTool {
    workspace_root: PathBuf,
}

impl CommandTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    async fn execute_terminal_command(&self, input: &EnhancedTerminalInput) -> Result<Value> {
        if input.command.is_empty() {
            return Err(anyhow!("command array cannot be empty"));
        }

        // Check if command contains shell metacharacters that require shell interpretation
        let full_command = input.command.join(" ");
        let has_shell_metacharacters = full_command.contains('|')
            || full_command.contains('>')
            || full_command.contains('<')
            || full_command.contains('&')
            || full_command.contains(';')
            || full_command.contains('(')
            || full_command.contains(')')
            || full_command.contains('$')
            || full_command.contains('`')
            || full_command.contains('*')
            || full_command.contains('?')
            || full_command.contains('[')
            || full_command.contains(']')
            || full_command.contains('{')
            || full_command.contains('}');

        let (program, args): (String, Vec<String>) = if has_shell_metacharacters {
            // Use shell to interpret metacharacters
            (
                "sh".to_string(),
                vec!["-c".to_string(), full_command.clone()],
            )
        } else {
            // Execute directly
            let (first, rest) = input
                .command
                .split_first()
                .ok_or_else(|| anyhow!("command array cannot be empty"))?;
            (first.clone(), rest.to_vec())
        };

        let work_dir = if let Some(ref working_dir) = input.working_dir {
            self.workspace_root.join(working_dir)
        } else {
            self.workspace_root.clone()
        };

        let duration = Duration::from_secs(input.timeout_secs.unwrap_or(30));
        let command_str = input.command.join(" ");
        let output = run_process(ProcessRequest {
            program: &program,
            args: &args,
            display: &command_str,
            current_dir: Some(work_dir.as_path()),
            timeout: duration,
        })
        .await?;

        Ok(json!({
            "success": output.success,
            "exit_code": output.exit_code,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "mode": "terminal",
            "pty_enabled": false,
            "command": command_str,
            "used_shell": has_shell_metacharacters
        }))
    }

    fn validate_command(&self, command: &[String]) -> Result<()> {
        if command.is_empty() {
            return Err(anyhow!("Command cannot be empty"));
        }

        let program = &command[0];
        let full_command = command.join(" ");

        // If this is a shell command (sh -c), validate the actual command being executed
        if program == "sh" && command.len() >= 3 && command[1] == "-c" {
            let actual_command = &command[2];

            // Check for extremely dangerous patterns even in shell commands
            if actual_command.contains("rm -rf /")
                || actual_command.contains("sudo rm")
                || actual_command.contains("format")
                || actual_command.contains("fdisk")
                || actual_command.contains("mkfs")
            {
                return Err(anyhow!(
                    "Potentially dangerous command pattern detected in shell command"
                ));
            }

            return Ok(());
        }

        // For direct commands, check the program name
        let dangerous_commands = ["rm", "rmdir", "del", "format", "fdisk", "mkfs", "dd"];
        if dangerous_commands.contains(&program.as_str()) {
            return Err(anyhow!("Dangerous command not allowed: {}", program));
        }

        // Check for dangerous patterns in the full command
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
        self.validate_command(&input.command)?;
        self.execute_terminal_command(&input).await
    }

    fn name(&self) -> &'static str {
        tools::RUN_TERMINAL_CMD
    }

    fn description(&self) -> &'static str {
        "Execute terminal commands"
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        let input: EnhancedTerminalInput = serde_json::from_value(args.clone())?;
        self.validate_command(&input.command)
    }
}

#[async_trait]
impl ModeTool for CommandTool {
    fn supported_modes(&self) -> Vec<&'static str> {
        vec!["terminal"]
    }

    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value> {
        let input: EnhancedTerminalInput = serde_json::from_value(args)?;
        match mode {
            "terminal" => self.execute_terminal_command(&input).await,
            _ => Err(anyhow!("Unsupported command execution mode: {}", mode)),
        }
    }
}
