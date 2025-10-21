use anyhow::{Context, Result};
use std::path::PathBuf;

/// Logical grouping for commands issued by the [`BashRunner`][crate::BashRunner].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    ChangeDirectory,
    ListDirectory,
    PrintDirectory,
    CreateDirectory,
    Remove,
    Copy,
    Move,
    Search,
}

/// Shell family used to execute commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellKind {
    Unix,
    Windows,
}

/// Describes a command that will be executed by a [`CommandExecutor`].
#[derive(Debug, Clone)]
pub struct CommandInvocation {
    pub shell: ShellKind,
    pub command: String,
    pub category: CommandCategory,
    pub working_dir: PathBuf,
    pub touched_paths: Vec<PathBuf>,
}

impl CommandInvocation {
    pub fn new(
        shell: ShellKind,
        command: String,
        category: CommandCategory,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            shell,
            command,
            category,
            working_dir,
            touched_paths: Vec::new(),
        }
    }

    pub fn with_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.touched_paths = paths;
        self
    }
}

/// Describes the exit status of a command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandStatus {
    success: bool,
    code: Option<i32>,
}

impl CommandStatus {
    pub fn new(success: bool, code: Option<i32>) -> Self {
        Self { success, code }
    }

    pub fn success(&self) -> bool {
        self.success
    }

    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

impl From<std::process::ExitStatus> for CommandStatus {
    fn from(status: std::process::ExitStatus) -> Self {
        let code = status.code();
        Self {
            success: status.success(),
            code,
        }
    }
}

/// Output produced by the executor for a command invocation.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: CommandStatus,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            status: CommandStatus::new(true, Some(0)),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }
}

/// Trait implemented by concrete command execution strategies.
pub trait CommandExecutor: Send + Sync {
    fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput>;
}

/// Executes commands by delegating to the system shell via [`std::process::Command`].
#[cfg(feature = "std-process")]
pub struct ProcessCommandExecutor;

#[cfg(feature = "std-process")]
impl ProcessCommandExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "std-process")]
impl Default for ProcessCommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std-process")]
impl CommandExecutor for ProcessCommandExecutor {
    fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput> {
        use std::process::Command;

        let mut cmd = match invocation.shell {
            ShellKind::Unix => {
                let mut command = Command::new("sh");
                command.arg("-c").arg(&invocation.command);
                command
            }
            ShellKind::Windows => {
                let mut command = Command::new("powershell");
                command
                    .arg("-NoProfile")
                    .arg("-NonInteractive")
                    .arg("-Command")
                    .arg(&invocation.command);
                command
            }
        };

        cmd.current_dir(&invocation.working_dir);
        let output = cmd
            .output()
            .with_context(|| format!("failed to execute command: {}", invocation.command))?;

        Ok(CommandOutput {
            status: CommandStatus::from(output.status),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}
