use anyhow::{Context, Result};
#[cfg(any(not(feature = "powershell-process"), feature = "pure-rust"))]
use anyhow::{anyhow, bail};
#[cfg(feature = "pure-rust")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(feature = "exec-events")]
use parking_lot::Mutex;
#[cfg(feature = "serde-errors")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "pure-rust")]
use std::fs;
#[cfg(feature = "exec-events")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "dry-run")]
use std::sync::{Arc, Mutex as DryRunMutex};

#[cfg(feature = "exec-events")]
use vtcode_exec_events::{
    CommandExecutionItem, CommandExecutionStatus, EventEmitter, ItemCompletedEvent, ItemStartedEvent, ThreadEvent,
    ThreadItem, ThreadItemDetails,
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
    shell: ShellKind,
    pub command: String,
    form: CommandForm,
    pub(crate) category: CommandCategory,
    pub(crate) working_dir: PathBuf,
    pub(crate) touched_paths: Vec<PathBuf>,
    #[cfg_attr(feature = "serde-errors", serde(default))]
    pub(crate) create_parents: bool,
}

#[cfg_attr(feature = "serde-errors", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
enum CommandForm {
    DirectArgv(Vec<String>),
    ValidatedShellScript(String),
    Invalid(String),
}

impl CommandInvocation {
    pub(crate) fn new(shell: ShellKind, command: String, category: CommandCategory, working_dir: PathBuf) -> Self {
        let form = match shell {
            ShellKind::Unix => match shell_words::split(&command) {
                Ok(argv) if !argv.is_empty() => CommandForm::DirectArgv(argv),
                Ok(_) => CommandForm::Invalid("direct command is empty".to_owned()),
                Err(error) => CommandForm::Invalid(format!("direct command is not valid argv: {error}")),
            },
            ShellKind::Windows => CommandForm::ValidatedShellScript(command.clone()),
        };
        Self {
            shell,
            command,
            form,
            category,
            working_dir,
            touched_paths: Vec::new(),
            create_parents: false,
        }
    }

    pub(crate) fn with_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.touched_paths = paths;
        self
    }

    pub(crate) fn with_create_parents(mut self, create_parents: bool) -> Self {
        self.create_parents = create_parents;
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
    pub(crate) fn new(success: bool, code: Option<i32>) -> Self {
        Self { success, code }
    }

    pub(crate) fn success(&self) -> bool {
        self.success
    }

    fn code(&self) -> Option<i32> {
        self.code
    }

    #[cold]
    pub fn failure(code: Option<i32>) -> Self {
        Self { success: false, code }
    }
}

impl From<std::process::ExitStatus> for CommandStatus {
    fn from(status: std::process::ExitStatus) -> Self {
        let code = status.code();
        Self { success: status.success(), code }
    }
}

