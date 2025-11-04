use std::fmt;
use std::path::{Path, PathBuf};

/// Supported sandbox runtime implementations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxRuntimeKind {
    /// Anthropic's `srt` runtime.
    AnthropicSrt,
    /// A Firecracker based sandbox runtime.
    Firecracker,
}

impl SandboxRuntimeKind {
    /// Returns the identifier used in persisted sandbox settings.
    pub fn as_str(&self) -> &'static str {
        match self {
            SandboxRuntimeKind::AnthropicSrt => "anthropic-srt",
            SandboxRuntimeKind::Firecracker => "firecracker",
        }
    }

    /// Parse a runtime identifier, ignoring ASCII case.
    pub fn from_identifier(identifier: &str) -> Option<Self> {
        match identifier.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "anthropic-srt" | "srt" => Some(SandboxRuntimeKind::AnthropicSrt),
            "firecracker" | "firecracker-microvm" | "fc" => Some(SandboxRuntimeKind::Firecracker),
            _ => None,
        }
    }
}

impl fmt::Display for SandboxRuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Immutable configuration for launching commands inside the sandbox runtime.
///
/// The profile bundles together the runtime binary, sandbox settings file,
/// persistent storage directory, and filesystem allowlist that should be passed
/// to the sandbox CLI when spawning a process.
///
/// ```rust
/// use std::path::PathBuf;
/// use vtcode_core::sandbox::{SandboxProfile, SandboxRuntimeKind};
///
/// let profile = SandboxProfile::new(
///     PathBuf::from("/usr/local/bin/srt"),
///     PathBuf::from("./.vtcode/sandbox/settings.json"),
///     PathBuf::from("./.vtcode/sandbox/persistent"),
///     vec![PathBuf::from("./workspace"), PathBuf::from("./.vtcode/sandbox/persistent")],
///     SandboxRuntimeKind::AnthropicSrt,
/// );
/// assert_eq!(profile.binary().ends_with("srt"), true);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxProfile {
    binary_path: PathBuf,
    settings_path: PathBuf,
    persistent_storage: PathBuf,
    allowed_paths: Vec<PathBuf>,
    runtime_kind: SandboxRuntimeKind,
}

impl SandboxProfile {
    /// Construct a new sandbox profile using the provided paths and runtime.
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

    /// Path to the sandbox runtime binary (e.g. `srt`).
    pub fn binary(&self) -> &Path {
        &self.binary_path
    }

    /// Path to the JSON settings file that configures sandbox permissions.
    pub fn settings(&self) -> &Path {
        &self.settings_path
    }

    /// Directory that persists sandbox state between executions.
    pub fn persistent_storage(&self) -> &Path {
        &self.persistent_storage
    }

    /// Filesystem locations that the sandbox runtime may access.
    pub fn allowed_paths(&self) -> &[PathBuf] {
        &self.allowed_paths
    }

    /// Runtime implementation backing this profile.
    pub fn runtime_kind(&self) -> SandboxRuntimeKind {
        self.runtime_kind
    }
}
