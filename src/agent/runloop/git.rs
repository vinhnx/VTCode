use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitStatusSummary {
    pub branch: String,
    pub dirty: bool,
}

fn is_git_repo() -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
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

    if !is_git_repo() {
        println!("Not in a git repository; skipping diff confirmation.");
        return Ok(true);
    }

    for file in modified_files {
        let output = std::process::Command::new("git")
            .args(["diff", file])
            .output()
            .with_context(|| format!("Failed to run git diff for {}", file))?;

        let diff = String::from_utf8_lossy(&output.stdout);
        if !diff.is_empty() {
            println!("Changes to {}:\n{}", file, diff);
            print!("Apply these changes? (y/n): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                std::process::Command::new("git")
                    .args(["checkout", "--", file])
                    .status()
                    .with_context(|| format!("Failed to revert {}", file))?;
                return Ok(false);
            }
        }
    }
    Ok(true)
}
