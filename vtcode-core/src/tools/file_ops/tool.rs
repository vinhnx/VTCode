//! File operations tool implementation.

use crate::tools::grep_file::GrepSearchManager;
use crate::tools::traits::{CacheableTool, FileTool, ModeTool, Tool};
use crate::tools::types::{ListInput, PathArgs};
use crate::utils::path::canonicalize_workspace;
use crate::utils::vtcodegitignore::should_exclude_file;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// File operations tool with multiple modes
#[derive(Clone)]
pub struct FileOpsTool {
    pub(super) workspace_root: PathBuf,
    pub(super) canonical_workspace_root: PathBuf,
    pub(super) grep_manager: Arc<GrepSearchManager>,
}

impl FileOpsTool {
    pub fn new(workspace_root: PathBuf, grep_search: Arc<GrepSearchManager>) -> Self {
        // grep_file manager is unused; keep param to avoid broad call-site churn
        let canonical_workspace_root = canonicalize_workspace(&workspace_root);

        Self {
            workspace_root,
            canonical_workspace_root,
            grep_manager: grep_search,
        }
    }

    /// Get relative path from workspace root, avoiding allocation when possible
    #[inline]
    pub(super) fn relative_path<'a>(&self, path: &'a Path) -> Cow<'a, str> {
        path.strip_prefix(&self.workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
    }

    /// Get relative path as JSON value (for API responses)
    #[inline]
    pub(super) fn relative_path_json(&self, path: &Path) -> String {
        self.relative_path(path).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::diff;
    use crate::tools::file_ops::diff_preview::{build_diff_preview, diff_preview_error_skip};

    #[test]
    fn diff_preview_reports_truncation_and_omission() {
        let after = (0..(diff::MAX_PREVIEW_LINES + 40))
            .map(|idx| format!("line {idx}\n"))
            .collect::<String>();

        let preview = build_diff_preview("sample.txt", None, &after);

        assert_eq!(preview["skipped"], Value::Bool(false));
        assert_eq!(preview["truncated"], Value::Bool(true));
        assert!(preview["omitted_line_count"].as_u64().unwrap() > 0);

        let content = preview["content"].as_str().unwrap();
        assert!(content.contains("lines omitted"));
        assert!(content.lines().count() <= diff::HEAD_LINE_COUNT + diff::TAIL_LINE_COUNT + 1);
    }

    #[test]
    fn diff_preview_skip_handles_error_detail() {
        let preview = diff_preview_error_skip("failed", Some("InvalidData"));
        assert_eq!(preview["reason"], Value::String("failed".to_string()));
        assert_eq!(preview["detail"], Value::String("InvalidData".to_string()));
        assert_eq!(preview["skipped"], Value::Bool(true));
    }
}

#[async_trait]
impl Tool for FileOpsTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let mut input: ListInput = serde_json::from_value(args.clone()).context(
            "Error: Invalid 'list_files' arguments. Expected JSON object with: path (required, string). Optional: mode (string), max_items (number), page (number), per_page (number), include_hidden (bool), response_format (string). Example: {\"path\": \"src\", \"page\": 1, \"per_page\": 50, \"response_format\": \"concise\"}",
        )?;

        // Standard path extraction from args (handles aliases)
        let path_args: PathArgs = serde_json::from_value(args).unwrap_or(PathArgs {
            path: input.path.clone(),
        });
        input.path = path_args.path;

        // Normalize path: strip /workspace prefix if present (common LLM pattern)
        if input.path.starts_with("/workspace/") {
            input.path = input.path.strip_prefix("/workspace/").unwrap().to_string();
        } else if input.path == "/workspace" {
            input.path = ".".to_string();
        }

        // Block root directory listing to prevent loops
        let normalized_path = input.path.trim_start_matches("./").trim_start_matches('/');
        if normalized_path.is_empty() || normalized_path == "." {
            return Err(anyhow!(
                "Error: list_files on root directory is blocked to prevent infinite loops. \
                 Please specify a subdirectory like 'src/', 'vtcode-core/src/', 'tests/', etc. \
                 Use grep_file with a pattern to search across the entire workspace."
            ));
        }

        let mode_clone = input.mode.clone();
        let mode = mode_clone.as_deref().unwrap_or("list");
        self.execute_mode(mode, serde_json::to_value(input)?).await
    }

    fn name(&self) -> &'static str {
        "list_files"
    }

    fn description(&self) -> &'static str {
        "Enhanced file discovery tool with multiple modes: list (default), recursive, find_name, find_content, largest (size ranking), tree (visual directory structure)"
    }
}

#[async_trait]
impl FileTool for FileOpsTool {
    fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    async fn should_exclude(&self, path: &Path) -> bool {
        should_exclude_file(path).await
    }
}

#[async_trait]
impl ModeTool for FileOpsTool {
    fn supported_modes(&self) -> Vec<&'static str> {
        vec![
            "list",
            "recursive",
            "find_name",
            "find_content",
            "largest",
            "tree",
        ]
    }

    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value> {
        let input: ListInput = serde_json::from_value(args)?;

        match mode {
            "list" => self.execute_basic_list(&input).await,
            "recursive" => self.execute_recursive_search(&input).await,
            "find_name" => self.execute_find_by_name(&input).await,
            "find_content" => self.execute_find_by_content(&input).await,
            "largest" => self.execute_largest_files(&input).await,
            "tree" => self.execute_tree_view(&input).await,
            _ => Err(anyhow!("Unsupported file operation mode: {}", mode)),
        }
    }
}

#[async_trait]
impl CacheableTool for FileOpsTool {
    fn cache_key(&self, args: &Value) -> String {
        format!(
            "files:{}:{}",
            args.get("path").and_then(|p| p.as_str()).unwrap_or(""),
            args.get("mode").and_then(|m| m.as_str()).unwrap_or("list")
        )
    }

    fn should_cache(&self, args: &Value) -> bool {
        // Cache list and recursive modes, but not content-based searches
        let mode = args.get("mode").and_then(|m| m.as_str()).unwrap_or("list");
        matches!(mode, "list" | "recursive" | "largest")
    }

    fn cache_ttl(&self) -> u64 {
        60 // 1 minute for file listings
    }
}
