use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::Path;
use std::path::PathBuf;

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
            if std::env::var("VTCODE_TUI_MODE").is_ok() {
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
    use super::{CodeChangeDelta, FileStat, compute_session_code_change_delta};
    use std::collections::HashMap;
    use std::path::PathBuf;

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
}
