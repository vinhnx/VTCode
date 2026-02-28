use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::utils::file_utils::read_file_with_context_sync;
use anyhow::{Context, Result, anyhow};
use editor_command::Editor;
use ratatui::crossterm::ExecutableCommand;
use ratatui::crossterm::event;
use ratatui::crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode, is_raw_mode_enabled,
};
use tempfile::NamedTempFile;
use tracing::debug;

/// Result from running a terminal application
#[derive(Debug)]
pub struct TerminalAppResult {
    /// Exit code from the application
    pub exit_code: i32,
    /// Whether the application completed successfully
    pub success: bool,
}

/// Runtime configuration for launching an external editor.
#[derive(Debug, Clone, Default)]
pub struct EditorLaunchConfig {
    /// Preferred editor command override (supports args, e.g. `code --wait`)
    pub preferred_editor: Option<String>,
}

/// Manages launching terminal applications
pub struct TerminalAppLauncher {
    workspace_root: PathBuf,
}

impl TerminalAppLauncher {
    /// Create a new terminal app launcher
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    /// Launch user's preferred editor with optional file
    ///
    /// If a file is provided, it will be opened in the editor.
    /// If no file is provided, a temporary file will be created and its
    /// contents returned after editing.
    ///
    /// Uses the `editor-command` crate to detect and launch the user's preferred editor
    /// from environment variables (VISUAL, EDITOR) or common editor defaults.
    ///
    /// # Errors
    ///
    /// Returns an error if the editor fails to launch or if file operations fail.
    pub fn launch_editor(&self, file: Option<PathBuf>) -> Result<Option<String>> {
        self.launch_editor_with_config(file, EditorLaunchConfig::default())
    }

    /// Launch user's preferred editor with explicit launch configuration.
    ///
    /// `preferred_editor`, when set, takes precedence over VISUAL/EDITOR env vars.
    pub fn launch_editor_with_config(
        &self,
        file: Option<PathBuf>,
        config: EditorLaunchConfig,
    ) -> Result<Option<String>> {
        let (file_path, is_temp) = if let Some(path) = file {
            (path, false)
        } else {
            // Create temp file for editing
            let temp =
                NamedTempFile::new().context("failed to create temporary file for editing")?;
            // Keep temp file alive by persisting it
            let (_, path) = temp.keep().context("failed to persist temporary file")?;
            (path, true)
        };
        let preferred_editor = config
            .preferred_editor
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        // Use unified terminal suspension logic
        self.suspend_terminal_for_command(|| {
            debug!("launching editor with file: {:?}", file_path);

            // Prefer explicit config override, then VISUAL/EDITOR, then fallback probes.
            let mut cmd = if let Some(preferred) = preferred_editor.as_deref() {
                debug!("using configured preferred editor command: {}", preferred);
                Self::build_editor_command_from_string(preferred, &file_path).with_context(
                    || {
                        format!(
                            "failed to parse tools.editor.preferred_editor '{}'",
                            preferred
                        )
                    },
                )?
            } else {
                match Editor::new() {
                    Ok(editor) => editor.open(&file_path),
                    Err(_) => {
                        // If EDITOR/VISUAL not set, search for available editors in PATH
                        debug!("EDITOR/VISUAL not set, searching for available editors");
                        Self::try_common_editors(&file_path).context(
                            "failed to detect editor: set tools.editor.preferred_editor, \
                             or set EDITOR/VISUAL, or install an editor in PATH",
                        )?
                    }
                }
            };

            let status = cmd
                .current_dir(&self.workspace_root)
                .status()
                .context("failed to spawn editor")?;

            if !status.success() {
                return Err(anyhow!(
                    "editor exited with non-zero status: {}",
                    status.code().unwrap_or(-1)
                ));
            }

            Ok(())
        })?;

        // Read temp file contents if it was a temp file
        let content = if is_temp {
            let content = read_file_with_context_sync(&file_path, "edited temporary file")
                .context("failed to read edited content from temporary file")?;
            fs::remove_file(&file_path).context("failed to remove temporary file")?;
            Some(content)
        } else {
            None
        };

        Ok(content)
    }

    fn build_editor_command_from_string(
        command: &str,
        file_path: &Path,
    ) -> Result<std::process::Command> {
        let tokens = shell_words::split(command)
            .with_context(|| format!("invalid editor command: {}", command))?;
        let (program, args) = tokens
            .split_first()
            .ok_or_else(|| anyhow!("editor command cannot be empty"))?;
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        cmd.arg(file_path);
        Ok(cmd)
    }

