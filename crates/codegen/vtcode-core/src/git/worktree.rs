//! Git worktree management for loop isolation.
//!
//! A `WorktreeManager` creates, lists, and removes git worktrees under
//! `{workspace}/.vtcode/worktrees/`. Each parallel loop run gets its own
//! worktree so concurrent agents cannot collide on the working tree.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

const WORKTREES_DIR_NAME: &str = "worktrees";

// ─── WorktreeInfo ────────────────────────────────────────────────────────────

/// Information about a discovered worktree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// The worktree name (directory name under `.vtcode/worktrees/`).
    pub name: String,
    /// Absolute path to the worktree working directory.
    pub path: PathBuf,
    /// The HEAD commit hash of the worktree.
    pub head: Option<String>,
    /// Whether the worktree has uncommitted changes.
    pub is_dirty: bool,
}

// ─── WorktreeManager ─────────────────────────────────────────────────────────

/// Manages git worktrees for loop isolation. Each worktree is an independent
/// checkout of the repository that can be worked on in parallel without
/// interfering with other worktrees or the main working tree.
pub struct WorktreeManager {
    workspace_root: PathBuf,
}

impl WorktreeManager {
    /// Create a new `WorktreeManager` for the given workspace.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self { workspace_root: workspace_root.into() }
    }

    /// The directory where worktrees are stored.
    pub fn worktrees_dir(&self) -> PathBuf {
        self.workspace_root.join(".vtcode").join(WORKTREES_DIR_NAME)
    }

    /// Create a new worktree with the given name. Returns the path to the
    /// new worktree's working directory.
    ///
    /// The worktree is created under `.vtcode/worktrees/{name}/` on a new
    /// branch named `loop/{name}`.
    pub fn create(&self, name: &str) -> Result<PathBuf> {
        let sanitized = sanitize_worktree_name(name);
        if sanitized.is_empty() {
            return Err(anyhow!("Worktree name cannot be empty after sanitization"));
        }

        let worktrees_dir = self.worktrees_dir();
        std::fs::create_dir_all(&worktrees_dir)
            .with_context(|| format!("Failed to create {}", worktrees_dir.display()))?;

        let worktree_path = worktrees_dir.join(&sanitized);
        if worktree_path.exists() {
            return Err(anyhow!("Worktree already exists at {}", worktree_path.display()));
        }

        let branch_name = format!("loop/{sanitized}");

        let output = Command::new("git")
            .args(["worktree", "add", "-b", &branch_name, &worktree_path.to_string_lossy()])
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git worktree add failed: {}", stderr.trim()));
        }

        Ok(worktree_path)
    }

    /// List all worktrees managed by this instance (under `.vtcode/worktrees/`).
    pub fn list(&self) -> Result<Vec<WorktreeInfo>> {
        let worktrees_dir = self.worktrees_dir();
        if !worktrees_dir.exists() {
            return Ok(Vec::new());
        }

        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git worktree list")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git worktree list failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let managed_dir = worktrees_dir.to_string_lossy().to_string();
        let mut worktrees = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_head: Option<String> = None;

        /// Build a WorktreeInfo if the path is under the managed directory.
        fn try_build_info(path: PathBuf, head: &mut Option<String>, managed_dir: &str) -> Option<WorktreeInfo> {
            if !path.starts_with(managed_dir) {
                return None;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            Some(WorktreeInfo { name, path, head: head.take(), is_dirty: false })
        }

        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                // If we have a pending entry, check if it's managed
                if let Some(path) = current_path.take() {
                    if let Some(info) = try_build_info(path, &mut current_head, &managed_dir) {
                        worktrees.push(info);
                    }
                }
                current_path = Some(PathBuf::from(rest));
                current_head = None;
            } else if let Some(rest) = line.strip_prefix("HEAD ") {
                current_head = Some(rest.to_string());
            } else if line == "detached" {
                // Detached HEAD state, head already set
            }
        }

        // Handle last entry
        if let Some(path) = current_path {
            if let Some(info) = try_build_info(path, &mut current_head, &managed_dir) {
                worktrees.push(info);
            }
        }

        // Check dirty status for each worktree
        for wt in &mut worktrees {
            let status_output = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&wt.path)
                .output();
            if let Ok(output) = status_output {
                wt.is_dirty = !output.stdout.is_empty();
            }
        }

        Ok(worktrees)
    }

    /// Remove a worktree by name. Runs `git worktree remove` with `--force`
    /// to handle worktrees with uncommitted changes.
    pub fn remove(&self, name: &str) -> Result<()> {
        let sanitized = sanitize_worktree_name(name);
        if sanitized.is_empty() {
            return Err(anyhow!("Worktree name cannot be empty after sanitization"));
        }
        let worktree_path = self.worktrees_dir().join(&sanitized);

        if !worktree_path.exists() {
            return Err(anyhow!("Worktree '{}' does not exist at {}", name, worktree_path.display()));
        }

        let output = Command::new("git")
            .args(["worktree", "remove", "--force", &worktree_path.to_string_lossy()])
            .current_dir(&self.workspace_root)
            .output()
            .context("Failed to run git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git worktree remove failed: {}", stderr.trim()));
        }

        // Clean up the orphan branch created by `create()`.
        let branch = format!("loop/{sanitized}");
        let branch_output = Command::new("git")
            .args(["branch", "-D", &branch])
            .current_dir(&self.workspace_root)
            .output();
        match branch_output {
            Ok(o) if !o.status.success() => {
                // Branch may not exist if the worktree was created externally;
                // log but do not fail the removal.
                tracing::debug!(
                    branch = %branch,
                    stderr = %String::from_utf8_lossy(&o.stderr),
                    "Could not delete orphan branch (may not exist)"
                );
            }
            Err(e) => {
                tracing::debug!(error = %e, "Failed to spawn git branch -D");
            }
            _ => {}
        }

        Ok(())
    }

    /// Remove all worktrees managed by this instance.
    pub fn remove_all(&self) -> Result<usize> {
        let worktrees = self.list()?;
        let count = worktrees.len();
        for wt in &worktrees {
            self.remove(&wt.name)?;
        }
        Ok(count)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn sanitize_worktree_name(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sanitize_worktree_name_basic() {
        assert_eq!(sanitize_worktree_name("my-loop"), "my-loop");
        assert_eq!(sanitize_worktree_name("my_loop"), "my_loop");
        assert_eq!(sanitize_worktree_name("loop123"), "loop123");
    }

    #[test]
    fn sanitize_worktree_name_replaces_special_chars() {
        assert_eq!(sanitize_worktree_name("my loop"), "my-loop");
        assert_eq!(sanitize_worktree_name("path/to/thing"), "path-to-thing");
        assert_eq!(sanitize_worktree_name("a@b#c"), "a-b-c");
    }

    #[test]
    fn sanitize_worktree_name_trims_dashes() {
        assert_eq!(sanitize_worktree_name("--test--"), "test");
        assert_eq!(sanitize_worktree_name("  spaces  "), "spaces");
    }

    #[test]
    fn sanitize_worktree_name_empty_after_sanitize() {
        assert_eq!(sanitize_worktree_name("///"), "");
        assert_eq!(sanitize_worktree_name(""), "");
    }

    #[test]
    fn worktree_manager_worktrees_dir() {
        let mgr = WorktreeManager::new("/tmp/workspace");
        assert_eq!(mgr.worktrees_dir(), PathBuf::from("/tmp/workspace/.vtcode/worktrees"));
    }

    // Integration tests below exercise the real `git worktree` CLI against a
    // throwaway git repo, fulfilling the plan's B1 "create/list/remove against a
    // temp git repo" verification requirement (previously only sanitization was
    // covered).

    use std::process::Command as ProcCommand;

    /// Build a `WorktreeManager` from a canonicalized repo root.
    ///
    /// `git worktree list --porcelain` returns canonical (symlink-resolved)
    /// paths, so the manager must be constructed from a canonical root or its
    /// `starts_with(managed_dir)` filter would miss worktrees when the caller
    /// passes a symlinked path (e.g. macOS `/tmp` -> `/private/tmp`).
    fn manager_for(repo: &TempDir) -> WorktreeManager {
        WorktreeManager::new(std::fs::canonicalize(repo.path()).expect("canonicalize repo"))
    }

    fn init_temp_git_repo() -> TempDir {
        let dir = TempDir::new().expect("temp dir");
        let run = |args: &[&str]| {
            let status = ProcCommand::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("spawn git");
            assert!(status.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&status.stderr));
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@vtcode.dev"]);
        run(&["config", "user.name", "vtcode-test"]);
        std::fs::write(dir.path().join("README.md"), "seed\n").expect("write seed");
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "seed"]);
        dir
    }

    #[test]
    fn create_then_list_returns_managed_worktree() {
        let repo = init_temp_git_repo();
        let mgr = manager_for(&repo);

        let path = mgr.create("loop-a").expect("create");
        assert!(path.exists(), "worktree directory must exist after create");

        let worktrees = mgr.list().expect("list");
        let found = worktrees.iter().find(|w| w.name == "loop-a");
        assert!(found.is_some(), "created worktree should be listed");
        assert_eq!(found.unwrap().path, path);
    }

    #[test]
    fn create_is_idempotency_safe() {
        let repo = init_temp_git_repo();
        let mgr = manager_for(&repo);
        mgr.create("loop-b").expect("create first");

        let err = mgr.create("loop-b");
        assert!(err.is_err(), "re-creating an existing worktree must fail");
    }

    #[test]
    fn remove_deletes_worktree_and_orphan_branch() {
        let repo = init_temp_git_repo();
        let mgr = manager_for(&repo);
        mgr.create("loop-c").expect("create");

        mgr.remove("loop-c").expect("remove");
        assert!(
            mgr.list().expect("list").iter().all(|w| w.name != "loop-c"),
            "worktree should no longer be listed after removal"
        );

        // The orphan `loop/{name}` branch created by `create()` must be GC'd too.
        let branch_check = ProcCommand::new("git")
            .args(["rev-parse", "--verify", "loop/loop-c"])
            .current_dir(repo.path())
            .output()
            .expect("check branch");
        assert!(!branch_check.status.success(), "orphan branch should be deleted on remove");
    }

    #[test]
    fn remove_missing_worktree_errors() {
        let repo = init_temp_git_repo();
        let mgr = manager_for(&repo);
        assert!(mgr.remove("does-not-exist").is_err(), "removing a non-existent worktree must error");
    }

    #[test]
    fn remove_all_clears_every_managed_worktree() {
        let repo = init_temp_git_repo();
        let mgr = manager_for(&repo);
        mgr.create("loop-x").expect("create x");
        mgr.create("loop-y").expect("create y");

        let removed = mgr.remove_all().expect("remove_all");
        assert_eq!(removed, 2);
        assert!(mgr.list().expect("list").is_empty());
    }
}
