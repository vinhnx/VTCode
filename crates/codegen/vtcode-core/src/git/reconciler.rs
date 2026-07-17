//! Worktree reconciliation: diff, verify, merge.
//!
//! After a worktree-isolated subagent finishes, the `WorktreeReconciler`
//! captures the branch diff, asks a [`DiffVerifier`](crate::git::verify::DiffVerifier)
//! to approve or reject it, and merges approved changes back into the main
//! branch.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{DiffVerifier, WorktreeManager};

// ─── ReconcileResult ────────────────────────────────────────────────────────

/// Outcome of a reconcile cycle.
#[derive(Debug, Clone)]
pub struct ReconcileResult {
    /// Whether the verifier approved the change.
    pub approved: bool,
    /// Whether the branch was successfully merged into main.
    pub merged: bool,
    /// Concrete issues identified by the verifier (empty if approved).
    pub issues: Vec<String>,
    /// Free-text reasoning from the verifier or error description.
    pub reasoning: String,
}

// ─── WorktreeReconciler ─────────────────────────────────────────────────────

/// Orchestrates the diff → verify → merge cycle for a completed worktree.
///
/// The reconciler is internal infrastructure and uses `Command::new("git")`
/// directly (consistent with `WorktreeManager`), bypassing the tool safety
/// layer.
pub struct WorktreeReconciler {
    workspace_root: PathBuf,
    main_branch: String,
}

