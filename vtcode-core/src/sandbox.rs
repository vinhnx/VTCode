use std::fmt;
use std::path::{Path, PathBuf};

/// Configuration required to launch commands inside the Anthropic sandbox runtime.
///
/// This module provides integration with Anthropic's sandbox runtime (`srt`),
/// which creates a secure execution environment for terminal commands with
/// configurable filesystem and network permissions.
///
/// # Features
///
/// The sandbox runtime provides:
/// - **Filesystem Isolation**: Commands can only access files within the project workspace
/// - **Network Control**: Domain-based allowlist for outbound network requests  
/// - **Security**: Prevention of access to sensitive system locations
/// - **Integration**: Seamless integration with VT Code's tool execution system
///
/// # Usage
///
/// The `SandboxProfile` is used by the bash runner to execute commands in the
/// sandboxed environment when enabled. It contains paths to the sandbox binary
/// and settings file that define the security policies.
///
/// Example usage in tool implementations:
///
/// ```rust,ignore
/// // This would be used within a tool implementation
/// if let Some(profile) = &sandbox_profile {
///     // Execute command in sandbox
///     let output = run_in_sandbox(profile.binary(), profile.settings(), command)?;
/// } else {
///     // Execute command normally
///     let output = run_command(command)?;
/// }
/// ```
///
/// # Security Model
///
/// The sandbox runtime implements the following security measures:
/// - Default deny for filesystem access (only workspace directory accessible)
/// - Default deny for network access (requires explicit domain allowlist)
/// - Prevention of access to sensitive system directories like `~/.ssh`, `/etc/ssh`, etc.
///
/// This approach ensures that AI agents running in VT Code operate within a secure
/// boundary that protects both the user's system and sensitive data.
///
/// This is a lightweight holder for the sandbox CLI binary (`srt`) and the
/// resolved settings file that encodes filesystem and network policies. Tool
/// implementations clone this struct and translate regular command invocations
/// into sandboxed executions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxRuntimeKind {
    AnthropicSrt,
    Firecracker,
}

impl SandboxRuntimeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SandboxRuntimeKind::AnthropicSrt => "anthropic-srt",
            SandboxRuntimeKind::Firecracker => "firecracker",
        }
    }
}

impl fmt::Display for SandboxRuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxProfile {
    binary_path: PathBuf,
    settings_path: PathBuf,
    persistent_storage: PathBuf,
    allowed_paths: Vec<PathBuf>,
    runtime_kind: SandboxRuntimeKind,
}

impl SandboxProfile {
    /// Create a new sandbox profile using the provided binary and settings paths.
    ///
    /// # Arguments
    ///
    /// * `binary_path` - Path to the runtime binary (Anthropic `srt` or Firecracker launcher)
    /// * `settings_path` - Path to the JSON settings file containing sandbox permissions
    /// * `persistent_storage` - Directory that persists sandbox state between executions
    /// * `allowed_paths` - Filesystem locations whitelisted for sandbox access
    /// * `runtime_kind` - Runtime implementation powering the sandbox
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::path::PathBuf;
    /// use vtcode_core::sandbox::{SandboxProfile, SandboxRuntimeKind};
    ///
    /// let binary_path = PathBuf::from("/usr/local/bin/srt");
    /// let settings_path = PathBuf::from("./.vtcode/sandbox/settings.json");
    /// let persistent_storage = PathBuf::from("./.vtcode/sandbox/persistent");
    /// let allowed_paths = vec![PathBuf::from("./workspace"), persistent_storage.clone()];
    /// let profile = SandboxProfile::new(
    ///     binary_path,
    ///     settings_path,
    ///     persistent_storage,
    ///     allowed_paths,
    ///     SandboxRuntimeKind::AnthropicSrt,
    /// );
    /// ```
    pub fn new(
        binary_path: PathBuf,
        settings_path: PathBuf,
        persistent_storage: PathBuf,
        allowed_paths: Vec<PathBuf>,
        runtime_kind: SandboxRuntimeKind,
    ) -> Self {
        Self {
            binary_path,
            settings_path,
            persistent_storage,
            allowed_paths,
            runtime_kind,
        }
    }

    /// Path to the sandbox CLI (`srt`).
    ///
    /// Returns a reference to the path where the Anthropic sandbox runtime
    /// binary is located. This is typically just the command `srt` if it's
    /// in the system PATH.
    pub fn binary(&self) -> &Path {
        &self.binary_path
    }

    /// Path to the JSON settings file that configures sandbox permissions.
    ///
    /// This file contains the allow and deny rules for filesystem access and
    /// network permissions that the sandbox will enforce during command execution.
    pub fn settings(&self) -> &Path {
        &self.settings_path
    }

    /// Path to the directory that persists sandbox state between executions.
    pub fn persistent_storage(&self) -> &Path {
        &self.persistent_storage
    }

    /// Filesystem locations that the sandbox runtime is permitted to access.
    pub fn allowed_paths(&self) -> &[PathBuf] {
        &self.allowed_paths
    }

    /// Runtime implementation backing this sandbox profile.
    pub fn runtime_kind(&self) -> SandboxRuntimeKind {
        self.runtime_kind
    }
}
