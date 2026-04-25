//! File operations tool implementation.

use crate::config::constants::tools;
use crate::tools::edited_file_monitor::EditedFileMonitor;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::traits::{CacheableTool, FileTool, ModeTool, Tool};
use crate::tools::types::{ListInput, PathArgs};
use crate::tools::validation::paths::validate_non_root_listing_path;
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
    pub(super) edited_file_monitor: Arc<EditedFileMonitor>,
}

impl FileOpsTool {
    pub fn new(workspace_root: PathBuf, grep_search: Arc<GrepSearchManager>) -> Self {
        let edited_file_monitor = Arc::new(EditedFileMonitor::new());
        Self::new_with_monitor(workspace_root, grep_search, edited_file_monitor)
    }

    pub fn new_with_monitor(
        workspace_root: PathBuf,
        grep_search: Arc<GrepSearchManager>,
        edited_file_monitor: Arc<EditedFileMonitor>,
    ) -> Self {
        // grep_file manager is unused; keep param to avoid broad call-site churn
        let canonical_workspace_root = canonicalize_workspace(&workspace_root);

        Self {
            workspace_root,
            canonical_workspace_root,
            grep_manager: grep_search,
            edited_file_monitor,
        }
    }

    /// Borrow the edited-file monitor without exposing shared ownership.
    pub fn edited_file_monitor_ref(&self) -> &EditedFileMonitor {
        self.edited_file_monitor.as_ref()
    }

    /// Get the shared edited-file monitor handle for callers that need to clone it.
    pub fn edited_file_monitor(&self) -> &Arc<EditedFileMonitor> {
        &self.edited_file_monitor
    }

    fn normalize_list_mode(mode: &str) -> Option<&'static str> {
        if mode.eq_ignore_ascii_case("list")
            || mode.eq_ignore_ascii_case("file")
            || mode.eq_ignore_ascii_case("files")
        {
            Some("list")
        } else if mode.eq_ignore_ascii_case("recursive") {
            Some("recursive")
        } else if mode.eq_ignore_ascii_case("find_name") {
            Some("find_name")
        } else if mode.eq_ignore_ascii_case("find_content") {
            Some("find_content")
        } else if mode.eq_ignore_ascii_case("largest") {
            Some("largest")
        } else if mode.eq_ignore_ascii_case("tree") {
            Some("tree")
        } else {
            None
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
            if let Some(stripped) = input.path.strip_prefix("/workspace/") {
                input.path = stripped.to_string();
            }
        } else if input.path == "/workspace" {
            input.path = ".".to_string();
        }

        validate_non_root_listing_path(Some(input.path.as_str()))?;

        let should_promote_glob_to_recursive = input.mode.is_none()
            && input
                .glob_pattern
                .as_deref()
                .map(str::trim)
                .is_some_and(|pattern| {
                    !pattern.is_empty() && (pattern.contains('/') || pattern.contains("**"))
                });
        if should_promote_glob_to_recursive {
            input.mode = Some("recursive".to_string());
        }

        let raw_mode = input.mode.as_deref().unwrap_or("list").trim().to_string();
        let mode = Self::normalize_list_mode(&raw_mode).unwrap_or(raw_mode.as_str());
        input.mode = Some(mode.to_string());

        self.execute_mode(mode, serde_json::to_value(input)?).await
    }

    fn name(&self) -> &str {
        tools::LIST_FILES
    }

    fn description(&self) -> &str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::diff;
    use crate::tools::file_ops::diff_preview::{build_diff_preview, diff_preview_error_skip};
    use crate::tools::grep_file::GrepSearchManager;
    use serde_json::json;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

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

    #[tokio::test]
    async fn globbed_list_pattern_promotes_to_recursive_mode() {
        let temp_dir = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(temp_dir.path().join("src/nested")).expect("create nested src");
        fs::write(temp_dir.path().join("src/lib.rs"), "pub fn lib() {}\n").expect("write lib");
        fs::write(
            temp_dir.path().join("src/nested/mod.rs"),
            "pub fn nested() {}\n",
        )
        .expect("write nested");
        fs::write(temp_dir.path().join("src/notes.md"), "# notes\n").expect("write notes");

        let grep_manager = Arc::new(GrepSearchManager::new(temp_dir.path().to_path_buf()));
        let file_ops = FileOpsTool::new(temp_dir.path().to_path_buf(), grep_manager);

        let result = file_ops
            .execute(json!({
                "path": "src",
                "pattern": "**/*.rs",
                "response_format": "detailed"
            }))
            .await
            .expect("recursive glob list should succeed");

        assert_eq!(result["mode"], json!("recursive"));
        assert_eq!(result["pattern"], json!("**/*.rs"));

        let items = result["items"].as_array().expect("items array");
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| {
            item["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(".rs"))
        }));
    }

    #[tokio::test]
    async fn file_mode_alias_executes_basic_list() {
        let temp_dir = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(temp_dir.path().join("src")).expect("create src");
        fs::write(temp_dir.path().join("src/lib.rs"), "pub fn lib() {}\n").expect("write lib");

        let grep_manager = Arc::new(GrepSearchManager::new(temp_dir.path().to_path_buf()));
        let file_ops = FileOpsTool::new(temp_dir.path().to_path_buf(), grep_manager);

        let result = file_ops
            .execute(json!({
                "path": "src",
                "mode": "file"
            }))
            .await
            .expect("file alias should behave like list");

        assert_eq!(result["mode"], json!("list"));
        let items = result["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["path"], json!("src/lib.rs"));
    }
}
