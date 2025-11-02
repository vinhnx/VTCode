//! Git diff tool providing structured diff information.

use super::traits::Tool;
use crate::config::constants::tools;
use crate::tools::types::GitDiffInput;
use crate::utils::diff::{DiffBundle, DiffHunk, DiffLineKind, DiffOptions, compute_diff};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
use vte::{Parser, Perform};

/// Strip ANSI codes from text
fn strip_ansi(text: &str) -> String {
    struct AnsiStripper {
        output: String,
    }

    impl AnsiStripper {
        fn new(capacity: usize) -> Self {
            Self {
                output: String::with_capacity(capacity),
            }
        }
    }

    impl Perform for AnsiStripper {
        fn print(&mut self, c: char) {
            self.output.push(c);
        }

        fn execute(&mut self, byte: u8) {
            match byte {
                b'\n' => self.output.push('\n'),
                b'\r' => self.output.push('\r'),
                b'\t' => self.output.push('\t'),
                _ => {}
            }
        }

        fn hook(
            &mut self,
            _params: &vte::Params,
            _intermediates: &[u8],
            _ignore: bool,
            _action: char,
        ) {
        }

        fn put(&mut self, _byte: u8) {}

        fn unhook(&mut self) {}

        fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

        fn csi_dispatch(
            &mut self,
            _params: &vte::Params,
            _intermediates: &[u8],
            _ignore: bool,
            _action: char,
        ) {
        }

        fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _action: u8) {}
    }

    let mut performer = AnsiStripper::new(text.len());
    let mut parser = Parser::new();

    for byte in text.as_bytes() {
        parser.advance(&mut performer, *byte);
    }

    performer.output
}

/// Structured Git diff tool.
#[derive(Clone)]
pub struct GitDiffTool {
    workspace_root: PathBuf,
}

impl GitDiffTool {
    /// Create a new git diff tool bound to the workspace root.
    pub fn new(workspace_root: PathBuf) -> Self {
        let canonical = workspace_root
            .canonicalize()
            .unwrap_or(workspace_root.clone());
        Self {
            workspace_root: canonical,
        }
    }

    async fn git_output<I, S>(&self, args: I) -> Result<std::process::Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args_vec: Vec<OsString> = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect();

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.workspace_root);
        cmd.env("LC_ALL", "C");
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.args(["--no-pager", "-c", "color.ui=never"]);
        cmd.args(&args_vec);
        let output = cmd.output().await.with_context(|| {
            format!(
                "failed to execute git with args {:?}",
                args_to_strings(&args_vec)
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "git command {:?} failed: {}",
                args_to_strings(&args_vec),
                stderr.trim()
            ));
        }
        Ok(output)
    }

    fn normalize_relative_path(&self, path: &str) -> Result<String> {
        if path.is_empty() {
            return Err(anyhow!("path cannot be empty"));
        }

        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.workspace_root.join(path)
        };

        let normalized = normalize_path(&candidate);
        if !normalized.starts_with(&self.workspace_root) {
            return Err(anyhow!("path '{}' escapes the workspace boundary", path));
        }

        let relative = normalized
            .strip_prefix(&self.workspace_root)
            .map_err(|_| anyhow!("failed to compute relative path for '{}'", path))?;

        Ok(relative.to_string_lossy().to_string())
    }

    async fn resolve_repository_root(&self) -> Result<PathBuf> {
        let output = self.git_output(["rev-parse", "--show-toplevel"]).await?;
        let repo = String::from_utf8_lossy(&output.stdout);
        let path = repo.trim();
        let root = if path.is_empty() {
            self.workspace_root.clone()
        } else {
            PathBuf::from(path)
        };
        Ok(root)
    }

    async fn list_changed_files(
        &self,
        input: &GitDiffInput,
        path_filters: &[String],
    ) -> Result<Vec<ChangedPath>> {
        let mut args = vec![
            "diff".to_string(),
            "--name-status".to_string(),
            "--find-renames".to_string(),
            "-z".to_string(),
        ];
        if input.staged {
            args.push("--cached".to_string());
        }

        if !path_filters.is_empty() {
            args.push("--".to_string());
            args.extend(path_filters.iter().cloned());
        }

        let output = self.git_output(args).await?;
        let entries = parse_name_status_z(&output.stdout);

        Ok(entries)
    }

    async fn load_old_content(&self, path: &ChangedPath, staged: bool) -> Result<Option<String>> {
        let target_path = path.old_path.as_ref().unwrap_or(&path.path);
        let spec = format!("HEAD:{target_path}");
        let args = vec!["show".to_string(), spec.clone()];
        match self.git_output(args).await {
            Ok(output) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
            Err(_) => {
                // File may not exist in HEAD (e.g., added file)
                if staged {
                    // Attempt to fall back to index version if available.
                    let index_spec = format!(":{target_path}");
                    let index_args = vec!["show".to_string(), index_spec];
                    if let Ok(output) = self.git_output(index_args).await {
                        return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
                    }
                }
                Ok(None)
            }
        }
    }

    async fn load_new_content(&self, path: &ChangedPath, staged: bool) -> Result<Option<String>> {
        if staged {
            let spec = format!(":{}", path.path);
            let args = vec!["show".to_string(), spec];
            match self.git_output(args).await {
                Ok(output) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
                Err(_) => Ok(None),
            }
        } else {
            let disk_path = self.workspace_root.join(&path.path);
            match fs::read(&disk_path).await {
                Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
                Err(_) => Ok(None),
            }
        }
    }

    fn build_diff_for_file(
        &self,
        path: &ChangedPath,
        old_content: Option<String>,
        new_content: Option<String>,
        context_lines: usize,
    ) -> FileDiff {
        let old = old_content.unwrap_or_default();
        let new = new_content.unwrap_or_default();

        let (old_label, new_label) = match path.old_path.as_ref() {
            Some(previous) => (format!("a/{previous}"), format!("b/{}", path.path)),
            None => (format!("a/{}", path.path), format!("b/{}", path.path)),
        };

        let bundle = compute_diff(
            &old,
            &new,
            DiffOptions {
                context_lines,
                old_label: Some(&old_label),
                new_label: Some(&new_label),
                ..Default::default()
            },
        );

        let summary = DiffSummary::from_bundle(&bundle);

        // Strip ANSI color codes from the formatted output
        let formatted = strip_ansi(&bundle.formatted);

        FileDiff {
            path: path.path.clone(),
            previous_path: path.old_path.clone(),
            status: path.status.clone(),
            hunks: bundle.hunks,
            formatted,
            is_empty: bundle.is_empty,
            summary,
        }
    }
}

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &'static str {
        tools::GIT_DIFF
    }

    fn description(&self) -> &'static str {
        "Generate structured git diffs (files → hunks → lines)"
    }

    async fn execute(&self, args: Value) -> Result<Value> {
        let input: GitDiffInput = serde_json::from_value(args.clone()).context(
            "git_diff requires an object with optional fields: paths (array), staged (bool), context_lines (integer), max_files (integer)",
        )?;

        let path_filters = input
            .paths
            .iter()
            .map(|path| self.normalize_relative_path(path))
            .collect::<Result<Vec<_>>>()?;

        let repo_root = self.resolve_repository_root().await?;
        let changed = self
            .list_changed_files(&input, &path_filters)
            .await?
            .into_iter()
            .collect::<Vec<_>>();

        let mut files = Vec::new();
        let mut total_additions = 0usize;
        let mut total_deletions = 0usize;
        let mut formatted_sections = Vec::new();

        let max_files = input.max_files.unwrap_or(usize::MAX);

        for path in changed.into_iter().take(max_files) {
            let old_content = self.load_old_content(&path, input.staged).await?;
            let new_content = self.load_new_content(&path, input.staged).await?;
            let file_diff =
                self.build_diff_for_file(&path, old_content, new_content, input.context_lines);

            total_additions += file_diff.summary.additions;
            total_deletions += file_diff.summary.deletions;

            if !file_diff.formatted.trim().is_empty() {
                formatted_sections.push(file_diff.formatted.clone());
            }

            files.push(file_diff);
        }

        let aggregated_formatted = if formatted_sections.is_empty() {
            String::new()
        } else {
            formatted_sections.join("\n")
        };

        Ok(json!({
            "success": true,
            "tool": tools::GIT_DIFF,
            "staged": input.staged,
            "context_lines": input.context_lines,
            "repository_root": repo_root.to_string_lossy(),
            "file_count": files.len(),
            "files": files,
            "formatted": aggregated_formatted,
            "addition_count": total_additions,
            "deletion_count": total_deletions
        }))
    }
}

