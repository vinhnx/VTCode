use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::utils::file_utils::read_file_with_context_sync;
use anyhow::{Context, Result, anyhow};
use ratatui::crossterm::ExecutableCommand;
use ratatui::crossterm::event;
use ratatui::crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode, is_raw_mode_enabled,
};
use tempfile::NamedTempFile;
use tracing::debug;
use vtcode_commons::EditorTarget;

/// Result from running a terminal application
#[derive(Debug)]
pub struct TerminalAppResult {
    /// Exit code from the application
    pub exit_code: i32,
    /// Whether the application completed successfully
    pub success: bool,
}

/// Runtime configuration for launching an external editor.
#[derive(Debug, Clone)]
pub struct EditorLaunchConfig {
    /// Preferred editor command override (supports args, e.g. `code --wait`)
    pub preferred_editor: Option<String>,
    /// Wait for the editor process to exit before returning.
    pub wait_for_editor: bool,
}

impl Default for EditorLaunchConfig {
    fn default() -> Self {
        Self {
            preferred_editor: None,
            wait_for_editor: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCommandStrategy {
    Shell,
    PowerShell,
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
    /// Uses the configured editor command, then VISUAL/EDITOR, then common editor defaults.
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
        let target = file.map(|path| EditorTarget::new(path, None));
        self.launch_editor_target_with_config(target, config)
    }

    /// Launch user's preferred editor with an optional file target and location.
    ///
    /// `preferred_editor`, when set, takes precedence over VISUAL/EDITOR env vars.
    pub fn launch_editor_target_with_config(
        &self,
        target: Option<EditorTarget>,
        config: EditorLaunchConfig,
    ) -> Result<Option<String>> {
        let (target, is_temp) = if let Some(target) = target {
            (target, false)
        } else {
            // Create temp file for editing
            let temp =
                NamedTempFile::new().context("failed to create temporary file for editing")?;
            // Keep temp file alive by persisting it
            let (_, path) = temp.keep().context("failed to persist temporary file")?;
            (EditorTarget::new(path, None), true)
        };
        let file_path = target.path().to_path_buf();
        let mut wait_for_editor = is_temp || config.wait_for_editor;
        let preferred_editor = config
            .preferred_editor
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        debug!(
            path = %file_path.display(),
            wait_for_editor,
            "launching editor"
        );

        let mut cmd = if let Some(preferred) = preferred_editor.as_deref() {
            debug!("using configured preferred editor command: {}", preferred);
            Self::build_editor_command_from_string(preferred, &target, wait_for_editor)
                .with_context(|| {
                    format!(
                        "failed to parse tools.editor.preferred_editor '{}'",
                        preferred
                    )
                })?
        } else if let Some(env_command) = Self::editor_command_from_env() {
            debug!("using editor command from environment: {}", env_command);
            Self::build_editor_command_from_string(&env_command, &target, wait_for_editor)
                .with_context(|| format!("failed to parse editor command '{}'", env_command))?
        } else {
            // If EDITOR/VISUAL not set, search for available editors in PATH
            debug!("EDITOR/VISUAL not set, searching for available editors");
            Self::try_common_editors(&target, wait_for_editor).context(
                "failed to detect editor: set tools.editor.preferred_editor, \
                 or set EDITOR/VISUAL, or install an editor in PATH",
            )?
        };

        if !wait_for_editor {
            let program = cmd.get_program().to_string_lossy().to_string();
            if Self::program_requires_terminal(&program) {
                debug!(
                    program = %program,
                    "forcing synchronous launch for terminal-based editor"
                );
                wait_for_editor = true;
            }
        }

        if wait_for_editor {
            self.suspend_terminal_for_command(|| {
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
        } else {
            cmd.current_dir(&self.workspace_root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("failed to spawn editor")?;
        }

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
        target: &EditorTarget,
        wait_for_editor: bool,
    ) -> Result<Command> {
        let tokens = shell_words::split(command)
            .with_context(|| format!("invalid editor command: {}", command))?;
        let (program, args) = tokens
            .split_first()
            .ok_or_else(|| anyhow!("editor command cannot be empty"))?;
        let adapter = EditorAdapter::from_program(program);
        let mut cmd = Command::new(program);
        cmd.args(filtered_editor_args(adapter, args, wait_for_editor));
        Self::append_editor_target_args(&mut cmd, program, target);
        Ok(cmd)
    }

    /// Try common editors in priority order as fallback when EDITOR/VISUAL not set
    fn try_common_editors(target: &EditorTarget, wait_for_editor: bool) -> Result<Command> {
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
                return Self::build_editor_command_from_string(candidate, target, wait_for_editor);
            }
        }

        Err(anyhow!(
            "no editor found in PATH. Install an editor (e.g. nvim, code, zed, emacs), \
             or configure tools.editor.preferred_editor"
        ))
    }

    fn editor_command_from_env() -> Option<String> {
        ["VISUAL", "EDITOR"]
            .into_iter()
            .find_map(|key| std::env::var(key).ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn program_requires_terminal(program: &str) -> bool {
        let normalized = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program)
            .to_ascii_lowercase();

        matches!(
            normalized.as_str(),
            "vi" | "vim" | "nvim" | "nano" | "emacs" | "pico" | "hx" | "helix"
        )
    }

    fn append_editor_target_args(cmd: &mut Command, program: &str, target: &EditorTarget) {
        let adapter = EditorAdapter::from_program(program);
        let file_path = target.path();

        match (adapter, target.point()) {
            (EditorAdapter::Vscode, Some(point)) => {
                cmd.arg("-g");
                cmd.arg(format_location_arg(file_path, point.line, point.column));
            }
            (EditorAdapter::ColonLocation, Some(point)) => {
                cmd.arg(format_location_arg(file_path, point.line, point.column));
            }
            (EditorAdapter::Vim, Some(point)) => {
                if let Some(column) = point.column {
                    cmd.arg(format!("+call cursor({},{})", point.line, column));
                } else {
                    cmd.arg(format!("+{}", point.line));
                }
                cmd.arg(file_path);
            }
            _ => {
                cmd.arg(file_path);
            }
        }
    }

    /// Suspend terminal UI state and run external command
    ///
    /// This is the unified method for launching external applications while
    /// properly managing terminal state. It follows the Ratatui recipe:
    /// <https://ratatui.rs/recipes/apps/spawn-vim/>
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
    fn suspend_terminal_for_command<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
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
            // Always attempt every restore step so we minimize the chance of leaving the terminal
            // in a partially restored state.
            let mut restore_errors = Vec::new();

            if let Err(error) = enable_raw_mode() {
                restore_errors.push(format!("failed to re-enable raw mode: {}", error));
            }

            if let Err(error) = io::stdout().execute(EnterAlternateScreen) {
                restore_errors.push(format!("failed to re-enter alternate screen: {}", error));
            }

            // This prevents ANSI escape codes from external apps' background color requests
            // from appearing in the TUI.
            if let Err(error) = io::stdout().execute(Clear(ClearType::All)) {
                restore_errors.push(format!("failed to clear terminal: {}", error));
            }

            if !restore_errors.is_empty() {
                let restore_summary = restore_errors.join("; ");
                return match result {
                    Ok(_) => Err(anyhow!("terminal restore failed: {}", restore_summary)),
                    Err(command_error) => Err(command_error
                        .context(format!("terminal restore also failed: {}", restore_summary))),
                };
            }
        }

        result
    }

    pub fn run_command_with_strategy(
        &self,
        command: &str,
        strategy: TerminalCommandStrategy,
    ) -> Result<TerminalAppResult> {
        self.suspend_terminal_for_command(|| {
            let mut cmd = match strategy {
                TerminalCommandStrategy::Shell => {
                    #[cfg(target_os = "windows")]
                    {
                        let mut command_builder = Command::new("cmd");
                        command_builder.arg("/C").arg(command);
                        command_builder
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let mut command_builder = Command::new("/bin/sh");
                        command_builder.arg("-lc").arg(command);
                        command_builder
                    }
                }
                TerminalCommandStrategy::PowerShell => {
                    let mut command_builder = if cfg!(target_os = "windows") {
                        Command::new("powershell")
                    } else {
                        Command::new("pwsh")
                    };
                    command_builder
                        .arg("-NoLogo")
                        .arg("-NoProfile")
                        .arg("-Command")
                        .arg(command);
                    command_builder
                }
            };

            let status = cmd
                .current_dir(&self.workspace_root)
                .status()
                .with_context(|| format!("failed to spawn update command: {}", command))?;

            Ok(TerminalAppResult {
                exit_code: status.code().unwrap_or(-1),
                success: status.success(),
            })
        })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorAdapter {
    Plain,
    Vscode,
    ColonLocation,
    Mate,
    MacOpen,
    Vim,
}

impl EditorAdapter {
    fn from_program(program: &str) -> Self {
        let program = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program)
            .to_ascii_lowercase();

        match program.as_str() {
            "code" | "code-insiders" => Self::Vscode,
            "zed" | "subl" => Self::ColonLocation,
            "mate" => Self::Mate,
            "open" => Self::MacOpen,
            "nvim" | "vim" | "vi" => Self::Vim,
            _ => Self::Plain,
        }
    }
}

fn filtered_editor_args(
    adapter: EditorAdapter,
    args: &[String],
    wait_for_editor: bool,
) -> Vec<String> {
    if wait_for_editor {
        return args.to_vec();
    }

    args.iter()
        .filter(|arg| !matches_wait_flag(adapter, arg))
        .cloned()
        .collect()
}

fn matches_wait_flag(adapter: EditorAdapter, arg: &str) -> bool {
    match adapter {
        EditorAdapter::Vscode => arg == "--wait",
        EditorAdapter::ColonLocation | EditorAdapter::Mate => arg == "--wait" || arg == "-w",
        EditorAdapter::MacOpen => arg == "-W",
        EditorAdapter::Plain | EditorAdapter::Vim => false,
    }
}

fn format_location_arg(path: &Path, line: usize, column: Option<usize>) -> String {
    let column = column.unwrap_or(1);
    format!("{}:{}:{}", path.display(), line, column)
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
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), None),
            true,
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
        let result = TerminalAppLauncher::build_editor_command_from_string(
            "   ",
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), None),
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_build_editor_command_uses_vscode_go_to_location() {
        let command = TerminalAppLauncher::build_editor_command_from_string(
            "code --wait",
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), Some(":12:4".to_string())),
            true,
        )
        .expect("command should parse");
        let args: Vec<String> = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec![
                "--wait".to_string(),
                "-g".to_string(),
                "/tmp/test.rs:12:4".to_string()
            ]
        );
    }

    #[test]
    fn test_build_editor_command_uses_colon_location_for_zed() {
        let command = TerminalAppLauncher::build_editor_command_from_string(
            "zed",
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), Some(":12".to_string())),
            true,
        )
        .expect("command should parse");
        let args: Vec<String> = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect();

        assert_eq!(args, vec!["/tmp/test.rs:12:1".to_string()]);
    }

    #[test]
    fn test_build_editor_command_uses_cursor_command_for_vim() {
        let command = TerminalAppLauncher::build_editor_command_from_string(
            "nvim",
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), Some(":12:4".to_string())),
            true,
        )
        .expect("command should parse");
        let args: Vec<String> = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec!["+call cursor(12,4)".to_string(), "/tmp/test.rs".to_string()]
        );
    }

    #[test]
    fn test_build_editor_command_degrades_unknown_commands_to_file_only() {
        let command = TerminalAppLauncher::build_editor_command_from_string(
            "custom-editor --flag",
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), Some(":12:4".to_string())),
            true,
        )
        .expect("command should parse");
        let args: Vec<String> = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect();

        assert_eq!(args, vec!["--flag".to_string(), "/tmp/test.rs".to_string()]);
    }

    #[test]
    fn test_build_editor_command_strips_vscode_wait_flag_when_not_waiting() {
        let command = TerminalAppLauncher::build_editor_command_from_string(
            "code --wait",
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), Some(":12:4".to_string())),
            false,
        )
        .expect("command should parse");
        let args: Vec<String> = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec!["-g".to_string(), "/tmp/test.rs:12:4".to_string()]
        );
    }

    #[test]
    fn test_build_editor_command_strips_sublime_wait_flag_when_not_waiting() {
        let command = TerminalAppLauncher::build_editor_command_from_string(
            "subl -w",
            &EditorTarget::new(PathBuf::from("/tmp/test.rs"), None),
            false,
        )
        .expect("command should parse");
        let args: Vec<String> = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect();

        assert_eq!(args, vec!["/tmp/test.rs".to_string()]);
    }

    #[test]
    fn test_program_requires_terminal_detects_terminal_editors() {
        assert!(TerminalAppLauncher::program_requires_terminal("nvim"));
        assert!(TerminalAppLauncher::program_requires_terminal(
            "/usr/bin/vim"
        ));
        assert!(TerminalAppLauncher::program_requires_terminal("helix"));
        assert!(!TerminalAppLauncher::program_requires_terminal("code"));
        assert!(!TerminalAppLauncher::program_requires_terminal("zed"));
    }
}
