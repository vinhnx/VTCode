use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use async_process::{Command, Stdio};

/// Description of a process to execute asynchronously.
pub struct ProcessRequest<'a> {
    /// Binary or script to execute.
    pub program: &'a str,
    /// Arguments passed to the program.
    pub args: &'a [String],
    /// Human-readable representation of the command, used for diagnostics.
    pub display: &'a str,
    /// Optional working directory for the process.
    pub current_dir: Option<&'a Path>,
    /// Timeout applied to the command execution.
    pub timeout: Duration,
}

/// Output collected from a finished process.
pub struct ProcessOutput {
    /// Whether the process exited successfully.
    pub success: bool,
    /// Exit code returned by the process. `-1` indicates termination without a code.
    pub exit_code: i32,
    /// Captured stdout data.
    pub stdout: String,
    /// Captured stderr data.
    pub stderr: String,
}

/// Execute a command asynchronously using the runtime-agnostic `async-process` crate.
pub async fn run_process(request: ProcessRequest<'_>) -> Result<ProcessOutput> {
    let mut command = Command::new(request.program);
    if !request.args.is_empty() {
        command.args(request.args);
    }
    if let Some(dir) = request.current_dir {
        command.current_dir(dir);
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let display = if request.display.is_empty() {
        format_command_label(request.program, request.args)
    } else {
        request.display.to_string()
    };

    let output = tokio::time::timeout(request.timeout, command.output())
        .await
        .with_context(|| {
            format!(
                "command '{}' timed out after {}s",
                display,
                request.timeout.as_secs()
            )
        })?
        .with_context(|| format!("failed to execute command '{}'", display))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(ProcessOutput {
        success: output.status.success(),
        exit_code,
        stdout,
        stderr,
    })
}

fn format_command_label(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        return program.to_string();
    }

    let mut segments = Vec::with_capacity(args.len() + 1);
    segments.push(program.to_string());
    segments.extend(args.iter().cloned());
    segments.join(" ")
}
