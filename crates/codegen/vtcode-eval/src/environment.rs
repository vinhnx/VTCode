//! Environment probes used to verify eval-task outcomes.
//!
//! The probe set is closed (external command, file existence, clean git tree),
//! so it is modelled as an enum instead of `Box<dyn EnvironmentProbe>`. That
//! removes a heap allocation and a vtable dispatch per probe while keeping the
//! public API easy to extend with a new variant.

use std::path::{Path, PathBuf};

/// A single verification check run against an eval workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentProbe {
    /// Runs an external command in the workspace; passes on exit status 0.
    Command { command: String, args: Vec<String> },
    /// Passes when `path` exists; relative paths resolve against the workspace.
    FileExists { path: PathBuf },
    /// Passes when the workspace git tree has no uncommitted changes.
    GitClean,
}

impl EnvironmentProbe {
    /// Build a command probe from a command name and its arguments.
    pub fn command(command: String, args: Vec<String>) -> Self {
        Self::Command { command, args }
    }

    /// Build a file-existence probe.
    pub fn file_exists(path: PathBuf) -> Self {
        Self::FileExists { path }
    }

    /// Build a clean-git-tree probe.
    pub fn git_clean() -> Self {
        Self::GitClean
    }

    /// Run this probe against `workspace`, returning `true` on success.
    ///
    /// Probe failures are reported as `false` rather than errors: a missing
    /// file or a failing command is a failed verification, not a broken runner.
    pub fn check(&self, workspace: &Path) -> bool {
        match self {
            Self::Command { command, args } => std::process::Command::new(command)
                .args(args)
                .current_dir(workspace)
                .output()
                .is_ok_and(|output| output.status.success()),
            // `join` keeps absolute paths unchanged and anchors relative ones
            // to the eval workspace (each attempt may run in its own worktree).
            Self::FileExists { path } => workspace.join(path).exists(),
            Self::GitClean => std::process::Command::new("git")
                .args(["diff", "--quiet"])
                .current_dir(workspace)
                .output()
                .is_ok_and(|output| output.status.success()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_exists_probe_checks_both_sides() {
        let workspace = std::env::current_dir().expect("current dir");
        assert!(EnvironmentProbe::file_exists(workspace.clone()).check(&workspace));

        let missing = workspace.join(format!("missing-probe-{}", std::process::id()));
        assert!(!EnvironmentProbe::file_exists(missing).check(&workspace));
    }

    #[test]
    fn file_exists_probe_resolves_relative_paths_against_workspace() {
        let marker = format!("vtcode-probe-marker-{}", std::process::id());
        let root = std::env::temp_dir().join(format!("vtcode-probe-root-{}", std::process::id()));
        let workspace = root.join("worktree");
        std::fs::create_dir_all(&workspace).expect("create probe workspace");
        std::fs::write(workspace.join(&marker), b"present").expect("create probe marker");

        // Relative path: only present under the workspace, never the process CWD.
        assert!(EnvironmentProbe::file_exists(PathBuf::from(&marker)).check(&workspace));
        // Absolute path: still accepted unchanged.
        assert!(EnvironmentProbe::file_exists(workspace.join(&marker)).check(&workspace));
        // Missing relative path: rejected on both sides of the boundary.
        assert!(!EnvironmentProbe::file_exists(PathBuf::from(format!("{marker}-missing"))).check(&workspace));

        std::fs::remove_dir_all(&root).expect("clean up probe workspace");
    }

    #[cfg(unix)]
    #[test]
    fn command_probe_uses_exit_status() {
        let workspace = std::env::current_dir().expect("current dir");
        assert!(EnvironmentProbe::command("true".into(), Vec::new()).check(&workspace));
        assert!(!EnvironmentProbe::command("false".into(), Vec::new()).check(&workspace));
    }
}
