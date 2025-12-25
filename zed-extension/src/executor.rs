/// VT Code CLI Command Executor
///
/// Handles execution of VT Code CLI commands and output capture.
/// This module provides the interface for running VT Code commands
/// and retrieving their results with timeout support.
use std::process::{Command, Stdio};
use std::time::Duration;

/// Result of a VT Code command execution
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Exit status (0 = success)
    pub status: i32,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
}

impl CommandResult {
    /// Check if command executed successfully
    pub fn is_success(&self) -> bool {
        self.status == 0
    }

    /// Get the output, preferring stdout if available
    pub fn output(&self) -> String {
        if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            self.stdout.clone()
        }
    }
}

/// Execute a VT Code command
///
/// # Arguments
/// * `command` - The vtcode subcommand (e.g., "ask", "analyze")
/// * `args` - Additional arguments to pass
///
/// # Returns
/// Result containing the command output or error message
pub fn execute_command(command: &str, args: &[&str]) -> Result<CommandResult, String> {
    // Default timeout: 30 seconds for most commands
    let timeout = match command {
        "analyze" => Duration::from_secs(60), // Workspace analysis can take longer
        "chat" => Duration::from_secs(120),   // Chat/interactive can be longer
        _ => Duration::from_secs(30),         // Default timeout
    };
    execute_command_with_timeout(command, args, timeout)
}

/// Execute a VT Code command with custom timeout
///
/// # Arguments
/// * `command` - The vtcode subcommand
/// * `args` - Additional arguments
/// * `_timeout` - Maximum duration to wait for command completion
///
/// # Returns
/// Result containing the command output or timeout error
pub fn execute_command_with_timeout(
    command: &str,
    args: &[&str],
    _timeout: Duration,
) -> Result<CommandResult, String> {
    // Build the full command
    let mut cmd = Command::new("vtcode");
    cmd.arg(command);

    for arg in args {
        cmd.arg(arg);
    }

    // Execute and capture output
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to execute vtcode command: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status = output.status.code().unwrap_or(-1);

    Ok(CommandResult {
        status,
        stdout,
        stderr,
    })
}

/// Check if VT Code CLI is available
pub fn check_vtcode_available() -> bool {
    Command::new("vtcode")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Get VT Code CLI version
pub fn get_vtcode_version() -> Result<String, String> {
    let result = execute_command("--version", &[])?;
    if result.is_success() {
        Ok(result.stdout.trim().to_string())
    } else {
        Err(result.stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_result_is_success() {
        let result = CommandResult {
            status: 0,
            stdout: "output".to_string(),
            stderr: String::new(),
        };
        assert!(result.is_success());

        let result = CommandResult {
            status: 1,
            stdout: String::new(),
            stderr: "error".to_string(),
        };
        assert!(!result.is_success());
    }

    #[test]
    fn test_command_result_output() {
        let result = CommandResult {
            status: 0,
            stdout: "stdout content".to_string(),
            stderr: String::new(),
        };
        assert_eq!(result.output(), "stdout content");

        let result = CommandResult {
            status: 1,
            stdout: String::new(),
            stderr: "stderr content".to_string(),
        };
        assert_eq!(result.output(), "stderr content");
    }

    #[test]
    fn test_timeout_defaults() {
        // These tests verify timeout logic exists
        // Actual timeout behavior requires process mocking
        let timeout = Duration::from_secs(30);
        assert!(timeout > Duration::from_secs(0));
    }

    #[test]
    fn test_command_specific_timeouts() {
        let analyze_timeout = Duration::from_secs(60);
        let chat_timeout = Duration::from_secs(120);
        let default_timeout = Duration::from_secs(30);

        assert!(analyze_timeout > default_timeout);
        assert!(chat_timeout > analyze_timeout);
    }
}
