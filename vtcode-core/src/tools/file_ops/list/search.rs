use super::FileOpsTool;
use crate::tools::grep_file::GrepSearchInput;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;
use walkdir::WalkDir;

impl FileOpsTool {
    /// Execute recursive file search
    pub(crate) async fn execute_recursive_search(&self, input: &ListInput) -> Result<Value> {
        // Allow recursive listing without pattern by defaulting to "*" (match all)
        static DEFAULT_PATTERN: &str = "*";
        let pattern = input.name_pattern.as_deref().unwrap_or(DEFAULT_PATTERN);
        let pattern_lower = pattern.to_lowercase();
        let search_path = self.workspace_root.join(&input.path);

        // Check if path exists before walking
        if !search_path.exists() {
            return Err(anyhow!(
                "Path '{}' does not exist. Workspace root: {}",
                input.path,
                self.workspace_root.display()
            ));
        }

        // Pre-allocate with max_items capacity to avoid reallocations - AGENTS.md max 5 items
        let mut items = Vec::with_capacity(input.max_items.min(5));
        let mut count = 0;
        let mut total_found = 0;

        for entry in WalkDir::new(&search_path).max_depth(10) {
            if count >= input.max_items {
                total_found += 1;
                continue; // Keep counting but don't add more items
            }

            let entry = entry.map_err(|e| anyhow!("Walk error: {}", e))?;
            let path = entry.path();

            if self.should_exclude(path).await {
                continue;
            }

            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !input.include_hidden && name.starts_with('.') {
                continue;
            }

            // Pattern matching - handle "*" as wildcard for all files
            let matches = if pattern == "*" {
                true // Match all files when pattern is "*"
            } else if input.case_sensitive.unwrap_or(true) {
                name.contains(pattern)
            } else {
                name.to_lowercase().contains(&pattern_lower)
            };

            if matches {
                // Extension filtering
                if let Some(ref extensions) = input.file_extensions {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if !extensions.iter().any(|e| e == ext) {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                let is_dir = entry.file_type().is_dir();
                items.push(json!({
                    "name": name,
                    "path": self.relative_path_json(path),
                    "type": if is_dir { "directory" } else { "file" },
                    "depth": entry.depth()
                }));
                count += 1;
            }
            total_found += 1;
        }

        // Add overflow indication if we found more items than max_items
        let mut result = self.paginate_and_format(items, count, input, "recursive", Some(pattern));
        if total_found > input.max_items
            && let Some(obj) = result.as_object_mut()
        {
            obj.insert(
                "overflow".to_string(),
                json!(format!("[+{} more items]", total_found - input.max_items)),
            );
        }
        Ok(result)
    }

    /// Execute find by exact name
    pub(crate) async fn execute_find_by_name(&self, input: &ListInput) -> Result<Value> {
        let file_name = input
            .name_pattern
            .as_ref()
            .ok_or_else(|| anyhow!("Error: Invalid 'list_files' arguments. When mode='find_name', must provide name_pattern (string). Example: {{\"path\": \".\", \"mode\": \"find_name\", \"name_pattern\": \"Cargo.toml\"}}"))?;
        let search_path = self.workspace_root.join(&input.path);

        for entry in WalkDir::new(&search_path).max_depth(10) {
            let entry = entry.map_err(|e| anyhow!("Walk error: {}", e))?;
            let path = entry.path();

            if self.should_exclude(path).await {
                continue;
            }

            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let matches = if input.case_sensitive.unwrap_or(true) {
                name == file_name.as_str()
            } else {
                name.to_lowercase() == file_name.to_lowercase()
            };

            if matches {
                let is_dir = entry.file_type().is_dir();
                return Ok(json!({
                    "success": true,
                    "found": true,
                    "name": name,
                    "path": self.relative_path_json(path),
                    "type": if is_dir { "directory" } else { "file" },
                    "mode": "find_name"
                }));
            }
        }

        Ok(json!({
            "success": true,
            "found": false,
            "mode": "find_name",
            "searched_for": file_name,
            "message": "Not found. Consider using mode='recursive' if searching in subdirectories."
        }))
    }

    /// Execute find by content pattern
    pub(crate) async fn execute_find_by_content(&self, input: &ListInput) -> Result<Value> {
        let content_pattern = input
            .content_pattern
            .as_ref()
            .ok_or_else(|| anyhow!("Error: Invalid 'list_files' arguments. When mode='find_content', must provide content_pattern (string). Example: {{\"path\": \"src\", \"mode\": \"find_content\", \"content_pattern\": \"fn main\"}}"))?;

        let search_root = self.workspace_root.join(&input.path);
        if self.should_exclude(&search_root).await {
            return Err(anyhow!(
                "Path '{}' is excluded by .vtcodegitignore",
                input.path
            ));
        }

        let search_input = GrepSearchInput {
            pattern: content_pattern.clone(),
            path: search_root.to_string_lossy().into_owned(),
            case_sensitive: input.case_sensitive,
            literal: Some(false),
            glob_pattern: None,
            context_lines: Some(0),
            include_hidden: Some(input.include_hidden),
            max_results: Some(input.max_items),
            respect_ignore_files: Some(true),
            max_file_size: None,
            search_hidden: Some(false),
            search_binary: Some(false),
            files_with_matches: Some(false),
            type_pattern: None,
            invert_match: Some(false),
            word_boundaries: Some(false),
            line_number: Some(true),
            column: Some(false),
            only_matching: Some(false),
            trim: Some(false),
            max_result_bytes: None,
            timeout: None,
            extra_ignore_globs: None,
        };

        let result = self
            .grep_manager
            .perform_search(search_input)
            .await
            .context("grep_file search failed for find_content")?;

        let mut seen_paths = std::collections::HashSet::with_capacity(result.matches.len());
        let mut items = Vec::with_capacity(result.matches.len());

        for entry in result.matches {
            let data = entry.get("data").and_then(|d| d.as_object());
            let file_text = data
                .and_then(|d| d.get("path"))
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str());

            let file_text = match file_text {
                Some(value) => value,
                None => continue,
            };

            if !seen_paths.insert(file_text.to_string()) {
                continue;
            }

            let file_path = PathBuf::from(file_text);
            let absolute_path = if file_path.is_absolute() {
                file_path
            } else {
                self.workspace_root.join(&file_path)
            };

            if self.should_exclude(&absolute_path).await {
                continue;
            }

            if tokio::fs::try_exists(&absolute_path).await.unwrap_or(false) {
                items.push(json!({
                    "name": absolute_path.file_name().unwrap_or_default().to_string_lossy(),
                    "path": absolute_path
                        .strip_prefix(&self.workspace_root)
                        .unwrap_or(&absolute_path)
                        .to_string_lossy(),
                    "type": "file",
                    "pattern_found": true
                }));
            }
        }

        let total_count = items.len();
        Ok(self.paginate_and_format(
            items,
            total_count,
            input,
            "find_content",
            Some(content_pattern),
        ))
    }
}