/// Represents a file reported by `git diff --name-status`.
#[derive(Debug, Clone)]
struct ChangedPath {
    status: GitFileStatus,
    path: String,
    old_path: Option<String>,
}

/// File-level diff summary.
#[derive(Debug, Clone, Serialize)]
struct FileDiff {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_path: Option<String>,
    status: GitFileStatus,
    hunks: Vec<DiffHunk>,
    formatted: String,
    is_empty: bool,
    summary: DiffSummary,
}

/// Aggregate additions/deletions for a diff.
#[derive(Debug, Clone, Serialize)]
struct DiffSummary {
    additions: usize,
    deletions: usize,
}

impl DiffSummary {
    fn from_bundle(bundle: &DiffBundle) -> Self {
        let mut additions = 0usize;
        let mut deletions = 0usize;
        for hunk in &bundle.hunks {
            for line in &hunk.lines {
                match line.kind {
                    DiffLineKind::Addition => additions += 1,
                    DiffLineKind::Deletion => deletions += 1,
                    DiffLineKind::Context => {}
                }
            }
        }
        DiffSummary {
            additions,
            deletions,
        }
    }
}

/// Git status per file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum GitFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChange,
    Unknown,
}

fn parse_name_status_z(bytes: &[u8]) -> Vec<ChangedPath> {
    let mut entries = Vec::new();
    let mut parts = bytes.split(|b| *b == 0).filter(|part| !part.is_empty());

    while let Some(status_bytes) = parts.next() {
        let status_str = String::from_utf8_lossy(status_bytes).to_string();
        let status_code = status_str.chars().next().unwrap_or(' ');
        let status = match status_code {
            'A' => GitFileStatus::Added,
            'M' => GitFileStatus::Modified,
            'D' => GitFileStatus::Deleted,
            'T' => GitFileStatus::TypeChange,
            'C' => GitFileStatus::Copied,
            'R' => GitFileStatus::Renamed,
            _ => GitFileStatus::Unknown,
        };

        match status {
            GitFileStatus::Renamed | GitFileStatus::Copied => {
                if let (Some(old_bytes), Some(new_bytes)) = (parts.next(), parts.next()) {
                    let old_path = String::from_utf8_lossy(old_bytes).to_string();
                    let new_path = String::from_utf8_lossy(new_bytes).to_string();
                    entries.push(ChangedPath {
                        status,
                        path: new_path,
                        old_path: Some(old_path),
                    });
                }
            }
            _ => {
                if let Some(path_bytes) = parts.next() {
                    let path = String::from_utf8_lossy(path_bytes).to_string();
                    entries.push(ChangedPath {
                        status,
                        path,
                        old_path: None,
                    });
                }
            }
        }
    }

    entries
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    normalized
}

fn args_to_strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect()
}
