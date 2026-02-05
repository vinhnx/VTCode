//! Turn metadata for LLM requests
//!
//! This module provides utilities for building turn metadata headers that are
//! sent with LLM requests, similar to OpenAI Codex PR #10145. The metadata
//! includes workspace information like git remote URLs and commit hash.

use crate::git_info::{self};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Workspace information included in turn metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceInfo {
    /// Git remote URLs keyed by remote name
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub remote_urls: BTreeMap<String, String>,
    /// HEAD commit hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    /// Repository root path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
}

/// Turn metadata structure sent as X-Turn-Metadata header
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnMetadata {
    /// Workspace information
    pub workspace: WorkspaceInfo,
}

/// Build the turn metadata header value for the given working directory.
///
/// This function collects git information from the workspace and formats it
/// as a JSON string suitable for use as the X-Turn-Metadata header value.
///
/// # Arguments
/// * `cwd` - The working directory to collect metadata from
///
/// # Returns
/// A JSON string containing the turn metadata, or an empty string if
/// metadata collection fails or the directory is not in a git repository.
///
/// # Example
/// ```
/// use std::path::Path;
/// use vtcode_core::turn_metadata::build_turn_metadata_header;
///
/// let metadata = build_turn_metadata_header(Path::new(".")).unwrap();
/// // metadata will be a JSON string like:
/// // {"workspace":{"remote_urls":{"origin":"https://github.com/user/repo.git"},"commit_hash":"abc1234"}}
/// ```
pub fn build_turn_metadata_header(cwd: &Path) -> Result<String> {
    if !git_info::is_git_repo(cwd) {
        return Ok(String::new());
    }

    let git_info = git_info::collect_git_info(cwd)?;

    let metadata = TurnMetadata {
        workspace: WorkspaceInfo {
            remote_urls: git_info.remotes,
            commit_hash: git_info.head_commit,
            repo_root: git_info.repo_root,
        },
    };

    // Serialize to compact JSON
    let json = serde_json::to_string(&metadata)?;
    Ok(json)
}

/// Build turn metadata as a serde_json::Value for direct use in LLM requests.
///
/// # Arguments
/// * `cwd` - The working directory to collect metadata from
///
/// # Returns
/// A serde_json::Value containing the turn metadata, or Null if
/// metadata collection fails or the directory is not in a git repository.
pub fn build_turn_metadata_value(cwd: &Path) -> Result<Value> {
    if !git_info::is_git_repo(cwd) {
        return Ok(Value::Null);
    }

    let git_info = git_info::collect_git_info(cwd)?;

    let metadata = TurnMetadata {
        workspace: WorkspaceInfo {
            remote_urls: git_info.remotes,
            commit_hash: git_info.head_commit,
            repo_root: git_info.repo_root,
        },
    };

    let value = serde_json::to_value(metadata)?;
    Ok(value)
}

/// Get the header name for turn metadata.
/// This is the header key used when sending metadata to LLM providers.
pub const TURN_METADATA_HEADER: &str = "X-Turn-Metadata";

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_turn_metadata_header() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let metadata = build_turn_metadata_header(&repo_root).unwrap();

        // Should produce valid JSON
        let parsed: Value = serde_json::from_str(&metadata).unwrap();
        assert!(parsed.get("workspace").is_some());
    }

    #[test]
    fn test_build_turn_metadata_value() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let value = build_turn_metadata_value(&repo_root).unwrap();

        assert!(!value.is_null());
        assert!(value.get("workspace").is_some());
    }

    #[test]
    fn test_turn_metadata_header_constant() {
        assert_eq!(TURN_METADATA_HEADER, "X-Turn-Metadata");
    }

    #[test]
    fn test_workspace_info_serialization() {
        let mut remotes = BTreeMap::new();
        remotes.insert(
            "origin".to_string(),
            "https://github.com/user/repo.git".to_string(),
        );

        let workspace = WorkspaceInfo {
            remote_urls: remotes,
            commit_hash: Some("abc1234".to_string()),
            repo_root: Some("/path/to/repo".to_string()),
        };

        let json = serde_json::to_string(&workspace).unwrap();
        assert!(json.contains("origin"));
        assert!(json.contains("abc1234"));
        assert!(json.contains("/path/to/repo"));
    }

    #[test]
    fn test_empty_remotes_skipped() {
        let workspace = WorkspaceInfo {
            remote_urls: BTreeMap::new(),
            commit_hash: Some("abc1234".to_string()),
            repo_root: None,
        };

        let json = serde_json::to_string(&workspace).unwrap();
        // Empty remotes should be skipped due to skip_serializing_if
        assert!(!json.contains("remote_urls"));
        assert!(json.contains("commit_hash"));
    }
}
