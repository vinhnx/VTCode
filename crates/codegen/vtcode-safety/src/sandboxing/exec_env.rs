//! Command specification and execution environment types.

use hashbrown::HashMap;
use std::ffi::OsString;
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
            Self::Timeout(d) => Some(u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
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
    pub(crate) program: OsString,

    /// Arguments to pass to the program.
    pub(crate) args: Vec<String>,

    /// Working directory for the command.
    pub(crate) cwd: PathBuf,

    /// Environment variables to set.
    pub(crate) env: HashMap<String, String>,

    /// Expiration mechanism for the command.
    pub(crate) expiration: ExecExpiration,

    /// Sandbox permissions for this command.
    sandbox_permissions: SandboxPermissions,

    /// Optional justification for why the command needs to run.
    justification: Option<String>,
}

impl Default for CommandSpec {
    fn default() -> Self {
        Self {
            program: OsString::new(),
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
    pub fn new(program: impl Into<OsString>) -> Self {
        Self { program: program.into(), ..Default::default() }
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
    fn with_justification(mut self, justification: impl Into<String>) -> Self {
        self.justification = Some(justification.into());
        self
    }

    /// Get the full command as a vector.
    fn full_command(&self) -> Vec<OsString> {
        let mut cmd = vec![self.program.clone()];
        cmd.extend(self.args.iter().cloned().map(OsString::from));
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

/// How the Linux sandbox helper binary is invoked.
///
/// The default helper is the `vtcode` binary itself (busybox pattern): the
/// launcher subcommand token must be prepended before the helper-protocol
/// flags. An external helper configured via `VTCODE_LINUX_SANDBOX_EXECUTABLE`
/// receives the protocol flags directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxSandboxLauncher {
    /// The helper program to execute.
    pub program: PathBuf,
    /// Arguments inserted before the helper-protocol flags (e.g. the
    /// `sandbox-exec` subcommand token for busybox dispatch).
    pub prefix_args: Vec<String>,
}

impl LinuxSandboxLauncher {
    /// An external helper binary invoked as `<helper> --sandbox-policy …`.
    pub fn external(helper: PathBuf) -> Self {
        Self { program: helper, prefix_args: Vec::new() }
    }

    /// VT Code itself, dispatched to its hidden `sandbox-exec` subcommand.
    pub fn busybox(binary: PathBuf) -> Self {
        Self {
            program: binary,
            prefix_args: vec!["sandbox-exec".to_string()],
        }
    }

    /// Resolve the Linux sandbox helper: an explicit
    /// `VTCODE_LINUX_SANDBOX_EXECUTABLE` override is invoked directly;
    /// otherwise VT Code itself is the helper via busybox dispatch.
    #[cfg(target_os = "linux")]
    pub fn resolve() -> Option<Self> {
        if let Some(helper) = std::env::var_os("VTCODE_LINUX_SANDBOX_EXECUTABLE") {
            let helper = std::path::PathBuf::from(helper);
            // A helper named `vtcode` speaks the busybox protocol (dispatch to
            // the hidden sandbox-exec subcommand); anything else is an
            // external helper with its own invocation contract.
            if helper.file_stem().is_some_and(|stem| stem == "vtcode") {
                return Some(Self::busybox(helper));
            }
            return Some(Self::external(helper));
        }
        // Wrapping with the running binary is only valid when that binary is
        // the VT Code CLI, which owns the sandbox-exec launcher protocol.
        // Under a test harness the current executable is a libtest binary
        // that cannot parse launcher flags, so only the CLI binary qualifies.
        let exe = std::env::current_exe().ok()?;
        if exe.file_stem().is_some_and(|stem| stem == "vtcode") {
            return Some(Self::busybox(exe));
        }
        None
    }

    #[cfg(not(target_os = "linux"))]
    pub fn resolve() -> Option<Self> {
        None
    }
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
    pub(crate) fn platform_default() -> Self {
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
    ///
    /// "Available" means the sandbox is **actually enforced**, not just that
    /// the type compiles on this platform. `WindowsRestrictedToken` is not
    /// yet implemented (the transform is a no-op pass-through), so it reports
    /// `false` on all platforms — callers who request a restrictive policy on
    /// Windows get an `UnavailableSandboxType` error instead of silently
    /// running unsandboxed.
    pub(crate) fn is_available(&self) -> bool {
        match self {
            Self::None => true,
            Self::MacosSeatbelt => cfg!(target_os = "macos"),
            // Available only when the kernel actually enforces Landlock
            // (Linux 5.13+); older kernels fail closed.
            #[cfg(target_os = "linux")]
            Self::LinuxLandlock => super::linux::landlock_supported(),
            #[cfg(not(target_os = "linux"))]
            Self::LinuxLandlock => false,
            // Not yet implemented — `transform_windows` is a pass-through.
            // Returning `false` here causes fail-closed behavior: requesting
            // a ReadOnly/WorkspaceWrite policy on Windows yields an explicit
            // error rather than silently running without a sandbox.
            Self::WindowsRestrictedToken => false,
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

        assert_eq!(spec.program, OsString::from("cat"));
        assert_eq!(spec.args, vec!["file.txt"]);
        assert_eq!(spec.cwd, PathBuf::from("/tmp"));
        assert_eq!(spec.justification, Some("testing".to_string()));
    }

    #[test]
    fn test_full_command() {
        let spec = CommandSpec::new("echo").with_args(vec!["hello", "world"]);

        assert_eq!(spec.full_command(), vec![OsString::from("echo"), OsString::from("hello"), OsString::from("world")]);
    }

    #[test]
    fn test_command_spec_accepts_path_backed_program() {
        let program = PathBuf::from("/tmp/example-program");
        let spec = CommandSpec::new(program.clone());

        assert_eq!(spec.program, program.into_os_string());
    }

    #[test]
    fn test_exec_expiration() {
        let timeout = ExecExpiration::Timeout(Duration::from_secs(10));
        assert_eq!(timeout.timeout_ms(), Some(10_000));

        let default = ExecExpiration::DefaultTimeout;
        assert_eq!(default.timeout_ms(), Some(30_000));
    }
}
