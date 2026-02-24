//! High-level shell command runner and workspace utilities
//!
//! Provides workspace-safe shell operations (cd, ls, pwd, etc.) and
//! handles platform-specific shell detection.
//!
//! ## Shell Snapshots
//!
//! This module integrates with the shell snapshot system to avoid re-running
//! login scripts for every command. When a snapshot is available, commands
//! can be executed with the cached environment, significantly improving
//! startup time.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tracing::debug;

use crate::telemetry::perf;
use crate::tools::command_cache::{
    InFlightState, cache_output, enter_inflight, finish_inflight, get_cached_output,
};
use crate::utils::path::{canonicalize_workspace, ensure_path_within_workspace};
use crate::utils::validation::validate_path_exists;

use super::shell_snapshot::{ShellSnapshot, global_snapshot_manager};

/// Represents a workspace-safe shell runner
pub struct ShellRunner<E: CommandExecutor = SystemExecutor> {
    workspace_root: PathBuf,
    working_dir: PathBuf,
    executor: E,
}

/// Trait for command execution strategies
#[async_trait::async_trait]
pub trait CommandExecutor: Send + Sync {
    /// Execute a command in the given directory
    async fn execute(&self, command: &str, cwd: &Path) -> Result<ShellOutput>;
}

/// Standard system command executor
pub struct SystemExecutor {
    shell: String,
}

impl Default for SystemExecutor {
    fn default() -> Self {
        Self {
            shell: resolve_fallback_shell(),
        }
    }
}

