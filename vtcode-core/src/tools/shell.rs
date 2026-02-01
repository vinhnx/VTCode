//! High-level shell command runner and workspace utilities
//!
//! Provides workspace-safe shell operations (cd, ls, pwd, etc.) and
//! handles platform-specific shell detection.

use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

use crate::utils::path::{canonicalize_workspace, normalize_path};
use crate::utils::validation::validate_path_exists;

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
        let mut log = self.log.lock().unwrap();
        log.push(command.to_string());
        Ok(ShellOutput {
            stdout: format!("(dry-run) {}", command),
            stderr: String::new(),
            exit_code: 0,
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

        let normalized = normalize_path(&target);
        self.ensure_within_workspace(&normalized)?;

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
        self.ensure_within_workspace(&target)?;

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
        self.executor.execute(command_str, &self.working_dir).await
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

    /// Ensure a path does not escape the workspace root
    fn ensure_within_workspace(&self, path: &Path) -> Result<()> {
        if !path.starts_with(&self.workspace_root) {
            bail!(
                "Security Error: path `{}` escapes workspace root",
                path.display()
            );
        }
        Ok(())
    }
}

/// Output from a shell command
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
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
