use anyhow::{Context, Result};
use hashbrown::{HashMap, HashSet};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::path::PathBuf;
use vtcode_core::ui::is_tui_mode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitStatusSummary {
    pub branch: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FileStat {
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CodeChangeDelta {
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirtyWorktreeStatus {
    Modified,
    Added,
    Deleted,
    TypeChanged,
    Unmerged,
}

impl DirtyWorktreeStatus {
    fn from_worktree_status(value: char) -> Option<Self> {
        match value {
            'M' => Some(Self::Modified),
            'A' => Some(Self::Added),
            'D' => Some(Self::Deleted),
            'T' => Some(Self::TypeChanged),
            'U' => Some(Self::Unmerged),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Modified => "modified",
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::TypeChanged => "type_changed",
            Self::Unmerged => "unmerged",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirtyWorktreeEntry {
    pub path: PathBuf,
    pub status: DirtyWorktreeStatus,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirtyWorktreeDiffPreview {
    pub display_path: String,
    pub before: String,
    pub after: String,
    pub used_fallback_preview: bool,
}

fn is_git_repo() -> bool {
    git_repo_check(None)
}

fn is_git_repo_at(workspace: &Path) -> bool {
    git_repo_check(Some(workspace))
}

fn git_repo_check(workspace: Option<&Path>) -> bool {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["rev-parse", "--git-dir"]);
    if let Some(workspace) = workspace {
        cmd.current_dir(workspace);
    }
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn normalize_workspace_path(workspace: &Path, path: &Path) -> PathBuf {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    fs::canonicalize(&candidate).unwrap_or_else(|_| {
        candidate
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
            .and_then(|parent| candidate.file_name().map(|name| parent.join(name)))
            .unwrap_or(candidate)
    })
}

pub(crate) fn workspace_relative_display(workspace: &Path, path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(workspace) {
        return relative.display().to_string();
    }
    if let Ok(canonical_workspace) = fs::canonicalize(workspace)
        && let Ok(relative) = path.strip_prefix(canonical_workspace)
    {
        return relative.display().to_string();
    }
    path.display().to_string()
}

fn workspace_relative_git_path(workspace: &Path, path: &Path) -> String {
    workspace_relative_display(workspace, path).replace('\\', "/")
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn dirty_worktree_fingerprint(path: &Path, status: DirtyWorktreeStatus) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(status.as_str().as_bytes());

    match fs::read(path) {
        Ok(bytes) => hasher.update(&bytes),
        Err(err) if err.kind() == io::ErrorKind::NotFound => hasher.update(b"missing"),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("Failed to read dirty worktree file {}", path.display()));
        }
    }

    Ok(hex_digest(&hasher.finalize()))
}

pub(crate) fn git_dirty_worktree_entries(
    workspace: &Path,
) -> Result<Option<Vec<DirtyWorktreeEntry>>> {
    if !is_git_repo_at(workspace) {
        return Ok(None);
    }

    let output = std::process::Command::new("git")
        .args([
            "-c",
            "core.quotepath=off",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=no",
            "--ignore-submodules=all",
        ])
        .current_dir(workspace)
        .output()
        .with_context(|| {
            format!(
                "Failed to read git worktree state for {}",
                workspace.display()
            )
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    let mut records = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    let mut entries = Vec::new();

    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }

        let index_status = record[0] as char;
        let worktree_status = record[1] as char;
        let raw_path = String::from_utf8_lossy(&record[3..]).trim().to_string();
        if raw_path.is_empty() {
            continue;
        }

        if matches!(index_status, 'R' | 'C') {
            let _ = records.next();
            continue;
        }

        let Some(status) = DirtyWorktreeStatus::from_worktree_status(worktree_status) else {
            continue;
        };

        let path = normalize_workspace_path(workspace, Path::new(&raw_path));
        let fingerprint = dirty_worktree_fingerprint(&path, status)?;
        entries.push(DirtyWorktreeEntry {
            path,
            status,
            fingerprint,
        });
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Some(entries))
}

pub(crate) fn git_diff_for_path(workspace: &Path, path: &Path) -> Result<Option<String>> {
    if !is_git_repo_at(workspace) {
        return Ok(None);
    }

    let repo_path = workspace_relative_git_path(workspace, path);
    let output = std::process::Command::new("git")
        .args(["diff", "--no-color", "--no-ext-diff", "--", &repo_path])
        .current_dir(workspace)
        .output()
        .with_context(|| format!("Failed to read git diff for {}", repo_path))?;

    if !output.status.success() {
        return Ok(None);
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(diff))
    }
}

pub(crate) fn git_diff_preview_for_path(
    workspace: &Path,
    path: &Path,
) -> Result<Option<DirtyWorktreeDiffPreview>> {
    if !is_git_repo_at(workspace) {
        return Ok(None);
    }
    if git_diff_for_path(workspace, path)?.is_none() {
        return Ok(None);
    }

    let display_path = workspace_relative_display(workspace, path);
    let repo_path = workspace_relative_git_path(workspace, path);
    let before = std::process::Command::new("git")
        .args(["show", &format!(":{repo_path}")])
        .current_dir(workspace)
        .output()
        .with_context(|| format!("Failed to read git index content for {}", repo_path))?;
    let before_text = if before.status.success() {
        String::from_utf8(before.stdout).ok()
    } else {
        None
    };

    let after_text = fs::read(path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());

    match (before_text, after_text) {
        (Some(before), Some(after)) => Ok(Some(DirtyWorktreeDiffPreview {
            display_path,
            before,
            after,
            used_fallback_preview: false,
        })),
        (_, Some(after)) => Ok(Some(DirtyWorktreeDiffPreview {
            display_path,
            before: String::new(),
            after,
            used_fallback_preview: true,
        })),
        _ => Ok(None),
    }
}

pub(crate) fn git_working_tree_numstat_snapshot(
    workspace: &Path,
) -> Result<Option<HashMap<PathBuf, FileStat>>> {
    if !is_git_repo_at(workspace) {
        return Ok(None);
    }

    let output = std::process::Command::new("git")
        .args([
            "-c",
            "core.quotepath=off",
            "diff",
            "--numstat",
            "HEAD",
            "--",
        ])
        .current_dir(workspace)
        .output()
        .with_context(|| {
            format!(
                "Failed to read git working tree numstat for {}",
                workspace.display()
            )
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut snapshot = HashMap::new();

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.splitn(3, '\t');
        let additions_raw = parts.next().unwrap_or_default();
        let deletions_raw = parts.next().unwrap_or_default();
        let path_raw = parts.next().unwrap_or_default();
        if path_raw.is_empty() {
            continue;
        }

        let additions = additions_raw.parse::<u64>().unwrap_or(0);
        let deletions = deletions_raw.parse::<u64>().unwrap_or(0);
        snapshot.insert(
            PathBuf::from(path_raw),
            FileStat {
                additions,
                deletions,
            },
        );
    }

    Ok(Some(snapshot))
}

pub(crate) fn compute_session_code_change_delta(
    start: Option<&HashMap<PathBuf, FileStat>>,
    end: Option<&HashMap<PathBuf, FileStat>>,
) -> Option<CodeChangeDelta> {
    let (Some(start), Some(end)) = (start, end) else {
        return None;
    };

    let mut keys = HashSet::new();
    keys.extend(start.keys().cloned());
    keys.extend(end.keys().cloned());

    let mut delta = CodeChangeDelta::default();
    for key in keys {
        let start_stat = start.get(&key).copied().unwrap_or_default();
        let end_stat = end.get(&key).copied().unwrap_or_default();
        delta.additions = delta
            .additions
            .saturating_add(end_stat.additions.saturating_sub(start_stat.additions));
        delta.deletions = delta
            .deletions
            .saturating_add(end_stat.deletions.saturating_sub(start_stat.deletions));
    }

    Some(delta)
}

pub(crate) fn git_status_summary(workspace: &Path) -> Result<Option<GitStatusSummary>> {
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace)
        .output()
        .with_context(|| format!("Failed to read git branch for {}", workspace.display()))?;

    if !branch_output.status.success() {
        return Ok(None);
    }

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();
    if branch.is_empty() {
        return Ok(None);
    }

    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace)
        .output()
        .with_context(|| format!("Failed to read git status for {}", workspace.display()))?;

    if !status_output.status.success() {
        return Ok(None);
    }

    let dirty = !String::from_utf8_lossy(&status_output.stdout)
        .trim()
        .is_empty();

    Ok(Some(GitStatusSummary { branch, dirty }))
}

pub(crate) async fn confirm_changes_with_git_diff(
    modified_files: &[String],
    skip_confirmations: bool,
) -> Result<bool> {
    if skip_confirmations {
        return Ok(true);
    }

    // Run blocking git repo check in a spawned blocking task
    let is_repo = tokio::task::spawn_blocking(is_git_repo)
        .await
        .context("Failed to spawn blocking git check")?;

    if !is_repo {
        tracing::debug!("Not in a git repository; skipping diff confirmation.");
        return Ok(true);
    }

    for file in modified_files {
        // Wrap blocking git diff command in spawn_blocking to avoid blocking the async runtime
        // See: https://tokio.rs/tokio/tutorial/select#the-select-macro
        let file_clone = file.clone();
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .args(["diff", &file_clone])
                // Disable pager and force no-color to avoid encoding issues with external pagers
                .env("GIT_PAGER", "cat")
                .output()
        })
        .await
        .context("Failed to spawn blocking git diff")?
        .with_context(|| format!("Failed to run git diff for {}", file))?;

        let diff = String::from_utf8_lossy(&output.stdout);
        if !diff.is_empty() {
            // In TUI mode, skip interactive confirmation to avoid corrupting display
            // The TUI has its own confirmation mechanisms
            if is_tui_mode() {
                tracing::debug!("Git diff for {}: {} bytes", file, diff.len());
                continue;
            }

            println!("Changes to {}:\n{}", file, diff);
            print!("Apply these changes? (y/n): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                let file_clone = file.clone();
                tokio::task::spawn_blocking(move || {
                    std::process::Command::new("git")
                        .args(["checkout", "--", &file_clone])
                        .status()
                })
                .await
                .context("Failed to spawn blocking git checkout")?
                .with_context(|| format!("Failed to revert {}", file))?;
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        CodeChangeDelta, DirtyWorktreeStatus, FileStat, compute_session_code_change_delta,
        git_diff_for_path, git_diff_preview_for_path, git_dirty_worktree_entries,
        normalize_workspace_path, workspace_relative_display,
    };
    use hashbrown::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn session_code_change_delta_is_net_positive_only() {
        let mut start = HashMap::new();
        start.insert(
            PathBuf::from("src/a.rs"),
            FileStat {
                additions: 10,
                deletions: 5,
            },
        );

        let mut end = HashMap::new();
        end.insert(
            PathBuf::from("src/a.rs"),
            FileStat {
                additions: 12,
                deletions: 8,
            },
        );

        let delta = compute_session_code_change_delta(Some(&start), Some(&end));
        assert_eq!(
            delta,
            Some(CodeChangeDelta {
                additions: 2,
                deletions: 3
            })
        );
    }

    #[test]
    fn session_code_change_delta_handles_new_and_reduced_files() {
        let mut start = HashMap::new();
        start.insert(
            PathBuf::from("src/preexisting.rs"),
            FileStat {
                additions: 20,
                deletions: 10,
            },
        );

        let mut end = HashMap::new();
        end.insert(
            PathBuf::from("src/preexisting.rs"),
            FileStat {
                additions: 5,
                deletions: 2,
            },
        );
        end.insert(
            PathBuf::from("src/new.rs"),
            FileStat {
                additions: 7,
                deletions: 1,
            },
        );

        let delta = compute_session_code_change_delta(Some(&start), Some(&end));
        assert_eq!(
            delta,
            Some(CodeChangeDelta {
                additions: 7,
                deletions: 1
            })
        );
    }

    #[test]
    fn session_code_change_delta_requires_both_snapshots() {
        assert_eq!(compute_session_code_change_delta(None, None), None);
    }

    fn init_repo() -> TempDir {
        let temp = TempDir::new().expect("temp dir");
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(temp.path())
                .status()
                .expect("git command");
            assert!(status.success(), "git command failed: {args:?}");
        };

        run(&["init"]);
        run(&["config", "user.name", "VT Code"]);
        run(&["config", "user.email", "vtcode@example.com"]);
        temp
    }

    #[test]
    fn workspace_display_prefers_relative_path() {
        let workspace = Path::new("/workspace");
        let path = Path::new("/workspace/docs/project/TODO.md");
        assert_eq!(
            workspace_relative_display(workspace, path),
            "docs/project/TODO.md"
        );
    }

    #[test]
    fn normalize_workspace_path_canonicalizes_parent_for_missing_files() {
        let temp = TempDir::new().expect("temp dir");
        let normalized = normalize_workspace_path(temp.path(), Path::new("docs/missing.md"));
        assert_eq!(normalized, temp.path().join("docs/missing.md"));
    }

    #[test]
    fn dirty_worktree_entries_parse_modified_files() {
        let repo = init_repo();
        fs::create_dir_all(repo.path().join("docs/project")).expect("mkdir");
        fs::write(repo.path().join("docs/project/TODO.md"), "before\n").expect("write");

        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .status()
                .expect("git command");
            assert!(status.success(), "git command failed: {args:?}");
        };

        run(&["add", "."]);
        run(&["commit", "-m", "test: seed repo"]);

        fs::write(repo.path().join("docs/project/TODO.md"), "after\n").expect("write");
        let entries = git_dirty_worktree_entries(repo.path())
            .expect("entries")
            .expect("git repo");

        assert_eq!(entries.len(), 1);
        assert_eq!(
            workspace_relative_display(repo.path(), &entries[0].path),
            "docs/project/TODO.md"
        );
        assert_eq!(entries[0].status, DirtyWorktreeStatus::Modified);
        assert!(!entries[0].fingerprint.is_empty());
    }

    #[test]
    fn git_diff_helpers_return_single_file_preview() {
        let repo = init_repo();
        fs::create_dir_all(repo.path().join("docs/project")).expect("mkdir");
        fs::write(repo.path().join("docs/project/TODO.md"), "before\n").expect("write");

        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .status()
                .expect("git command");
            assert!(status.success(), "git command failed: {args:?}");
        };

        run(&["add", "."]);
        run(&["commit", "-m", "test: seed repo"]);

        let path = repo.path().join("docs/project/TODO.md");
        fs::write(&path, "after\n").expect("write");

        let diff = git_diff_for_path(repo.path(), &path)
            .expect("diff")
            .expect("non-empty diff");
        assert!(diff.contains("docs/project/TODO.md"));
        assert!(diff.contains("-before"));
        assert!(diff.contains("+after"));

        let preview = git_diff_preview_for_path(repo.path(), &path)
            .expect("preview")
            .expect("preview");
        assert_eq!(preview.display_path, "docs/project/TODO.md");
        assert_eq!(preview.before, "before\n");
        assert_eq!(preview.after, "after\n");
        assert!(!preview.used_fallback_preview);
    }
}
