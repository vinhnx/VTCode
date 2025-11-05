use crate::executor::{
    CommandCategory, CommandExecutor, CommandInvocation, CommandOutput, ShellKind,
};
use crate::policy::CommandPolicy;
use anyhow::{Context, Result, anyhow, bail};
use path_clean::PathClean;
use shell_escape::escape;
use std::fs;
use std::path::{Path, PathBuf};
use vtcode_commons::WorkspacePaths;

pub struct BashRunner<E, P> {
    executor: E,
    policy: P,
    workspace_root: PathBuf,
    working_dir: PathBuf,
    shell_kind: ShellKind,
}

impl<E, P> BashRunner<E, P>
where
    E: CommandExecutor,
    P: CommandPolicy,
{
    pub fn new(workspace_root: PathBuf, executor: E, policy: P) -> Result<Self> {
        if !workspace_root.exists() {
            bail!(
                "workspace root `{}` does not exist",
                workspace_root.display()
            );
        }

        let canonical_root = workspace_root
            .canonicalize()
            .with_context(|| format!("failed to canonicalize `{}`", workspace_root.display()))?;

        Ok(Self {
            executor,
            policy,
            workspace_root: canonical_root.clone(),
            working_dir: canonical_root,
            shell_kind: default_shell_kind(),
        })
    }

    pub fn from_workspace_paths<W>(paths: &W, executor: E, policy: P) -> Result<Self>
    where
        W: WorkspacePaths,
    {
        Self::new(paths.workspace_root().to_path_buf(), executor, policy)
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    pub fn shell_kind(&self) -> ShellKind {
        self.shell_kind
    }

    pub fn cd(&mut self, path: &str) -> Result<()> {
        let candidate = self.resolve_path(path);
        if !candidate.exists() {
            bail!("directory `{}` does not exist", candidate.display());
        }
        if !candidate.is_dir() {
            bail!("path `{}` is not a directory", candidate.display());
        }

        let canonical = candidate
            .canonicalize()
            .with_context(|| format!("failed to canonicalize `{}`", candidate.display()))?;

        self.ensure_within_workspace(&canonical)?;

        let invocation = CommandInvocation::new(
            self.shell_kind,
            format!("cd {}", format_path(self.shell_kind, &canonical)),
            CommandCategory::ChangeDirectory,
            canonical.clone(),
        )
        .with_paths(vec![canonical.clone()]);

        self.policy.check(&invocation)?;
        self.working_dir = canonical;
        Ok(())
    }

    pub fn ls(&self, path: Option<&str>, show_hidden: bool) -> Result<String> {
        let target = path
            .map(|p| self.resolve_existing_path(p))
            .transpose()?
            .unwrap_or_else(|| self.working_dir.clone());

        let command = match self.shell_kind {
            ShellKind::Unix => {
                let flag = if show_hidden { "-la" } else { "-l" };
                format!("ls {} {}", flag, format_path(self.shell_kind, &target))
            }
            ShellKind::Windows => {
                let mut parts = vec!["Get-ChildItem".to_string()];
                if show_hidden {
                    parts.push("-Force".to_string());
                }
                parts.push(format!("-Path {}", format_path(self.shell_kind, &target)));
                join_command(parts)
            }
        };

        let invocation = CommandInvocation::new(
            self.shell_kind,
            command,
            CommandCategory::ListDirectory,
            self.working_dir.clone(),
        )
        .with_paths(vec![target]);

        let output = self.expect_success(invocation)?;
        Ok(output.stdout)
    }

    pub fn pwd(&self) -> Result<String> {
        let invocation = CommandInvocation::new(
            self.shell_kind,
            match self.shell_kind {
                ShellKind::Unix => "pwd".to_string(),
                ShellKind::Windows => "Get-Location".to_string(),
            },
            CommandCategory::PrintDirectory,
            self.working_dir.clone(),
        );
        self.policy.check(&invocation)?;
        Ok(self.working_dir.to_string_lossy().to_string())
    }

    pub fn mkdir(&self, path: &str, parents: bool) -> Result<()> {
        let target = self.resolve_path(path);
        self.ensure_mutation_target_within_workspace(&target)?;

        let command = match self.shell_kind {
            ShellKind::Unix => {
                let mut parts = vec!["mkdir".to_string()];
                if parents {
                    parts.push("-p".to_string());
                }
                parts.push(format_path(self.shell_kind, &target));
                join_command(parts)
            }
            ShellKind::Windows => {
                let mut parts = vec!["New-Item".to_string(), "-ItemType Directory".to_string()];
                if parents {
                    parts.push("-Force".to_string());
                }
                parts.push(format!("-Path {}", format_path(self.shell_kind, &target)));
                join_command(parts)
            }
        };

        let invocation = CommandInvocation::new(
            self.shell_kind,
            command,
            CommandCategory::CreateDirectory,
            self.working_dir.clone(),
        )
        .with_paths(vec![target]);

        self.expect_success(invocation).map(|_| ())
    }

    pub fn rm(&self, path: &str, recursive: bool, force: bool) -> Result<()> {
        let target = self.resolve_path(path);
        self.ensure_mutation_target_within_workspace(&target)?;

        let command = match self.shell_kind {
            ShellKind::Unix => {
                let mut parts = vec!["rm".to_string()];
                if recursive {
                    parts.push("-r".to_string());
                }
                if force {
                    parts.push("-f".to_string());
                }
                parts.push(format_path(self.shell_kind, &target));
                join_command(parts)
            }
            ShellKind::Windows => {
                let mut parts = vec!["Remove-Item".to_string()];
                if recursive {
                    parts.push("-Recurse".to_string());
                }
                if force {
                    parts.push("-Force".to_string());
                }
                parts.push(format!("-Path {}", format_path(self.shell_kind, &target)));
                join_command(parts)
            }
        };

        let invocation = CommandInvocation::new(
            self.shell_kind,
            command,
            CommandCategory::Remove,
            self.working_dir.clone(),
        )
        .with_paths(vec![target]);

        self.expect_success(invocation).map(|_| ())
    }

    pub fn cp(&self, source: &str, dest: &str, recursive: bool) -> Result<()> {
        let source_path = self.resolve_existing_path(source)?;
        let dest_path = self.resolve_path(dest);
        self.ensure_mutation_target_within_workspace(&dest_path)?;

        let command = match self.shell_kind {
            ShellKind::Unix => {
                let mut parts = vec!["cp".to_string()];
                if recursive {
                    parts.push("-r".to_string());
                }
                parts.push(format_path(self.shell_kind, &source_path));
                parts.push(format_path(self.shell_kind, &dest_path));
                join_command(parts)
            }
            ShellKind::Windows => {
                let mut parts = vec![
                    "Copy-Item".to_string(),
                    format!("-Path {}", format_path(self.shell_kind, &source_path)),
                    format!("-Destination {}", format_path(self.shell_kind, &dest_path)),
                ];
                if recursive {
                    parts.push("-Recurse".to_string());
                }
                join_command(parts)
            }
        };

        let invocation = CommandInvocation::new(
            self.shell_kind,
            command,
            CommandCategory::Copy,
            self.working_dir.clone(),
        )
        .with_paths(vec![source_path, dest_path]);

        self.expect_success(invocation).map(|_| ())
    }

    pub fn mv(&self, source: &str, dest: &str) -> Result<()> {
        let source_path = self.resolve_existing_path(source)?;
        let dest_path = self.resolve_path(dest);
        self.ensure_mutation_target_within_workspace(&dest_path)?;

        let command = match self.shell_kind {
            ShellKind::Unix => format!(
                "mv {} {}",
                format_path(self.shell_kind, &source_path),
                format_path(self.shell_kind, &dest_path)
            ),
            ShellKind::Windows => join_command(vec![
                "Move-Item".to_string(),
                format!("-Path {}", format_path(self.shell_kind, &source_path)),
                format!("-Destination {}", format_path(self.shell_kind, &dest_path)),
            ]),
        };

        let invocation = CommandInvocation::new(
            self.shell_kind,
            command,
            CommandCategory::Move,
            self.working_dir.clone(),
        )
        .with_paths(vec![source_path, dest_path]);

        self.expect_success(invocation).map(|_| ())
    }

    pub fn grep(&self, pattern: &str, path: Option<&str>, recursive: bool) -> Result<String> {
        let target = path
            .map(|p| self.resolve_existing_path(p))
            .transpose()?
            .unwrap_or_else(|| self.working_dir.clone());

        let command = match self.shell_kind {
            ShellKind::Unix => {
                let mut parts = vec!["grep".to_string(), "-n".to_string()];
                if recursive {
                    parts.push("-r".to_string());
                }
                parts.push(format_pattern(self.shell_kind, pattern));
                parts.push(format_path(self.shell_kind, &target));
                join_command(parts)
            }
            ShellKind::Windows => {
                let mut parts = vec![
                    "Select-String".to_string(),
                    format!("-Pattern {}", format_pattern(self.shell_kind, pattern)),
                    format!("-Path {}", format_path(self.shell_kind, &target)),
                    "-SimpleMatch".to_string(),
                ];
                if recursive {
                    parts.push("-Recurse".to_string());
                }
                join_command(parts)
            }
        };

        let invocation = CommandInvocation::new(
            self.shell_kind,
            command,
            CommandCategory::Search,
            self.working_dir.clone(),
        )
        .with_paths(vec![target]);

        let output = self.execute_invocation(invocation)?;
        if output.status.success() {
            return Ok(output.stdout);
        }

        if output.stdout.trim().is_empty() && output.stderr.trim().is_empty() {
            Ok(String::new())
        } else {
            Err(anyhow!(
                "search command failed: {}",
                if output.stderr.trim().is_empty() {
                    output.stdout
                } else {
                    output.stderr
                }
            ))
        }
    }

    fn execute_invocation(&self, invocation: CommandInvocation) -> Result<CommandOutput> {
        self.policy.check(&invocation)?;
        self.executor.execute(&invocation)
    }

    fn expect_success(&self, invocation: CommandInvocation) -> Result<CommandOutput> {
        let output = self.execute_invocation(invocation.clone())?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(anyhow!(
                "command `{}` failed: {}",
                invocation.command,
                if output.stderr.trim().is_empty() {
                    output.stdout
                } else {
                    output.stderr
                }
            ))
        }
    }

    fn resolve_existing_path(&self, raw: &str) -> Result<PathBuf> {
        let path = self.resolve_path(raw);
        if !path.exists() {
            bail!("path `{}` does not exist", path.display());
        }

        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize `{}`", path.display()))?;

        self.ensure_within_workspace(&canonical)?;
        Ok(canonical)
    }

    fn resolve_path(&self, raw: &str) -> PathBuf {
        let candidate = Path::new(raw);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.working_dir.join(candidate)
        };
        joined.clean()
    }

    fn ensure_mutation_target_within_workspace(&self, candidate: &Path) -> Result<()> {
        if let Ok(metadata) = fs::symlink_metadata(candidate) {
            if metadata.file_type().is_symlink() {
                let canonical = candidate
                    .canonicalize()
                    .with_context(|| format!("failed to canonicalize `{}`", candidate.display()))?;
                return self.ensure_within_workspace(&canonical);
            }
        }

        if candidate.exists() {
            let canonical = candidate
                .canonicalize()
                .with_context(|| format!("failed to canonicalize `{}`", candidate.display()))?;
            self.ensure_within_workspace(&canonical)
        } else {
            let parent = self.canonicalize_existing_parent(candidate)?;
            self.ensure_within_workspace(&parent)
        }
    }

    fn canonicalize_existing_parent(&self, candidate: &Path) -> Result<PathBuf> {
        let mut current = candidate.parent();
        while let Some(path) = current {
            if path.exists() {
                return path
                    .canonicalize()
                    .with_context(|| format!("failed to canonicalize `{}`", path.display()));
            }
            current = path.parent();
        }

        Ok(self.working_dir.clone())
    }

    fn ensure_within_workspace(&self, candidate: &Path) -> Result<()> {
        if !candidate.starts_with(&self.workspace_root) {
            bail!(
                "path `{}` escapes workspace root `{}`",
                candidate.display(),
                self.workspace_root.display()
            );
        }
        Ok(())
    }
}