    /// Try common editors in priority order as fallback when EDITOR/VISUAL not set
    fn try_common_editors(file_path: &Path) -> Result<std::process::Command> {
        let candidates = if cfg!(target_os = "windows") {
            vec![
                "code --wait",
                "code",
                "zed --wait",
                "zed",
                "subl -w",
                "subl",
                "notepad++",
                "notepad",
            ]
        } else if cfg!(target_os = "macos") {
            vec![
                "code --wait",
                "code",
                "zed --wait",
                "zed",
                "subl -w",
                "subl",
                "mate -w",
                "mate",
                "open -a TextEdit",
                "nvim",
                "vim",
                "vi",
                "nano",
                "emacs",
            ]
        } else {
            vec![
                "code --wait",
                "code",
                "zed --wait",
                "zed",
                "subl -w",
                "subl",
                "mate -w",
                "mate",
                "nvim",
                "vim",
                "vi",
                "nano",
                "emacs",
            ]
        };

        for candidate in candidates {
            let tokens = match shell_words::split(candidate) {
                Ok(tokens) => tokens,
                Err(_) => continue,
            };
            let Some(program) = tokens.first() else {
                continue;
            };
            if which::which(program).is_ok() {
                debug!("found fallback editor: {}", candidate);
                return Self::build_editor_command_from_string(candidate, file_path);
            }
        }

        Err(anyhow!(
            "no editor found in PATH. Install an editor (e.g. nvim, code, zed, emacs), \
             or configure tools.editor.preferred_editor"
        ))
    }

    /// Suspend terminal UI state and run external command
    ///
    /// This is the unified method for launching external applications while
    /// properly managing terminal state. It follows the Ratatui recipe:
    /// https://ratatui.rs/recipes/apps/spawn-vim/
    ///
    /// The sequence ensures:
    /// 1. Event handler is stopped (if applicable)
    /// 2. Alternate screen is left
    /// 3. Pending events are drained (CRITICAL!)
    /// 4. Raw mode is disabled
    /// 5. External command runs freely
    /// 6. Raw mode is re-enabled
    /// 7. Alternate screen is re-entered
    /// 8. Terminal is cleared (removes artifacts)
    /// 9. Event handler is restarted (if applicable)
    ///
    /// # Errors
    ///
    /// Returns an error if terminal state management fails or command fails.
    fn suspend_terminal_for_command<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        let was_raw_mode = match is_raw_mode_enabled() {
            Ok(enabled) => enabled,
            Err(error) => {
                debug!(%error, "failed to query raw mode status; assuming non-raw terminal state");
                false
            }
        };

        if was_raw_mode {
            // Leave alternate screen
            io::stdout()
                .execute(LeaveAlternateScreen)
                .context("failed to leave alternate screen")?;

            // CRITICAL: Drain any pending crossterm events BEFORE disabling raw mode.
            // This prevents the external app from receiving garbage input (like terminal
            // capability responses or buffered keystrokes) that might have been sent to the TUI.
            while event::poll(Duration::from_millis(0)).unwrap_or(false) {
                let _ = event::read();
            }

            // Disable raw mode
            disable_raw_mode().context("failed to disable raw mode")?;
        }

        // Run the command
        let result = f();

        if was_raw_mode {
            // Re-enable raw mode
            enable_raw_mode().context("failed to re-enable raw mode")?;

            // Re-enter alternate screen
            io::stdout()
                .execute(EnterAlternateScreen)
                .context("failed to re-enter alternate screen")?;

            // Clear terminal to remove artifacts
            // This prevents ANSI escape codes from external apps' background color requests
            // from appearing in the TUI.
            io::stdout()
                .execute(Clear(ClearType::All))
                .context("failed to clear terminal")?;
        }

        result
    }

    /// Launch git interface (Lazygit or interactive git)
    ///
    /// This will attempt to launch Lazygit if available, otherwise falls back
    /// to an interactive git command.
    ///
    /// # Errors
    ///
    /// Returns an error if the git interface fails to launch.
    pub fn launch_git_interface(&self) -> Result<()> {
        self.suspend_terminal_for_command(|| {
            let git_cmd = if which::which("lazygit").is_ok() {
                "lazygit"
            } else {
                "git"
            };

            let status = Command::new(git_cmd)
                .current_dir(&self.workspace_root)
                .status()
                .with_context(|| format!("failed to spawn {}", git_cmd))?;

            if !status.success() {
                return Err(anyhow!(
                    "{} exited with non-zero status: {}",
                    git_cmd,
                    status.code().unwrap_or(-1)
                ));
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn test_launcher_creation() {
        let launcher = TerminalAppLauncher::new(PathBuf::from("/tmp"));
        // Just verify it can be created without panicking
        assert_eq!(launcher.workspace_root, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_build_editor_command_supports_arguments() {
        let command = TerminalAppLauncher::build_editor_command_from_string(
            "code --wait",
            Path::new("/tmp/test.rs"),
        )
        .expect("command should parse");
        let args: Vec<String> = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect();

        assert_eq!(command.get_program(), OsStr::new("code"));
        assert_eq!(args, vec!["--wait".to_string(), "/tmp/test.rs".to_string()]);
    }

    #[test]
    fn test_build_editor_command_rejects_empty_string() {
        let result =
            TerminalAppLauncher::build_editor_command_from_string("   ", Path::new("/tmp/test.rs"));
        assert!(result.is_err());
    }
}
