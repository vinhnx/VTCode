//! Command execution tool

use super::traits::{ModeTool, Tool};
use super::types::*;
use crate::config::PtyConfig;
use crate::config::constants::tools;
use crate::tools::pty::wait_status_code;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use expectrl::process::unix::WaitStatus;
use expectrl::{Eof, Error as ExpectError, Expect, Session};
use serde_json::{Value, json};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};
use tokio::task::spawn_blocking;

/// Command execution tool using standard process handling
#[derive(Clone)]
pub struct CommandTool {
    workspace_root: PathBuf,
    pty_config: PtyConfig,
}

impl CommandTool {
    pub fn new(workspace_root: PathBuf, pty_config: PtyConfig) -> Self {
        Self {
            workspace_root,
            pty_config,
        }
    }

    async fn execute_terminal_command(&self, input: &EnhancedTerminalInput) -> Result<Value> {
        if input.command.is_empty() {
            return Err(anyhow!("command array cannot be empty"));
        }

        let full_command = input.command.join(" ");
        let has_shell_metacharacters = command_requires_shell(&full_command);

        let command_parts = if has_shell_metacharacters {
            vec!["sh".to_string(), "-c".to_string(), full_command.clone()]
        } else {
            input.command.clone()
        };

        let program = command_parts
            .get(0)
            .expect("command_parts must contain at least one element")
            .clone();
        let args = command_parts[1..].to_vec();

        let work_dir = if let Some(ref working_dir) = input.working_dir {
            self.workspace_root.join(working_dir)
        } else {
            self.workspace_root.clone()
        };

        let timeout_secs = input
            .timeout_secs
            .unwrap_or(self.pty_config.command_timeout_seconds);
        if timeout_secs == 0 {
            return Err(anyhow!("timeout_secs must be greater than zero"));
        }

        let timeout = Duration::from_secs(timeout_secs);
        let rows = resolve_terminal_dimension("LINES", self.pty_config.default_rows);
        let cols = resolve_terminal_dimension("COLUMNS", self.pty_config.default_cols);
        let working_directory_display = describe_working_dir(&self.workspace_root, &work_dir);

        let program_clone = program.clone();
        let args_clone = args.clone();
        let work_dir_clone = work_dir.clone();
        let command_display = full_command.clone();

        let (stdout, exit_code, duration) = spawn_blocking(move || {
            run_command_with_expectrl(
                program_clone,
                args_clone,
                work_dir_clone,
                timeout,
                rows,
                cols,
                command_display,
            )
        })
        .await
        .context("failed to join terminal command task")??;

        Ok(json!({
            "success": exit_code == 0,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": String::new(),
            "mode": "terminal",
            "pty_enabled": true,
            "pty": {
                "rows": rows,
                "cols": cols,
            },
            "command": full_command,
            "used_shell": has_shell_metacharacters,
            "timeout_secs": timeout_secs,
            "duration_ms": duration.as_millis(),
            "working_directory": working_directory_display,
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

fn command_requires_shell(full_command: &str) -> bool {
    full_command.contains('|')
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
        || full_command.contains('}')
}

fn resolve_terminal_dimension(var: &str, fallback: u16) -> u16 {
    env::var(var)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn describe_working_dir(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_string(),
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().to_string(),
    }
}

fn run_command_with_expectrl(
    program: String,
    args: Vec<String>,
    work_dir: PathBuf,
    timeout: Duration,
    rows: u16,
    cols: u16,
    command_display: String,
) -> Result<(String, i32, Duration)> {
    let mut command = StdCommand::new(&program);
    command.args(&args);
    command.current_dir(&work_dir);
    command.env("TERM", "xterm-256color");
    command.env("COLUMNS", cols.to_string());
    command.env("LINES", rows.to_string());

    let start = Instant::now();
    let mut session = Session::spawn(command)
        .with_context(|| format!("failed to spawn PTY command '{}'", command_display))?;
    session.set_expect_timeout(Some(timeout));

    if let Err(error) = session.get_process_mut().set_window_size(cols, rows) {
        tracing::warn!(
            command = %command_display,
            error = %error,
            "failed to set PTY size for command"
        );
    }

    let output_bytes = match session.expect(Eof) {
        Ok(captures) => captures.as_bytes().to_vec(),
        Err(ExpectError::ExpectTimeout) => {
            let _ = session.get_process_mut().exit(true);
            let _ = session.get_process().wait();
            return Err(anyhow!(
                "command '{}' timed out after {}s",
                command_display,
                timeout.as_secs()
            ));
        }
        Err(ExpectError::Eof) => Vec::new(),
        Err(error) => {
            let _ = session.get_process_mut().exit(true);
            let _ = session.get_process().wait();
            return Err(anyhow!("failed to read command output: {}", error));
        }
    };

    let wait_status: WaitStatus = session
        .get_process()
        .wait()
        .context("failed to wait for PTY command to exit")?;
    let exit_code = wait_status_code(wait_status);
    let stdout = String::from_utf8_lossy(&output_bytes).to_string();

    Ok((stdout, exit_code, start.elapsed()))
}
