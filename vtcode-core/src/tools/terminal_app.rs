use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, LeaveAlternateScreen, EnterAlternateScreen};
use crossterm::ExecutableCommand;
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
    /// This follows the Ratatui pattern for spawning external editors:
    /// https://ratatui.rs/recipes/apps/spawn-vim/
    ///
    /// # Errors
    ///
    /// Returns an error if the editor fails to launch or if file operations fail.
    pub fn launch_editor(&self, file: Option<PathBuf>) -> Result<Option<String>> {
        let editor = Self::detect_editor();
        debug!("detected editor: {}", editor);

        let (file_path, is_temp) = if let Some(path) = file {
            (path, false)
        } else {
            // Create temp file for editing
            let temp = NamedTempFile::new()
                .context("failed to create temporary file for editing")?;
            // Keep temp file alive by persisting it
            let (_, path) = temp
                .keep()
                .context("failed to persist temporary file")?;
            (path, true)
        };

        // Use unified terminal suspension logic
        self.suspend_terminal_for_command(|| {
            let status = Command::new(&editor)
                .arg(&file_path)
                .current_dir(&self.workspace_root)
                .status()
                .with_context(|| format!("failed to spawn editor '{}'", editor))?;

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
            let content = fs::read_to_string(&file_path)
                .context("failed to read edited content from temporary file")?;
            fs::remove_file(&file_path)
                .context("failed to remove temporary file")?;
            Some(content)
        } else {
            None
        };

        Ok(content)
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
        // Leave alternate screen
        io::stdout()
            .execute(LeaveAlternateScreen)
            .context("failed to leave alternate screen")?;

        // CRITICAL: Drain any pending crossterm events BEFORE disabling raw mode.
        // This prevents the external app from receiving garbage input (like terminal
        // capability responses or buffered keystrokes) that might have been sent to the TUI.
        while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }

        // Disable raw mode
        disable_raw_mode().context("failed to disable raw mode")?;

        // Run the command
        let result = f();

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

    /// Detect user's preferred editor
    ///
    /// Checks in order: $EDITOR, $VISUAL, nvim, vim, vi, nano
    fn detect_editor() -> String {
        // Check EDITOR environment variable
        if let Ok(editor) = env::var("EDITOR") {
            if !editor.is_empty() {
                return editor;
            }
        }

        // Check VISUAL environment variable
        if let Ok(visual) = env::var("VISUAL") {
            if !visual.is_empty() {
                return visual;
            }
        }

        // Try common editors in order of preference
        let editors = ["nvim", "vim", "vi", "nano"];
        for editor in &editors {
            if Self::command_exists(editor) {
                return editor.to_string();
            }
        }

        // Ultimate fallback
        "vi".to_string()
    }

    /// Check if a command exists in PATH
    fn command_exists(command: &str) -> bool {
        which::which(command).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_editor() {
        let editor = TerminalAppLauncher::detect_editor();
        assert!(!editor.is_empty());
    }

    #[test]
    fn test_command_exists() {
        // Test with a command that should exist on all systems
        assert!(TerminalAppLauncher::command_exists("sh"));
    }
}
