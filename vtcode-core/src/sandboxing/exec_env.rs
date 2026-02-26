//! Command specification and execution environment types.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::SandboxPermissions;

/// Mechanism to terminate an exec invocation before it finishes naturally.
#[derive(Debug, Clone, Default)]
pub enum ExecExpiration {
    /// Timeout after a specified duration.
    Timeout(Duration),

    /// Use the default timeout.
    #[default]
    DefaultTimeout,

    /// Cancel via a cancellation token.
    Cancellation(CancellationToken),
}

impl From<Option<u64>> for ExecExpiration {
    fn from(timeout_ms: Option<u64>) -> Self {
        match timeout_ms {
            Some(ms) => Self::Timeout(Duration::from_millis(ms)),
            None => Self::DefaultTimeout,
        }
    }
}

impl From<u64> for ExecExpiration {
    fn from(timeout_ms: u64) -> Self {
        Self::Timeout(Duration::from_millis(timeout_ms))
    }
}

impl ExecExpiration {
    /// Get the timeout in milliseconds, if applicable.
    pub fn timeout_ms(&self) -> Option<u64> {
        match self {
            Self::Timeout(d) => Some(d.as_millis() as u64),
            Self::DefaultTimeout => Some(30_000), // 30 second default
            Self::Cancellation(_) => None,
        }
    }

    /// Get the timeout duration, if applicable.
    pub fn timeout_duration(&self) -> Option<Duration> {
        match self {
            Self::Timeout(d) => Some(*d),
            Self::DefaultTimeout => Some(Duration::from_secs(30)),
            Self::Cancellation(_) => None,
        }
    }
}

/// Specification for a command to be executed.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// The program to execute.
    pub program: String,

    /// Arguments to pass to the program.
    pub args: Vec<String>,

    /// Working directory for the command.
    pub cwd: PathBuf,

    /// Environment variables to set.
    pub env: HashMap<String, String>,

    /// Expiration mechanism for the command.
    pub expiration: ExecExpiration,

    /// Sandbox permissions for this command.
    pub sandbox_permissions: SandboxPermissions,

    /// Optional justification for why the command needs to run.
    pub justification: Option<String>,
}

impl Default for CommandSpec {
    fn default() -> Self {
        Self {
            program: String::new(),
            args: Vec::new(),
            cwd: PathBuf::new(),
            env: HashMap::new(),
            expiration: ExecExpiration::DefaultTimeout,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
        }
    }
}

impl CommandSpec {
    /// Create a new command specification.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Default::default()
        }
    }

    /// Add arguments to the command.
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Set the working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// Set environment variables.
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Set the expiration.
    pub fn with_expiration(mut self, expiration: ExecExpiration) -> Self {
        self.expiration = expiration;
        self
    }

    /// Set sandbox permissions.
    pub fn with_sandbox_permissions(mut self, permissions: SandboxPermissions) -> Self {
        self.sandbox_permissions = permissions;
        self
    }

    /// Set a justification.
    pub fn with_justification(mut self, justification: impl Into<String>) -> Self {
        self.justification = Some(justification.into());
        self
    }

    /// Get the full command as a vector.
    pub fn full_command(&self) -> Vec<String> {
        let mut cmd = vec![self.program.clone()];
        cmd.extend(self.args.clone());
        cmd
    }
}

/// The prepared execution environment after sandbox transformation.
#[derive(Debug, Clone)]
pub struct ExecEnv {
    /// The program to execute (may be wrapped).
    pub program: PathBuf,

    /// Arguments to the program (may include sandbox wrapper args).
    pub args: Vec<String>,

    /// Working directory.
    pub cwd: PathBuf,

    /// Environment variables.
    pub env: HashMap<String, String>,

    /// Expiration mechanism.
    pub expiration: ExecExpiration,

    /// Whether the sandbox is active.
    pub sandbox_active: bool,

    /// Type of sandbox applied.
    pub sandbox_type: SandboxType,
}

/// Type of sandbox being used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SandboxType {
    /// No sandbox applied.
    #[default]
    None,

    /// macOS Seatbelt sandbox.
    MacosSeatbelt,

    /// Linux Landlock + Seccomp sandbox.
    LinuxLandlock,

    /// Windows restricted token sandbox.
    WindowsRestrictedToken,
}

impl SandboxType {
    /// Get the platform-appropriate sandbox type.
    pub fn platform_default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacosSeatbelt
        }
        #[cfg(target_os = "linux")]
        {
            Self::LinuxLandlock
        }
        #[cfg(target_os = "windows")]
        {
            Self::WindowsRestrictedToken
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Self::None
        }
    }

    /// Check if this sandbox type is available on the current platform.
    pub fn is_available(&self) -> bool {
        match self {
            Self::None => true,
            Self::MacosSeatbelt => cfg!(target_os = "macos"),
            Self::LinuxLandlock => cfg!(target_os = "linux"),
            Self::WindowsRestrictedToken => cfg!(target_os = "windows"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_spec_builder() {
        let spec = CommandSpec::new("cat")
            .with_args(vec!["file.txt"])
            .with_cwd("/tmp")
            .with_justification("testing");

        assert_eq!(spec.program, "cat");
        assert_eq!(spec.args, vec!["file.txt"]);
        assert_eq!(spec.cwd, PathBuf::from("/tmp"));
        assert_eq!(spec.justification, Some("testing".to_string()));
    }

    #[test]
    fn test_full_command() {
        let spec = CommandSpec::new("echo").with_args(vec!["hello", "world"]);

        assert_eq!(spec.full_command(), vec!["echo", "hello", "world"]);
    }

    #[test]
    fn test_exec_expiration() {
        let timeout = ExecExpiration::Timeout(Duration::from_secs(10));
        assert_eq!(timeout.timeout_ms(), Some(10_000));

        let default = ExecExpiration::DefaultTimeout;
        assert_eq!(default.timeout_ms(), Some(30_000));
    }
}
