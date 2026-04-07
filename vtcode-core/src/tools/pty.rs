//! PTY session management.
//!
//! When the `tui` feature is enabled, the full PTY implementation is compiled
//! in from submodules.  When disabled, a no-op `PtyManager` stub and shared
//! types are provided so callers compile without feature-gating every call.

// ── Submodules (TUI only) ───────────────────────────────────────────────────

#[cfg(feature = "tui")]
mod command_utils;
#[cfg(feature = "tui")]
mod formatting;
#[cfg(feature = "tui")]
mod manager;
#[cfg(feature = "tui")]
mod manager_utils;
#[cfg(feature = "tui")]
mod preview;
#[cfg(feature = "tui")]
mod raw_vt_buffer;
#[cfg(feature = "tui")]
mod screen_backend;
#[cfg(feature = "tui")]
mod scrollback;
#[cfg(feature = "tui")]
mod session;
#[cfg(feature = "tui")]
mod types;

// ── Re-exports (TUI) ───────────────────────────────────────────────────────

#[cfg(feature = "tui")]
pub use command_utils::{
    is_cargo_command, is_cargo_command_string, is_development_toolchain_command,
};
#[cfg(feature = "tui")]
pub use manager::PtyManager;
#[cfg(feature = "tui")]
pub use portable_pty::PtySize;
#[cfg(feature = "tui")]
pub use preview::PtyPreviewRenderer;
#[cfg(feature = "tui")]
pub use types::{PtyCommandRequest, PtyCommandResult, PtyOutputCallback};

// ── Shared types (headless) ─────────────────────────────────────────────────

#[cfg(not(feature = "tui"))]
mod headless_pty {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use anyhow::{Context, Result, anyhow};
    use hashbrown::HashMap;

    use crate::config::CommandsConfig;
    use crate::config::PtyConfig;
    use crate::tools::types::VTCodePtySession;
    use crate::utils::path::ensure_path_within_workspace;
    use crate::zsh_exec_bridge::ZshExecBridgeSession;

    pub use portable_pty::PtySize;

    pub type PtyOutputCallback = Arc<dyn Fn(&str) + Send + Sync>;

    pub struct PtyCommandRequest {
        pub command: Vec<String>,
        pub working_dir: PathBuf,
        pub timeout: Duration,
        pub size: PtySize,
        pub max_tokens: Option<usize>,
        pub output_callback: Option<PtyOutputCallback>,
    }

    impl PtyCommandRequest {
        pub fn with_streaming(
            command: Vec<String>,
            working_dir: PathBuf,
            timeout: Duration,
            callback: PtyOutputCallback,
        ) -> Self {
            Self {
                command,
                working_dir,
                timeout,
                size: PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                max_tokens: None,
                output_callback: Some(callback),
            }
        }
    }

    pub struct PtyCommandResult {
        pub exit_code: i32,
        pub output: String,
        pub duration: Duration,
        pub size: PtySize,
        pub applied_max_tokens: Option<usize>,
    }

    /// No-op PTY manager for headless builds.
    #[derive(Clone, Default)]
    pub struct PtyManager {
        workspace_root: PathBuf,
        _config: PtyConfig,
    }

    impl PtyManager {
        pub fn new(workspace_root: PathBuf, config: PtyConfig) -> Self {
            Self {
                workspace_root,
                _config: config,
            }
        }

        pub async fn resolve_working_dir(&self, working_dir: Option<&str>) -> Result<PathBuf> {
            let requested = match working_dir {
                Some(dir) if !dir.trim().is_empty() => dir.trim(),
                _ => return Ok(self.workspace_root.clone()),
            };

            let candidate = self.workspace_root.join(requested);
            let normalized = ensure_path_within_workspace(&candidate, &self.workspace_root)
                .map_err(|_| {
                    anyhow!(
                        "Working directory '{}' escapes the workspace root",
                        candidate.display()
                    )
                })?;
            let metadata = tokio::fs::metadata(&normalized).await.with_context(|| {
                format!(
                    "Working directory '{}' does not exist",
                    normalized.display()
                )
            })?;
            if !metadata.is_dir() {
                return Err(anyhow!(
                    "Working directory '{}' is not a directory",
                    normalized.display()
                ));
            }
            Ok(normalized)
        }

        pub fn apply_commands_config(&self, _commands_config: &CommandsConfig) {}

        pub(crate) fn create_session_with_bridge(
            &self,
            _session_id: String,
            _command: Vec<String>,
            _working_dir: PathBuf,
            _size: PtySize,
            _extra_env: HashMap<String, String>,
            _zsh_exec_bridge: Option<ZshExecBridgeSession>,
        ) -> Result<VTCodePtySession> {
            Err(anyhow!("PTY support disabled in headless build"))
        }

        pub fn snapshot_session(&self, _session_id: &str) -> Result<VTCodePtySession> {
            Err(anyhow!("PTY support disabled in headless build"))
        }

        pub fn read_session_output(
            &self,
            _session_id: &str,
            _drain: bool,
        ) -> Result<Option<String>> {
            Err(anyhow!("PTY support disabled in headless build"))
        }

        pub fn send_input_to_session(
            &self,
            _session_id: &str,
            _data: &[u8],
            _append_newline: bool,
        ) -> Result<usize> {
            Err(anyhow!("PTY support disabled in headless build"))
        }

        pub fn is_session_completed(&self, _session_id: &str) -> Result<Option<i32>> {
            Err(anyhow!("PTY support disabled in headless build"))
        }

        pub fn terminate_session(&self, _session_id: &str) -> Result<()> {
            Err(anyhow!("PTY support disabled in headless build"))
        }

        pub fn close_session(&self, _session_id: &str) -> Result<VTCodePtySession> {
            Err(anyhow!("PTY support disabled in headless build"))
        }

        pub fn terminate_all_sessions(&self) {}
    }

    #[derive(Clone, Default)]
    pub struct PtyPreviewRenderer;

    pub fn is_cargo_command(_command: &PtyCommandRequest) -> bool {
        false
    }

    pub fn is_cargo_command_string(_command: &str) -> bool {
        false
    }

    pub fn is_development_toolchain_command(_command: &str) -> bool {
        false
    }
}

#[cfg(not(feature = "tui"))]
pub use headless_pty::*;