/// Output produced by the executor for a command invocation.
#[cfg_attr(feature = "serde-errors", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub(crate) status: CommandStatus,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl CommandOutput {
    fn success(stdout: impl Into<String>) -> Self {
        Self {
            status: CommandStatus::new(true, Some(0)),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn failure(code: Option<i32>, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            status: CommandStatus::failure(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
}

/// Trait implemented by concrete command execution strategies (sync,
/// validated-invocation form).
///
/// See `vtcode_core::tools::shell::CommandExecutor` for the async
/// string-command counterpart used by core's `ShellRunner`.
pub trait CommandExecutor: Send + Sync {
    fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput>;
}

/// Executes commands by delegating to the system shell via [`std::process::Command`].
#[cfg(feature = "std-process")]
pub struct ProcessCommandExecutor;

#[cfg(feature = "std-process")]
impl ProcessCommandExecutor {
    fn new() -> Self {
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

        let mut cmd = match &invocation.form {
            CommandForm::DirectArgv(argv) => {
                let (program, args) = argv.split_first().context("direct command is missing executable")?;
                let mut command = Command::new(program);
                command.args(args);
                command
            }
            CommandForm::ValidatedShellScript(script) if invocation.shell == ShellKind::Unix => {
                let mut command = Command::new("sh");
                command.arg("-c").arg(script);
                command
            }
            CommandForm::ValidatedShellScript(script) => {
                #[cfg(not(feature = "powershell-process"))]
                {
                    bail!("powershell-process feature disabled; enable it to execute Windows commands");
                }
                #[cfg(feature = "powershell-process")]
                let mut command = Command::new("powershell");
                command.arg("-NoProfile").arg("-NonInteractive").arg("-Command").arg(script);
                #[cfg(feature = "powershell-process")]
                {
                    command
                }
            }
            CommandForm::Invalid(message) => return Err(anyhow::Error::msg(message.clone())),
        };

        #[cfg(unix)]
        {
            let directory = vtcode_commons::fs::bound_file::open_directory_handle(&invocation.working_dir)
                .with_context(|| format!("bind command working directory {}", invocation.working_dir.display()))?;
            vtcode_commons::fs::bound_file::set_command_working_directory(&mut cmd, &directory)
                .context("confine command working directory")?;
            cmd.current_dir(&invocation.working_dir);
        }
        #[cfg(not(unix))]
        cmd.current_dir(&invocation.working_dir);
        let output = cmd
            .output()
            .with_context(|| format!("failed to execute command: {}", invocation.command))?;

        Ok(CommandOutput {
            status: CommandStatus::from(output.status),
            stdout: String::from_utf8(output.stdout).unwrap_or_else(|e| e.to_string()),
            stderr: String::from_utf8(output.stderr).unwrap_or_else(|e| e.to_string()),
        })
    }
}

#[cfg(feature = "dry-run")]
#[derive(Clone, Default)]
pub struct DryRunCommandExecutor {
    log: Arc<DryRunMutex<Vec<CommandInvocation>>>,
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

    fn mkdir(path: &Path, create_parents: bool) -> Result<()> {
        if create_parents {
            fs::create_dir_all(path).with_context(|| format!("failed to create directory `{}`", path.display()))?
        } else {
            fs::create_dir(path).with_context(|| format!("failed to create directory `{}`", path.display()))?
        }
        Ok(())
    }

    fn rm(path: &Path, command: &str) -> Result<()> {
        if path.is_dir() {
            if command.contains("-r") || command.contains("-Recurse") {
                fs::remove_dir_all(path).with_context(|| format!("failed to remove directory `{}`", path.display()))?
            } else {
                fs::remove_dir(path).with_context(|| format!("failed to remove directory `{}`", path.display()))?
            }
        } else if path.exists() {
            fs::remove_file(path).with_context(|| format!("failed to remove file `{}`", path.display()))?
        }
        Ok(())
    }

    fn copy_recursive(source: &Path, dest: &Path, recursive: bool) -> Result<()> {
        let canonical_source = vtcode_commons::canonicalize(source)
            .with_context(|| format!("failed to resolve copy source `{}`", source.display()))?;
        if !canonical_source.is_dir() {
            return Self::copy_single_file(&canonical_source, dest);
        }
        if !recursive {
            bail!("copying directory `{}` requires recursive flag", source.display());
        }
        fs::create_dir_all(dest).with_context(|| format!("failed to create directory `{}`", dest.display()))?;
        let canonical_dest = vtcode_commons::canonicalize(dest)
            .with_context(|| format!("failed to resolve copy destination `{}`", dest.display()))?;
        Self::copy_dir_anchored(&canonical_source, &canonical_source, &canonical_dest, &canonical_dest)
    }

    /// Copy directory contents with both sides anchored at their roots.
    ///
    /// Descendant symlinks are resolved and must stay beneath the root they
    /// were discovered under; links escaping to the rest of the filesystem
    /// fail the copy instead of leaking reads or writes outside the
    /// authorized trees. Links that stay inside keep the previous
    /// copy-through behavior.
    ///
    /// Residual limitation: validation is check-then-act per entry, so an
    /// actor swapping links concurrently with the copy is not defeated —
    /// only planted at-rest links are. Bound directory handles would be
    /// needed against a live concurrent swapper.
    fn copy_dir_anchored(source_dir: &Path, source_anchor: &Path, dest_dir: &Path, dest_anchor: &Path) -> Result<()> {
        for entry in
            fs::read_dir(source_dir).with_context(|| format!("failed to read directory `{}`", source_dir.display()))?
        {
            let entry = entry?;
            let canonical_entry = vtcode_commons::canonicalize(entry.path())
                .with_context(|| format!("failed to resolve `{}`", entry.path().display()))?;
            if !canonical_entry.starts_with(source_anchor) {
                bail!(
                    "refusing to copy through symlink `{}` escaping `{}`",
                    entry.path().display(),
                    source_anchor.display()
                );
            }
            let dest_path = dest_dir.join(entry.file_name());
            Self::reject_escaping_destination_link(&dest_path, dest_anchor)?;
            if canonical_entry.is_dir() {
                fs::create_dir_all(&dest_path)
                    .with_context(|| format!("failed to create directory `{}`", dest_path.display()))?;
                // `create_dir_all` follows links, so re-anchor the real
                // location before descending into it.
                let canonical_dest_subdir = vtcode_commons::canonicalize(&dest_path)
                    .with_context(|| format!("failed to resolve `{}`", dest_path.display()))?;
                if !canonical_dest_subdir.starts_with(dest_anchor) {
                    bail!("refusing to copy into `{}` escaping `{}`", dest_path.display(), dest_anchor.display());
                }
                Self::copy_dir_anchored(&canonical_entry, source_anchor, &canonical_dest_subdir, dest_anchor)?;
            } else {
                Self::copy_file(&canonical_entry, &dest_path)?;
            }
        }
        Ok(())
    }

    /// Copy one file, refusing a symlinked destination that escapes the
    /// destination's own directory.
    fn copy_single_file(canonical_source: &Path, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to prepare destination directory `{}`", parent.display()))?;
            let canonical_parent = vtcode_commons::canonicalize(parent)
                .with_context(|| format!("failed to resolve `{}`", parent.display()))?;
            Self::reject_escaping_destination_link(dest, &canonical_parent)?;
        }
        Self::copy_file(canonical_source, dest)
    }

    /// Fail when `dest` is a symlink resolving outside `dest_anchor`.
    ///
    /// `fs::copy` follows a symlinked destination, so a planted link would
    /// otherwise redirect the write outside the authorized tree.
    fn reject_escaping_destination_link(dest: &Path, dest_anchor: &Path) -> Result<()> {
        if fs::symlink_metadata(dest).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            let canonical_dest = vtcode_commons::canonicalize(dest)
                .with_context(|| format!("failed to resolve destination `{}`", dest.display()))?;
            if !canonical_dest.starts_with(dest_anchor) {
                bail!(
                    "refusing to copy through destination symlink `{}` escaping `{}`",
                    dest.display(),
                    dest_anchor.display()
                );
            }
        }
        Ok(())
    }

    fn copy_file(source: &Path, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to prepare destination directory `{}`", parent.display()))?;
        }
        fs::copy(source, dest)
            .with_context(|| format!("failed to copy `{}` to `{}`", source.display(), dest.display()))?;
        Ok(())
    }

    fn move_path(source: &Path, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to prepare destination directory `{}`", parent.display()))?;
        }

        if let Err(rename_err) = fs::rename(source, dest) {
            Self::copy_recursive(source, dest, true)
                .and_then(|_| Self::rm(source, "-r -f"))
                .with_context(|| {
                    format!("failed to move `{}` to `{}` via rename: {rename_err}", source.display(), dest.display())
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
                for entry in
                    fs::read_dir(path).with_context(|| format!("failed to read directory `{}`", path.display()))?
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
                Self::mkdir(path, invocation.create_parents)?;
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
                let recursive = invocation.command.contains("-r") || invocation.command.contains("-Recurse");
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
            CommandCategory::Search => {
                bail!("pure-rust executor does not implement search; enable std-process or provide a custom executor")
            }
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
    emitter: Mutex<T>,
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
            emitter: Mutex::new(emitter),
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
        let mut id = String::with_capacity(self.id_prefix.len() + 10);
        id.push_str(&self.id_prefix);
        use std::fmt::Write;
        let _ = write!(id, "{value}");
        id
    }

    fn emit_event(&self, event: ThreadEvent) {
        let mut emitter = self.emitter.lock();
        EventEmitter::emit(&mut *emitter, &event);
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
            arguments: None,
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
            context: None,
            id: item_id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(self.command_details(
                invocation,
                CommandExecutionStatus::InProgress,
                None,
                None,
            ))),
        };
        self.emit_event(ThreadEvent::ItemStarted(ItemStartedEvent { item: starting_item }));

        match self.inner.execute(invocation) {
            Ok(output) => {
                let status = if output.status.success() {
                    CommandExecutionStatus::Completed
                } else {
                    CommandExecutionStatus::Failed
                };

                let completed_item = ThreadItem {
                    context: None,
                    id: item_id,
                    details: ThreadItemDetails::CommandExecution(Box::new(self.command_details(
                        invocation,
                        status,
                        Some(&output),
                        None,
                    ))),
                };
                self.emit_event(ThreadEvent::ItemCompleted(ItemCompletedEvent { item: completed_item }));
                Ok(output)
            }
            Err(err) => {
                let failure = ThreadItem {
                    context: None,
                    id: item_id,
                    details: ThreadItemDetails::CommandExecution(Box::new(self.command_details(
                        invocation,
                        CommandExecutionStatus::Failed,
                        None,
                        Some(&err),
                    ))),
                };
                self.emit_event(ThreadEvent::ItemCompleted(ItemCompletedEvent { item: failure }));
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

#[cfg(all(test, feature = "serde-errors"))]
mod serde_tests {
    use super::{CommandForm, CommandInvocation, CommandOutput, CommandStatus};
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    /// Compile-time assertion that a type satisfies the `serde-errors`
    /// contract; the oracle is that this module fails to compile when a derive
    /// is missing, which is exactly how `--all-features` broke previously.
    fn assert_serde<T: Serialize + DeserializeOwned>() {}

    /// Every type reachable from a serialized payload must derive the same
    /// features. `CommandInvocation` derives under `serde-errors`, so its
    /// private `CommandForm` (and the output/status types) must as well, or
    /// `cargo check --all-features` fails.
    #[test]
    fn serde_error_types_satisfy_serialize_and_deserialize() {
        assert_serde::<CommandInvocation>();
        assert_serde::<CommandForm>();
        assert_serde::<CommandOutput>();
        assert_serde::<CommandStatus>();
    }
}
