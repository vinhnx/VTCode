//! Debug utilities for testing sandbox configurations.
//!
//! Following the Codex pattern: "codex debug seatbelt and codex debug landlock
//! let you test arbitrary commands through the sandbox."

use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::{CommandSpec, SandboxManager, SandboxPolicy, SandboxType};

/// Result of a sandbox debug test.
#[derive(Debug)]
pub struct SandboxDebugResult {
    /// Whether the command succeeded.
    pub success: bool,
    /// Exit code if available.
    pub exit_code: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// The sandbox type used.
    pub sandbox_type: SandboxType,
    /// Whether the sandbox was actually applied.
    pub sandbox_active: bool,
}

impl SandboxDebugResult {
    /// Create a result indicating sandbox is not available.
    pub fn unavailable(sandbox_type: SandboxType) -> Self {
        Self {
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: format!(
                "Sandbox type {:?} is not available on this platform",
                sandbox_type
            ),
            sandbox_type,
            sandbox_active: false,
        }
    }
}

/// Debug sandbox configuration by running a test command.
///
/// This allows testing sandbox restrictions without affecting production execution.
pub async fn debug_sandbox(
    sandbox_type: SandboxType,
    policy: &SandboxPolicy,
    command: &[String],
    cwd: &Path,
    sandbox_executable: Option<&Path>,
) -> Result<SandboxDebugResult> {
    if !sandbox_type.is_available() {
        return Ok(SandboxDebugResult::unavailable(sandbox_type));
    }

    if command.is_empty() {
        anyhow::bail!("Command cannot be empty");
    }

    let spec = CommandSpec::new(&command[0])
        .with_args(command[1..].to_vec())
        .with_cwd(cwd);

    let manager = SandboxManager::new();
    let exec_env = manager
        .transform(spec, policy, cwd, sandbox_executable)
        .context("Failed to transform command for sandbox")?;

    let mut cmd = Command::new(&exec_env.program);
    cmd.args(&exec_env.args)
        .current_dir(&exec_env.cwd)
        .envs(&exec_env.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .context("Failed to execute sandboxed command")?;

    Ok(SandboxDebugResult {
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        sandbox_type: exec_env.sandbox_type,
        sandbox_active: exec_env.sandbox_active,
    })
}

/// Test if a specific path is writable under the given sandbox policy.
pub async fn test_path_writable(
    policy: &SandboxPolicy,
    test_path: &Path,
    cwd: &Path,
    sandbox_executable: Option<&Path>,
) -> Result<bool> {
    let test_file = test_path.join(".vtcode_sandbox_test");
    let test_command = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "touch '{}' && rm -f '{}'",
            test_file.display(),
            test_file.display()
        ),
    ];

    let result = debug_sandbox(
        SandboxType::platform_default(),
        policy,
        &test_command,
        cwd,
        sandbox_executable,
    )
    .await?;

    Ok(result.success)
}

/// Test if network access is blocked under the given sandbox policy.
pub async fn test_network_blocked(
    policy: &SandboxPolicy,
    cwd: &Path,
    sandbox_executable: Option<&Path>,
) -> Result<bool> {
    let test_command = vec![
        "sh".to_string(),
        "-c".to_string(),
        "curl -s --connect-timeout 2 https://example.com > /dev/null 2>&1".to_string(),
    ];

    let result = debug_sandbox(
        SandboxType::platform_default(),
        policy,
        &test_command,
        cwd,
        sandbox_executable,
    )
    .await?;

    Ok(!result.success)
}

/// Get a human-readable summary of sandbox capabilities for the current platform.
pub fn sandbox_capabilities_summary() -> String {
    let mut summary = String::new();

    summary.push_str("VT Code Sandbox Capabilities\n");
    summary.push_str("=============================\n\n");

    summary.push_str(&format!(
        "Platform default: {:?}\n\n",
        SandboxType::platform_default()
    ));

    summary.push_str("Available sandbox types:\n");
    for sandbox_type in [
        SandboxType::MacosSeatbelt,
        SandboxType::LinuxLandlock,
        SandboxType::WindowsRestrictedToken,
    ] {
        let available = if sandbox_type.is_available() {
            "✓"
        } else {
            "✗"
        };
        summary.push_str(&format!("  {} {:?}\n", available, sandbox_type));
    }

    summary.push_str("\nSandbox policies:\n");
    summary.push_str("  - ReadOnly: Read files, no writes except /dev/null, no network\n");
    summary
        .push_str("  - WorkspaceWrite: Read all, write to workspace, optional network allowlist\n");
    summary.push_str("  - DangerFullAccess: No restrictions (use with caution)\n");

    summary.push_str("\nSecurity features:\n");
    summary.push_str("  - Sensitive path blocking (~/.ssh, ~/.aws, etc.)\n");
    summary.push_str("  - .git directory write protection\n");
    summary.push_str("  - Environment variable sanitization\n");
    summary.push_str("  - Seccomp syscall filtering (Linux)\n");
    summary.push_str("  - Resource limits (memory, PIDs, disk, CPU)\n");

    summary
}

/// Debug subcommand types for CLI integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugSubcommand {
    /// Test macOS Seatbelt sandbox.
    Seatbelt,
    /// Test Linux Landlock sandbox.
    Landlock,
    /// Show sandbox capabilities.
    Capabilities,
}

impl DebugSubcommand {
    /// Get the sandbox type for this debug subcommand.
    pub fn sandbox_type(&self) -> SandboxType {
        match self {
            Self::Seatbelt => SandboxType::MacosSeatbelt,
            Self::Landlock => SandboxType::LinuxLandlock,
            Self::Capabilities => SandboxType::platform_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_summary() {
        let summary = sandbox_capabilities_summary();
        assert!(summary.contains("VT Code Sandbox Capabilities"));
        assert!(summary.contains("Platform default"));
        assert!(summary.contains("ReadOnly"));
        assert!(summary.contains("WorkspaceWrite"));
    }

    #[test]
    fn test_debug_subcommand() {
        assert_eq!(
            DebugSubcommand::Seatbelt.sandbox_type(),
            SandboxType::MacosSeatbelt
        );
        assert_eq!(
            DebugSubcommand::Landlock.sandbox_type(),
            SandboxType::LinuxLandlock
        );
    }

    #[tokio::test]
    async fn test_debug_sandbox_unavailable() {
        let result = SandboxDebugResult::unavailable(SandboxType::LinuxLandlock);
        assert!(!result.success);
        assert!(!result.sandbox_active);
        assert!(result.stderr.contains("not available"));
    }
}
