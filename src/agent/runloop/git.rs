use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;
use vtcode_core::ui::tui::{DiffHunk, InlineEvent, InlineHandle, InlineSession};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitStatusSummary {
    pub branch: String,
    pub dirty: bool,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
        println!("Not in a git repository; skipping diff confirmation.");
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

/// Show a TUI diff preview for file changes and wait for user approval
#[allow(dead_code)]
pub(crate) async fn confirm_with_diff_preview(
    handle: &InlineHandle,
    session: &mut InlineSession,
    file_path: &str,
    before: &str,
    after: &str,
) -> Result<bool> {
    let hunks: Vec<DiffHunk> = vec![];
    
    handle.show_diff_preview(
        file_path.to_string(),
        before.to_string(),
        after.to_string(),
        hunks,
        0,
    );
    
    while let Some(event) = session.next_event().await {
        match event {
            InlineEvent::DiffPreviewApply => return Ok(true),
            InlineEvent::DiffPreviewReject => return Ok(false),
            _ => continue,
        }
    }
    
    Ok(false)
}
