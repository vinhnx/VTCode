//! Git repository information collection
//!
//! This module provides utilities for collecting git metadata from the workspace,
//! similar to OpenAI Codex PR #10145. It collects remote URLs, HEAD commit hash,
//! and repository root path for inclusion in LLM request headers.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Git repository information for a workspace
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitInfo {
    /// Remote URLs keyed by remote name (e.g., "origin")
    pub remotes: BTreeMap<String, String>,
    /// HEAD commit hash (short form)
    pub head_commit: Option<String>,
    /// Repository root path
    pub repo_root: Option<String>,
}

/// Get git remote URLs for fetch remotes in the repository at the given path.
/// Returns a BTreeMap mapping remote names to their fetch URLs.
///
/// # Arguments
/// * `cwd` - The working directory to run git commands in
///
/// # Returns
/// A BTreeMap where keys are remote names (e.g., "origin") and values are fetch URLs.
/// Returns an empty map if not in a git repository or if no remotes are configured.
pub fn get_git_remote_urls(cwd: &Path) -> Result<BTreeMap<String, String>> {
    let output = Command::new("git")
        .args(["remote", "-v"])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git remote -v in {}", cwd.display()))?;

    if !output.status.success() {
        // Not a git repository or git not available
        return Ok(BTreeMap::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut remotes = BTreeMap::new();

    // Parse output like:
    // origin  https://github.com/user/repo.git (fetch)
    // origin  https://github.com/user/repo.git (push)
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let name = parts[0].to_string();
            let url = parts[1].to_string();
            let purpose = parts[2].trim_matches(|c| c == '(' || c == ')');

            // Only collect fetch remotes to avoid duplicates
            if purpose == "fetch" {
                remotes.insert(name, url);
            }
        }
    }

    Ok(remotes)
}

/// Get the HEAD commit hash (short form) for the repository at the given path.
///
/// # Arguments
/// * `cwd` - The working directory to run git commands in
///
/// # Returns
/// The short commit hash (7 characters) of HEAD, or None if not in a git repository.
pub fn get_head_commit_hash(cwd: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git rev-parse in {}", cwd.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let hash = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();

    if hash.is_empty() {
        Ok(None)
    } else {
        Ok(Some(hash))
    }
}

/// Get the repository root path for the given working directory.
///
/// # Arguments
/// * `cwd` - The working directory to run git commands in
///
/// # Returns
/// The absolute path to the repository root, or None if not in a git repository.
pub fn get_git_repo_root(cwd: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git rev-parse --show-toplevel in {}", cwd.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let root = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();

    if root.is_empty() {
        Ok(None)
    } else {
        Ok(Some(root))
    }
}

/// Collect all git information for a workspace.
///
/// # Arguments
/// * `cwd` - The working directory to collect git info from
///
/// # Returns
/// A GitInfo struct containing remote URLs, HEAD commit hash, and repo root.
/// Returns default GitInfo if not in a git repository.
pub fn collect_git_info(cwd: &Path) -> Result<GitInfo> {
    let remotes = get_git_remote_urls(cwd)?;
    let head_commit = get_head_commit_hash(cwd)?;
    let repo_root = get_git_repo_root(cwd)?;

    Ok(GitInfo {
        remotes,
        head_commit,
        repo_root,
    })
}

/// Check if the given path is inside a git repository.
///
/// # Arguments
/// * `cwd` - The working directory to check
///
/// # Returns
/// true if inside a git repository, false otherwise.
pub fn is_git_repo(cwd: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_git_repo() {
        // The vtcode repo itself should be a git repo
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert!(is_git_repo(&repo_root));
    }

    #[test]
    fn test_get_git_repo_root() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = get_git_repo_root(&repo_root).unwrap();
        assert!(root.is_some());
        // The root should contain the path
        assert!(root.unwrap().contains("vtcode"));
    }

    #[test]
    fn test_get_head_commit_hash() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let hash = get_head_commit_hash(&repo_root).unwrap();
        assert!(hash.is_some());
        // Short hash should be 7-12 characters
        let hash_str = hash.unwrap();
        assert!(hash_str.len() >= 7 && hash_str.len() <= 12);
        // Should only contain hex characters
        assert!(hash_str.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_collect_git_info() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let info = collect_git_info(&repo_root).unwrap();

        // Should have a HEAD commit
        assert!(info.head_commit.is_some());

        // Should have a repo root
        assert!(info.repo_root.is_some());

        // Remotes may or may not be present depending on git config
        // but the function should not error
    }

    #[test]
    fn test_non_git_directory() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let non_git_path = temp_dir.path().join("not_a_repo");
        fs::create_dir(&non_git_path).unwrap();

        // Should return empty results without error
        assert!(!is_git_repo(&non_git_path));
        assert!(get_git_remote_urls(&non_git_path).unwrap().is_empty());
        assert!(get_head_commit_hash(&non_git_path).unwrap().is_none());
        assert!(get_git_repo_root(&non_git_path).unwrap().is_none());
    }
}
