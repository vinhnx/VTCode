//! Command execution tool

use super::traits::{ModeTool, Tool};
use super::types::*;
use crate::config::CommandsConfig;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

use crate::utils::process::{ProcessRequest, run_process};
use shell_words::split;

/// Command execution tool using standard process handling
#[derive(Clone)]
pub struct CommandTool {
    workspace_root: PathBuf,
    commands_config: CommandsConfig,
}

impl CommandTool {
    pub fn new(workspace_root: PathBuf, commands_config: CommandsConfig) -> Self {
        Self {
            workspace_root,
            commands_config,
        }
    }

    async fn execute_terminal_command(&self, input: &EnhancedTerminalInput) -> Result<Value> {
        let command_parts = Self::normalize_command_parts(&input.command)?;
        Self::validate_normalized_command(&command_parts)?;

        // Check if command contains shell metacharacters that require shell interpretation
        let full_command = command_parts.join(" ");
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
            let (first, rest) = command_parts
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
        let command_str = full_command;
        let output = run_process(ProcessRequest {
            program: &program,
            args: &args,
            display: &command_str,
            current_dir: Some(work_dir.as_path()),
            timeout: duration,
            stdin: None,
            max_stdout_bytes: self.commands_config.max_stdout_bytes,
            max_stderr_bytes: self.commands_config.max_stderr_bytes,
        })
        .await?;

        Ok(json!({
            "success": output.success,
            "exit_code": output.exit_code,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "stdout_bytes": output.stdout_bytes,
            "stderr_bytes": output.stderr_bytes,
            "stdout_truncated": output.stdout_truncated,
            "stderr_truncated": output.stderr_truncated,
            "timed_out": output.timed_out,
            "duration_ms": output.duration.as_millis(),
            "mode": "terminal",
            "pty_enabled": false,
            "command": command_str,
            "used_shell": has_shell_metacharacters
        }))
    }

    fn normalize_command_parts(command: &[String]) -> Result<Vec<String>> {
        if command.is_empty() {
            return Err(anyhow!("command array cannot be empty"));
        }

        if command.len() == 1 {
            let raw = command[0].trim();
            if raw.is_empty() {
                return Err(anyhow!("command cannot be empty"));
            }

            if raw.chars().any(char::is_whitespace) {
                let parts = split(raw)
                    .with_context(|| format!("failed to parse command string '{}'", raw))?;
                if parts.is_empty() {
                    return Err(anyhow!("command cannot be empty"));
                }
                return Ok(parts);
            }
        }

        Ok(command.to_vec())
    }

    fn validate_normalized_command(command: &[String]) -> Result<()> {
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
        let command_parts = Self::normalize_command_parts(&input.command)?;
        Self::validate_normalized_command(&command_parts)
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