#[async_trait::async_trait]
impl CommandExecutor for SystemExecutor {
    async fn execute(&self, command_str: &str, cwd: &Path) -> Result<ShellOutput> {
        let mut tags = std::collections::HashMap::new();
        tags.insert("subsystem".to_string(), "shell".to_string());
        tags.insert("program".to_string(), self.shell.clone());
        perf::record_value("vtcode.perf.spawn_count", 1.0, tags);

        let mut cmd = Command::new(&self.shell);
        cmd.arg("-c")
            .arg(command_str)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().await?;

        Ok(ShellOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

/// Executor that only logs commands without executing them
pub struct DryRunExecutor {
    pub log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Default for DryRunExecutor {
    fn default() -> Self {
        Self {
            log: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl CommandExecutor for DryRunExecutor {
    async fn execute(&self, command: &str, _cwd: &Path) -> Result<ShellOutput> {
        let mut log = self
            .log
            .lock()
            .map_err(|e| anyhow::anyhow!("DryRunExecutor log lock poisoned: {e}"))
            .context("Failed to record dry-run command")?;
        log.push(command.to_string());
        Ok(ShellOutput {
            stdout: format!("(dry-run) {}", command),
            stderr: String::new(),
            exit_code: 0,
        })
    }
}

/// Executor that uses shell snapshots for faster command execution.
///
/// This executor captures the shell environment once (after login scripts run)
/// and reuses it for subsequent commands, avoiding the overhead of re-running
/// login scripts for every command.
pub struct SnapshotExecutor {
    shell: String,
    snapshot: Option<std::sync::Arc<ShellSnapshot>>,
}

impl SnapshotExecutor {
    /// Create a new snapshot executor.
    ///
    /// The snapshot will be lazily captured on first command execution.
    pub fn new() -> Self {
        Self {
            shell: resolve_fallback_shell(),
            snapshot: None,
        }
    }

    /// Create a snapshot executor with a pre-captured snapshot.
    pub fn with_snapshot(snapshot: std::sync::Arc<ShellSnapshot>) -> Self {
        Self {
            shell: snapshot.shell_path.clone(),
            snapshot: Some(snapshot),
        }
    }

    /// Get or capture a shell snapshot.
    async fn get_snapshot(&self) -> Result<std::sync::Arc<ShellSnapshot>> {
        if let Some(ref snap) = self.snapshot {
            return Ok(std::sync::Arc::clone(snap));
        }
        global_snapshot_manager().get_or_capture().await
    }
}

impl Default for SnapshotExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl CommandExecutor for SnapshotExecutor {
    async fn execute(&self, command_str: &str, cwd: &Path) -> Result<ShellOutput> {
        let snapshot = self.get_snapshot().await?;

        let mut tags = std::collections::HashMap::new();
        tags.insert("subsystem".to_string(), "shell_snapshot".to_string());
        tags.insert("program".to_string(), self.shell.clone());
        perf::record_value("vtcode.perf.spawn_count", 1.0, tags);

        let mut cmd = Command::new(&self.shell);
        cmd.arg("-c")
            .arg(command_str)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        cmd.env_clear();
        for (key, value) in &snapshot.env {
            cmd.env(key, value);
        }

        debug!(
            shell = %self.shell,
            env_vars = snapshot.env.len(),
            "Executing command with snapshot environment"
        );

        let output = cmd.output().await?;

        Ok(ShellOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

impl ShellRunner<SystemExecutor> {
    /// Create a new system shell runner anchored to the workspace root
    pub fn new(workspace_root: PathBuf) -> Self {
        let canonical_root = canonicalize_workspace(&workspace_root);
        Self {
            workspace_root: canonical_root.clone(),
            working_dir: canonical_root,
            executor: SystemExecutor::default(),
        }
    }
}

impl ShellRunner<SnapshotExecutor> {
    /// Create a new shell runner that uses environment snapshots.
    ///
    /// This avoids re-running login scripts for every command by capturing
    /// the shell environment once and reusing it.
    pub fn with_snapshot(workspace_root: PathBuf) -> Self {
        let canonical_root = canonicalize_workspace(&workspace_root);
        Self {
            workspace_root: canonical_root.clone(),
            working_dir: canonical_root,
            executor: SnapshotExecutor::new(),
        }
    }

    /// Create a new shell runner with a pre-captured snapshot.
    pub fn with_existing_snapshot(
        workspace_root: PathBuf,
        snapshot: std::sync::Arc<ShellSnapshot>,
    ) -> Self {
        let canonical_root = canonicalize_workspace(&workspace_root);
        Self {
            workspace_root: canonical_root.clone(),
            working_dir: canonical_root,
            executor: SnapshotExecutor::with_snapshot(snapshot),
        }
    }
}

impl<E: CommandExecutor> ShellRunner<E> {
    /// Create a new shell runner with a custom executor
    pub fn with_executor(workspace_root: PathBuf, executor: E) -> Self {
        let canonical_root = canonicalize_workspace(&workspace_root);
        Self {
            workspace_root: canonical_root.clone(),
            working_dir: canonical_root,
            executor,
        }
    }

    /// Get current working directory relative to workspace root
    pub fn cwd_relative(&self) -> String {
        self.working_dir
            .strip_prefix(&self.workspace_root)
            .unwrap_or(&self.working_dir)
            .to_string_lossy()
            .into_owned()
    }

    /// Change directory workspace-safely
    pub fn cd(&mut self, path: &str) -> Result<()> {
        let target = self.resolve_path(path);

        if !target.exists() {
            bail!("directory `{}` does not exist", path);
        }
        if !target.is_dir() {
            bail!("path `{}` is not a directory", path);
        }

        let normalized = ensure_path_within_workspace(&target, &self.workspace_root)?;

        self.working_dir = normalized;
        Ok(())
    }

    /// List directory contents (simplified version of ListDirHandler)
    pub async fn ls(&self, path: Option<&str>) -> Result<Vec<Value>> {
        let target = match path {
            Some(p) => self.resolve_path(p),
            None => self.working_dir.clone(),
        };

        validate_path_exists(&target, "path")?;
        ensure_path_within_workspace(&target, &self.workspace_root)?;

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&target).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            entries.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len(),
            }));
        }

        Ok(entries)
    }

    /// Execute a shell command in the current working directory
    pub async fn exec(&self, command_str: &str) -> Result<ShellOutput> {
        if let Some(cached) = get_cached_output(command_str, &self.working_dir) {
            return Ok(cached);
        }

        let mut inflight_token = None;
        if let Some(inflight) = enter_inflight(command_str, &self.working_dir).await {
            match inflight {
                InFlightState::Wait(receiver) => {
                    if let Ok(result) = receiver.await {
                        return result.map_err(|msg| anyhow::anyhow!(msg));
                    }
                }
                InFlightState::Owner(token) => {
                    inflight_token = Some(token);
                }
            }
        }

        let output = self.executor.execute(command_str, &self.working_dir).await;

        if let Some(token) = inflight_token {
            let result = output
                .as_ref()
                .map(|out| out.clone())
                .map_err(|err| err.to_string());
            finish_inflight(token, result).await;
        }

        let output = output?;
        cache_output(command_str, &self.working_dir, output.clone());
        Ok(output)
    }

    /// Resolve a path relative to current working directory
    fn resolve_path(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.working_dir.join(path)
        }
    }
}

/// Output from a shell command
#[derive(Clone, Debug)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ShellOutput {
    /// Sanitize output to redact any secrets that may have been printed
    ///
    /// This should be called before displaying output in UI or writing to logs
    pub fn sanitize_secrets(&self) -> Self {
        Self {
            stdout: vtcode_commons::sanitizer::redact_secrets(self.stdout.clone()),
            stderr: vtcode_commons::sanitizer::redact_secrets(self.stderr.clone()),
            exit_code: self.exit_code,
        }
    }
}

/// Resolve the fallback shell for command execution when program is not found.
pub fn resolve_fallback_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        let trimmed = shell.trim();
        if !trimmed.is_empty() && Path::new(trimmed).exists() {
            return trimmed.to_string();
        }
    }

    const SHELL_CANDIDATES: &[&str] = &["/bin/bash", "/usr/bin/bash", "/bin/zsh", "/bin/sh"];

    for shell_path in SHELL_CANDIDATES {
        if Path::new(shell_path).exists() {
            return shell_path.to_string();
        }
    }

    "/bin/sh".to_string()
}
