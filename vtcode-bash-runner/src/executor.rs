use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};

#[cfg(feature = "serde-errors")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "pure-rust")]
use std::fs;
#[cfg(feature = "dry-run")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "exec-events")]
use std::sync::{
    Mutex as StdMutex,
    atomic::{AtomicU64, Ordering},
};

#[cfg(feature = "exec-events")]
use vtcode_exec_events::{
    CommandExecutionItem, CommandExecutionStatus, EventEmitter, ItemCompletedEvent,
    ItemStartedEvent, ThreadEvent, ThreadItem, ThreadItemDetails,
};

/// Logical grouping for commands issued by the [`BashRunner`][crate::BashRunner].
#[cfg_attr(feature = "serde-errors", derive(Serialize, Deserialize))]
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
#[cfg_attr(feature = "serde-errors", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellKind {
    Unix,
    Windows,
}

/// Describes a command that will be executed by a [`CommandExecutor`].
#[cfg_attr(feature = "serde-errors", derive(Serialize, Deserialize))]
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
#[cfg_attr(feature = "serde-errors", derive(Serialize, Deserialize))]
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

    pub fn failure(code: Option<i32>) -> Self {
        Self {
            success: false,
            code,
        }
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
#[cfg_attr(feature = "serde-errors", derive(Serialize, Deserialize))]
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

    pub fn failure(
        code: Option<i32>,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Self {
        Self {
            status: CommandStatus::failure(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
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
                #[cfg(not(feature = "powershell-process"))]
                {
                    bail!(
                        "powershell-process feature disabled; enable it to execute Windows commands"
                    );
                }
                #[cfg(feature = "powershell-process")]
                let mut command = Command::new("powershell");
                command
                    .arg("-NoProfile")
                    .arg("-NonInteractive")
                    .arg("-Command")
                    .arg(&invocation.command);
                #[cfg(feature = "powershell-process")]
                {
                    command
                }
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

#[cfg(feature = "dry-run")]
#[derive(Clone, Default)]
pub struct DryRunCommandExecutor {
    log: Arc<Mutex<Vec<CommandInvocation>>>,
}

#[cfg(feature = "dry-run")]
impl DryRunCommandExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn logged_invocations(&self) -> Vec<CommandInvocation> {
        match self.log.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

#[cfg(feature = "dry-run")]
impl CommandExecutor for DryRunCommandExecutor {
    fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput> {
        let mut guard = match self.log.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.push(invocation.clone());
        Ok(match invocation.category {
            CommandCategory::ListDirectory => CommandOutput::success("(dry-run listing)"),
            _ => CommandOutput::success(String::new()),
        })
    }
}

#[cfg(feature = "pure-rust")]
#[derive(Debug, Default, Clone, Copy)]
pub struct PureRustCommandExecutor;

#[cfg(feature = "pure-rust")]
impl PureRustCommandExecutor {
    fn resolve_primary_path(invocation: &CommandInvocation) -> Result<&PathBuf> {
        invocation
            .touched_paths
            .first()
            .ok_or_else(|| anyhow!("invocation missing target path"))
    }

    fn should_include_hidden(command: &str) -> bool {
        command.contains("-a") || command.contains("-Force")
    }

    fn mkdir(path: &Path, command: &str) -> Result<()> {
        if command.contains("-p") || command.contains("-Force") {
            fs::create_dir_all(path)
                .with_context(|| format!("failed to create directory `{}`", path.display()))?
        } else {
            fs::create_dir(path)
                .with_context(|| format!("failed to create directory `{}`", path.display()))?
        }
        Ok(())
    }

    fn rm(path: &Path, command: &str) -> Result<()> {
        if path.is_dir() {
            if command.contains("-r") || command.contains("-Recurse") {
                fs::remove_dir_all(path)
                    .with_context(|| format!("failed to remove directory `{}`", path.display()))?
            } else {
                fs::remove_dir(path)
                    .with_context(|| format!("failed to remove directory `{}`", path.display()))?
            }
        } else if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("failed to remove file `{}`", path.display()))?
        }
        Ok(())
    }

    fn copy_recursive(source: &Path, dest: &Path, recursive: bool) -> Result<()> {
        if source.is_dir() {
            if !recursive {
                bail!(
                    "copying directory `{}` requires recursive flag",
                    source.display()
                );
            }
            fs::create_dir_all(dest)
                .with_context(|| format!("failed to create directory `{}`", dest.display()))?;
            for entry in fs::read_dir(source)
                .with_context(|| format!("failed to read directory `{}`", source.display()))?
            {
                let entry = entry?;
                let entry_path = entry.path();
                let dest_path = dest.join(entry.file_name());
                if entry_path.is_dir() {
                    Self::copy_recursive(&entry_path, &dest_path, true)?;
                } else {
                    Self::copy_file(&entry_path, &dest_path)?;
                }
            }
        } else {
            Self::copy_file(source, dest)?;
        }
        Ok(())
    }

    fn copy_file(source: &Path, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to prepare destination directory `{}`",
                    parent.display()
                )
            })?;
        }
        fs::copy(source, dest).with_context(|| {
            format!(
                "failed to copy `{}` to `{}`",
                source.display(),
                dest.display()
            )
        })?;
        Ok(())
    }

    fn move_path(source: &Path, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to prepare destination directory `{}`",
                    parent.display()
                )
            })?;
        }

        if let Err(rename_err) = fs::rename(source, dest) {
            Self::copy_recursive(source, dest, true)
                .and_then(|_| Self::rm(source, "-r -f"))
                .with_context(|| {
                    format!(
                        "failed to move `{}` to `{}` via rename: {rename_err}",
                        source.display(),
                        dest.display()
                    )
                })?;
        }
        Ok(())
    }
}