impl WorktreeReconciler {
    /// Create a reconciler for the given workspace.
    ///
    /// `main_branch` defaults to `"main"` if empty.
    pub fn new(workspace_root: impl Into<PathBuf>, main_branch: &str) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            main_branch: if main_branch.is_empty() {
                "main".to_string()
            } else {
                main_branch.to_string()
            },
        }
    }

    /// The branch name for a given worktree name (convention: `loop/{name}`).
    pub fn branch_name(worktree_name: &str) -> String {
        format!("loop/{worktree_name}")
    }

    // ── Git helpers ──────────────────────────────────────────────────────

    /// Run a `git diff` variant with extra args and return stdout.
    fn run_git_diff(&self, base: &str, head: &str, extra_args: &[&str]) -> Result<String> {
        let range = format!("{base}...{head}");
        let mut args = vec!["diff"];
        args.extend_from_slice(extra_args);
        args.push(&range);

        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git diff")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git diff failed: {}", stderr.trim()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run `git diff {base}...{head}` and return the unified diff text.
    pub fn git_diff(&self, base: &str, head: &str) -> Result<String> {
        self.run_git_diff(base, head, &[])
    }

    /// Run `git diff --name-only {base}...{head}` and return changed file paths.
    pub fn git_diff_name_only(&self, base: &str, head: &str) -> Result<Vec<PathBuf>> {
        let stdout = self.run_git_diff(base, head, &["--name-only"])?;
        Ok(stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| PathBuf::from(l.trim()))
            .collect())
    }

    /// Run `git diff --stat {base}...{head}` and return the summary text.
    pub fn git_diff_stat(&self, base: &str, head: &str) -> Result<String> {
        self.run_git_diff(base, head, &["--stat"])
    }

    /// Check whether a branch has any commits ahead of the base.
    pub fn branch_has_commits(&self, base: &str, head: &str) -> Result<bool> {
        let range = format!("{base}..{head}");
        let output = Command::new("git")
            .args(["log", "--oneline", &range])
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git log")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git log failed: {}", stderr.trim()));
        }

        Ok(!output.stdout.is_empty())
    }

    /// Merge a branch into the workspace. Prefers `--ff-only`; falls back
    /// to a standard merge if fast-forward is not possible.
    pub fn git_merge(&self, branch: &str) -> Result<()> {
        // Try fast-forward first.
        let ff_output = Command::new("git")
            .args(["merge", "--ff-only", branch])
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git merge --ff-only")?;

        if ff_output.status.success() {
            return Ok(());
        }

        // Fall back to standard merge.
        let output = Command::new("git")
            .args(["merge", branch])
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git merge")?;

        if !output.status.success() {
            // Abort the failed merge to leave the repo clean.
            let abort_output = Command::new("git")
                .args(["merge", "--abort"])
                .current_dir(&self.workspace_root)
                .output();
            match abort_output {
                Ok(o) if !o.status.success() => {
                    tracing::warn!(
                        stderr = %String::from_utf8_lossy(&o.stderr),
                        "git merge --abort failed; repo may be in a dirty state"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to spawn git merge --abort");
                }
                _ => {}
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git merge failed: {}", stderr.trim()));
        }

        Ok(())
    }

    /// Delete a local branch (must be fully merged).
    pub fn git_branch_delete(&self, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["branch", "-d", branch])
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git branch -d")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git branch -d failed: {}", stderr.trim()));
        }

        Ok(())
    }

    /// Check whether a worktree working directory is clean (no uncommitted changes).
    pub fn is_worktree_clean(&self, worktree_path: &Path) -> Result<bool> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree_path)
            .output()
            .context("Failed to run git status")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git status failed: {}", stderr.trim()));
        }

        Ok(output.stdout.is_empty())
    }

    // ── Core reconcile ───────────────────────────────────────────────────

    /// Reconcile a completed worktree: diff → verify → merge → cleanup.
    ///
    /// 1. Verify the worktree is clean.
    /// 2. Check the branch has commits ahead of main.
    /// 3. Capture the diff and changed files.
    /// 4. Ask `verifier` to approve or reject the diff.
    /// 5. If approved, merge the branch and clean up the worktree.
    /// 6. If rejected, return the result without merging.
    ///
    /// `verifier` receives `(diff_text, changed_files)` and returns a verdict.
    /// This decouples the reconciler from `SubagentController` to keep the git
    /// module free of subagent dependencies.
    pub fn reconcile(
        &self,
        worktree_name: &str,
        worktree_path: &Path,
        verifier: &(dyn DiffVerifier + Send + Sync),
    ) -> Result<ReconcileResult> {
        let branch = Self::branch_name(worktree_name);

        // 1. Pre-flight: worktree must be clean.
        if !self.is_worktree_clean(worktree_path)? {
            return Ok(ReconcileResult {
                approved: false,
                merged: false,
                issues: vec!["Worktree has uncommitted changes".to_string()],
                reasoning: "Subagent did not commit all changes before completing.".to_string(),
            });
        }

        // 2. Check for commits.
        if !self.branch_has_commits(&self.main_branch, &branch)? {
            return Ok(ReconcileResult {
                approved: true,
                merged: false,
                issues: Vec::new(),
                reasoning: "No commits on branch; nothing to merge.".to_string(),
            });
        }

        // 3. Capture diff.
        let diff_text = self.git_diff(&self.main_branch, &branch)?;
        let changed_files = self.git_diff_name_only(&self.main_branch, &branch)?;

        if diff_text.trim().is_empty() {
            return Ok(ReconcileResult {
                approved: true,
                merged: false,
                issues: Vec::new(),
                reasoning: "Empty diff; nothing to merge.".to_string(),
            });
        }

        // 4. Verify.
        let verdict = verifier.verify(&diff_text, &changed_files)?;
        let approved = verdict.approved;
        let issues = verdict.issues;
        let reasoning = verdict.reasoning;

        if !approved {
            return Ok(ReconcileResult { approved: false, merged: false, issues, reasoning });
        }

        // 5. Merge.
        self.git_merge(&branch).context("Failed to merge approved branch")?;

        // 6. Cleanup: delete branch and remove worktree.
        if let Err(e) = self.git_branch_delete(&branch) {
            tracing::warn!(error = %e, branch = %branch, "Failed to delete branch after merge");
        }
        let wm = WorktreeManager::new(&self.workspace_root);
        if let Err(e) = wm.remove(worktree_name) {
            tracing::warn!(error = %e, worktree = %worktree_name, "Failed to remove worktree after merge");
        }

        Ok(ReconcileResult { approved: true, merged: true, issues, reasoning })
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_name_format() {
        assert_eq!(WorktreeReconciler::branch_name("my-loop"), "loop/my-loop");
        assert_eq!(WorktreeReconciler::branch_name("test_1"), "loop/test_1");
    }

    #[test]
    fn reconciler_defaults_main_branch() {
        let r = WorktreeReconciler::new("/tmp/ws", "");
        assert_eq!(r.main_branch, "main");
    }

    #[test]
    fn reconciler_custom_main_branch() {
        let r = WorktreeReconciler::new("/tmp/ws", "develop");
        assert_eq!(r.main_branch, "develop");
    }
}