fn default_shell_kind() -> ShellKind {
    if cfg!(windows) {
        ShellKind::Windows
    } else {
        ShellKind::Unix
    }
}

fn join_command(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_path(shell: ShellKind, path: &Path) -> String {
    match shell {
        ShellKind::Unix => escape(path.to_string_lossy()).to_string(),
        ShellKind::Windows => format!("'{}'", path.to_string_lossy().replace('\'', "''")),
    }
}

fn format_pattern(shell: ShellKind, pattern: &str) -> String {
    match shell {
        ShellKind::Unix => escape(pattern.into()).to_string(),
        ShellKind::Windows => format!("'{}'", pattern.replace('\'', "''")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::{CommandInvocation, CommandOutput, CommandStatus};
    use crate::policy::AllowAllPolicy;
    use assert_fs::TempDir;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingExecutor {
        invocations: Arc<Mutex<Vec<CommandInvocation>>>,
    }

    impl CommandExecutor for RecordingExecutor {
        fn execute(&self, invocation: &CommandInvocation) -> Result<CommandOutput> {
            self.invocations.lock().unwrap().push(invocation.clone());
            Ok(CommandOutput {
                status: CommandStatus::new(true, Some(0)),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn cd_updates_working_directory() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let runner = BashRunner::new(
            dir.path().to_path_buf(),
            RecordingExecutor::default(),
            AllowAllPolicy,
        );
        let mut runner = runner.unwrap();
        runner.cd("nested").unwrap();
        assert_eq!(runner.working_dir(), nested);
    }

    #[test]
    fn mkdir_records_invocation() {
        let dir = TempDir::new().unwrap();
        let executor = RecordingExecutor::default();
        let runner = BashRunner::new(dir.path().to_path_buf(), executor.clone(), AllowAllPolicy);
        runner.unwrap().mkdir("new_dir", true).unwrap();
        let invocations = executor.invocations.lock().unwrap();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].category, CommandCategory::CreateDirectory);
    }
}