#[cfg(feature = "pure-rust")]
impl CommandExecutor for PureRustCommandExecutor {
    fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput> {
        match invocation.category {
            CommandCategory::ListDirectory => {
                let path = Self::resolve_primary_path(invocation)?;
                let mut entries = Vec::new();
                for entry in fs::read_dir(path)
                    .with_context(|| format!("failed to read directory `{}`", path.display()))?
                {
                    let entry = entry?;
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if !Self::should_include_hidden(&invocation.command) && name.starts_with('.') {
                        continue;
                    }
                    entries.push(name.to_string());
                }
                entries.sort();
                Ok(CommandOutput::success(entries.join("\n")))
            }
            CommandCategory::CreateDirectory => {
                let path = Self::resolve_primary_path(invocation)?;
                Self::mkdir(path, &invocation.command)?;
                Ok(CommandOutput::success(String::new()))
            }
            CommandCategory::Remove => {
                let path = Self::resolve_primary_path(invocation)?;
                Self::rm(path, &invocation.command)?;
                Ok(CommandOutput::success(String::new()))
            }
            CommandCategory::Copy => {
                let source = invocation
                    .touched_paths
                    .first()
                    .ok_or_else(|| anyhow!("copy missing source path"))?;
                let dest = invocation
                    .touched_paths
                    .get(1)
                    .ok_or_else(|| anyhow!("copy missing destination path"))?;
                let recursive =
                    invocation.command.contains("-r") || invocation.command.contains("-Recurse");
                Self::copy_recursive(source.as_path(), dest.as_path(), recursive)?;
                Ok(CommandOutput::success(String::new()))
            }
            CommandCategory::Move => {
                let source = invocation
                    .touched_paths
                    .first()
                    .ok_or_else(|| anyhow!("move missing source path"))?;
                let dest = invocation
                    .touched_paths
                    .get(1)
                    .ok_or_else(|| anyhow!("move missing destination path"))?;
                Self::move_path(source.as_path(), dest.as_path())?;
                Ok(CommandOutput::success(String::new()))
            }
            CommandCategory::Search => bail!(
                "pure-rust executor does not implement search; enable std-process or provide a custom executor"
            ),
            CommandCategory::ChangeDirectory | CommandCategory::PrintDirectory => {
                Ok(CommandOutput::success(String::new()))
            }
        }
    }
}

#[cfg(feature = "exec-events")]
#[derive(Debug)]
pub struct EventfulExecutor<E, T> {
    inner: E,
    emitter: StdMutex<T>,
    counter: AtomicU64,
    id_prefix: String,
}

#[cfg(feature = "exec-events")]
impl<E, T> EventfulExecutor<E, T>
where
    T: EventEmitter,
{
    pub fn new(inner: E, emitter: T) -> Self {
        Self {
            inner,
            emitter: StdMutex::new(emitter),
            counter: AtomicU64::new(0),
            id_prefix: "cmd-".to_string(),
        }
    }

    pub fn with_id_prefix(inner: E, emitter: T, prefix: impl Into<String>) -> Self {
        let mut executor = Self::new(inner, emitter);
        executor.id_prefix = prefix.into();
        executor
    }

    fn next_id(&self) -> String {
        let value = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{}{}", self.id_prefix, value)
    }

    fn emit_event(&self, event: ThreadEvent) {
        if let Ok(mut emitter) = self.emitter.lock() {
            EventEmitter::emit(&mut *emitter, &event);
        }
    }

    fn command_details(
        &self,
        invocation: &CommandInvocation,
        status: CommandExecutionStatus,
        output: Option<&CommandOutput>,
        error: Option<&anyhow::Error>,
    ) -> CommandExecutionItem {
        let aggregated_output = if let Some(output) = output {
            aggregate_output(output)
        } else if let Some(err) = error {
            err.to_string()
        } else {
            String::new()
        };

        CommandExecutionItem {
            command: invocation.command.clone(),
            aggregated_output,
            exit_code: output.and_then(|out| out.status.code()),
            status,
        }
    }
}

#[cfg(feature = "exec-events")]
impl<E, T> CommandExecutor for EventfulExecutor<E, T>
where
    E: CommandExecutor,
    T: EventEmitter + Send,
{
    fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput> {
        let item_id = self.next_id();
        let starting_item = ThreadItem {
            id: item_id.clone(),
            details: ThreadItemDetails::CommandExecution(self.command_details(
                invocation,
                CommandExecutionStatus::InProgress,
                None,
                None,
            )),
        };
        self.emit_event(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: starting_item,
        }));

        match self.inner.execute(invocation) {
            Ok(output) => {
                let completed_item = ThreadItem {
                    id: item_id,
                    details: ThreadItemDetails::CommandExecution(self.command_details(
                        invocation,
                        CommandExecutionStatus::Completed,
                        Some(&output),
                        None,
                    )),
                };
                self.emit_event(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: completed_item,
                }));
                Ok(output)
            }
            Err(err) => {
                let failure = ThreadItem {
                    id: item_id,
                    details: ThreadItemDetails::CommandExecution(self.command_details(
                        invocation,
                        CommandExecutionStatus::Failed,
                        None,
                        Some(&err),
                    )),
                };
                self.emit_event(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: failure,
                }));
                Err(err)
            }
        }
    }
}

#[cfg(feature = "exec-events")]
fn aggregate_output(output: &CommandOutput) -> String {
    let mut combined = String::new();
    if !output.stdout.trim().is_empty() {
        combined.push_str(output.stdout.trim());
    }
    if !output.stderr.trim().is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(output.stderr.trim());
    }
    combined
}
