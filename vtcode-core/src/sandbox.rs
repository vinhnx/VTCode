use std::path::{Path, PathBuf};

/// Configuration required to launch commands inside the Anthropic sandbox runtime.
///
/// This is a lightweight holder for the sandbox CLI binary (`srt`) and the
/// resolved settings file that encodes filesystem and network policies. Tool
/// implementations clone this struct and translate regular command invocations
/// into sandboxed executions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxProfile {
    binary_path: PathBuf,
    settings_path: PathBuf,
}

impl SandboxProfile {
    /// Create a new sandbox profile using the provided binary and settings paths.
    pub fn new(binary_path: PathBuf, settings_path: PathBuf) -> Self {
        Self {
            binary_path,
            settings_path,
        }
    }

    /// Path to the sandbox CLI (`srt`).
    pub fn binary(&self) -> &Path {
        &self.binary_path
    }

    /// Path to the JSON settings file that configures sandbox permissions.
    pub fn settings(&self) -> &Path {
        &self.settings_path
    }
}
