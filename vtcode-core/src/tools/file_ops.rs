//! File operation tools with composable functionality

use super::traits::{CacheableTool, FileTool, ModeTool, Tool};
use super::types::*;
use crate::config::constants::diff;
use crate::tools::grep_file::{GrepSearchInput, GrepSearchManager};
use crate::utils::image_processing::read_image_file;
use crate::utils::vtcodegitignore::should_exclude_file;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::io::AsyncSeekExt;
use tracing::{info, warn};
use walkdir::WalkDir;

/// File operations tool with multiple modes
#[derive(Clone)]
pub struct FileOpsTool {
    workspace_root: PathBuf,
    canonical_workspace_root: PathBuf,
    grep_manager: Arc<GrepSearchManager>,
}

impl FileOpsTool {
    pub fn new(workspace_root: PathBuf, grep_search: Arc<GrepSearchManager>) -> Self {
        // grep_file manager is unused; keep param to avoid broad call-site churn
        let canonical_workspace_root =
            std::fs::canonicalize(&workspace_root).unwrap_or_else(|error| {
                warn!(
                    path = %workspace_root.display(),
                    %error,
                    "Failed to canonicalize workspace root; falling back to provided path"
                );
                workspace_root.clone()
            });

        Self {
            workspace_root,
            canonical_workspace_root,
            grep_manager: grep_search,
        }
    }

    /// Get relative path from workspace root, avoiding allocation when possible
    #[inline]
    fn relative_path<'a>(&self, path: &'a Path) -> Cow<'a, str> {
        path.strip_prefix(&self.workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
    }

    /// Execute basic directory listing
    async fn execute_basic_list(&self, input: &ListInput) -> Result<Value> {
        let base = self.workspace_root.join(&input.path);

        // Check if path exists before proceeding
        if !base.exists() {
            return Err(anyhow!(
                "Path '{}' does not exist. Workspace root: {}",
                input.path,
                self.workspace_root.display()
            ));
        }

        if self.should_exclude(&base).await {
            return Err(anyhow!(
                "Path '{}' is excluded by .vtcodegitignore",
                input.path
            ));
        }

        // Pre-allocate with reasonable estimate for directory entries
        // Most directories have 10-50 items, so start with 32 to avoid reallocations
        let mut all_items = Vec::with_capacity(32);
        if base.is_file() {
            let file_name = base
                .file_name()
                .ok_or_else(|| anyhow!("Invalid file name for path: {}", input.path))?;
            all_items.push(json!({
                "name": file_name.to_string_lossy(),
                "path": input.path,
                "type": "file"
            }));
        } else if base.is_dir() {
            let mut entries = tokio::fs::read_dir(&base).await.with_context(|| {
                format!(
                    "Failed to read directory: {}. Workspace root: {}",
                    input.path,
                    self.workspace_root.display()
                )
            })?;
            while let Some(entry) = entries
                .next_entry()
                .await
                .with_context(|| format!("Failed to read directory entry in: {}", input.path))?
            {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();

                if !input.include_hidden && name.starts_with('.') {
                    continue;
                }
                if self.should_exclude(&path).await {
                    continue;
                }

                let is_dir = entry
                    .file_type()
                    .await
                    .with_context(|| format!("Failed to read file type for: {}", path.display()))?
                    .is_dir();

                let relative_path = self.relative_path(&path);

                all_items.push(json!({
                    "name": name,
                    "path": relative_path,
                    "type": if is_dir { "directory" } else { "file" }
                }));
            }
        } else {
            warn!(
                path = %input.path,
                exists = base.exists(),
                is_file = base.is_file(),
                is_dir = base.is_dir(),
                "Path does not exist or is neither file nor directory"
            );
            return Err(anyhow!("Path '{}' does not exist", input.path));
        }

        // Apply max_items cap first for token efficiency - AGENTS.md requires max 5 items
        let capped_total = all_items.len().min(input.max_items);
        let (page, per_page) = (
            input.page.unwrap_or(1).max(1),
            input.per_page.unwrap_or(5).max(1), // Default to 5 items per page for context optimization
        );
        let start = (page - 1).saturating_mul(per_page);
        let end = (start + per_page).min(capped_total);
        let has_more = end < capped_total;
        let has_overflow = all_items.len() > input.max_items;

        // Log paging operation details
        info!(
            path = %input.path,
            total_items = all_items.len(),
            capped_total = capped_total,
            page = page,
            per_page = per_page,
            start_index = start,
            end_index = end,
            has_more = has_more,
            "Executing paginated file listing"
        );

        // Validate paging parameters
        if page > 1 && start >= capped_total {
            warn!(
                path = %input.path,
                page = page,
                per_page = per_page,
                total_items = capped_total,
                "Requested page exceeds available data"
            );
        }

        let mut page_items = if start < end {
            all_items[start..end].to_vec()
        } else {
            warn!(
                path = %input.path,
                page = page,
                per_page = per_page,
                start_index = start,
                end_index = end,
                "Empty page result - no items in requested range"
            );
            vec![]
        };

        // Respect response_format
        let concise = input
            .response_format
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("concise"))
            .unwrap_or(true);
        if concise {
            for obj in page_items.iter_mut() {
                if let Some(map) = obj.as_object_mut() {
                    map.remove("modified");
                }
            }
        }

        // Implement AGENTS.md pattern for context optimization: show summary with sample
        let guidance = if has_overflow {
            // Show overflow indication when we have more items than max_items
            Some(format!(
                "[+{} more items]",
                all_items.len() - input.max_items
            ))
        } else if all_items.len() > 50 && page == 1 {
            // For large directories on first page, show summary pattern
            let file_count = page_items
                .iter()
                .filter(|item| {
                    item.as_object()
                        .and_then(|obj| obj.get("type"))
                        .and_then(|t| t.as_str())
                        == Some("file")
                })
                .count();
            let dir_count = page_items
                .iter()
                .filter(|item| {
                    item.as_object()
                        .and_then(|obj| obj.get("type"))
                        .and_then(|t| t.as_str())
                        == Some("directory")
                })
                .count();

            let mut sample_names = page_items
                .iter()
                .take(5)
                .filter_map(|item| {
                    item.as_object()
                        .and_then(|obj| obj.get("name"))
                        .and_then(|n| n.as_str())
                })
                .collect::<Vec<_>>();

            if sample_names.len() > 3 {
                sample_names.truncate(3);
                sample_names.push("...");
            }

            let summary = if file_count > 0 && dir_count > 0 {
                format!(
                    "{} files and {} directories (showing first {}: {})",
                    file_count,
                    dir_count,
                    sample_names.len(),
                    sample_names.join(", ")
                )
            } else if file_count > 0 {
                format!(
                    "{} files (showing first {}: {})",
                    file_count,
                    sample_names.len(),
                    sample_names.join(", ")
                )
            } else if dir_count > 0 {
                format!(
                    "{} directories (showing first {}: {})",
                    dir_count,
                    sample_names.len(),
                    sample_names.join(", ")
                )
            } else {
                format!("Empty directory")
            };

            Some(format!(
                "{} [+{} more items]",
                summary,
                all_items.len() - sample_names.len()
            ))
        } else if has_more || capped_total < all_items.len() || all_items.len() > 20 {
            Some(format!(
                "Showing {} of {} items (page {}, per_page {}). Use 'page' and 'per_page' to page through results.",
                page_items.len(),
                capped_total,
                page,
                per_page
            ))
        } else {
            None
        };

        let mut out = json!({
            "success": true,
            "items": page_items,
            "count": page_items.len(),
            "total": capped_total,
            "page": page,
            "per_page": per_page,
            "has_more": has_more,
            "mode": "list",
            "response_format": if concise { "concise" } else { "detailed" }
        });

        if let Some(msg) = guidance {
            out["message"] = json!(msg);
        }
        Ok(out)
    }

    /// Execute recursive file search
    async fn execute_recursive_search(&self, input: &ListInput) -> Result<Value> {
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
                    "path": path.strip_prefix(&self.workspace_root).unwrap_or(path).to_string_lossy(),
                    "type": if is_dir { "directory" } else { "file" },
                    "depth": entry.depth()
                }));
                count += 1;
            }
            total_found += 1;
        }

        // Add overflow indication if we found more items than max_items
        let mut result = self.paginate_and_format(items, count, input, "recursive", Some(pattern));
        if total_found > input.max_items {
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "overflow".to_string(),
                    json!(format!("[+{} more items]", total_found - input.max_items)),
                );
            }
        }
        Ok(result)
    }

    /// Execute find by exact name
    async fn execute_find_by_name(&self, input: &ListInput) -> Result<Value> {
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
                    "path": path.strip_prefix(&self.workspace_root).unwrap_or(path).to_string_lossy(),
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

    /// Execute tree view of directory structure
    async fn execute_tree_view(&self, input: &ListInput) -> Result<Value> {
        use std::collections::HashMap;
        use tokio::fs;

        let search_path = self.workspace_root.join(&input.path);

        if self.should_exclude(&search_path).await {
            return Err(anyhow!(
                "Path '{}' is excluded by .vtcodegitignore",
                input.path
            ));
        }

        let mut dir_contents: HashMap<String, Vec<(String, String)>> = HashMap::new(); // path -> [(name, type)]

        // Walk the directory structure up to max_depth
        for entry in walkdir::WalkDir::new(&search_path)
            .max_depth(10)
            .follow_links(false)
        {
            let entry = entry.map_err(|e| anyhow!("Walk error: {}", e))?;
            let path = entry.path();

            if self.should_exclude(path).await {
                continue;
            }

            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !input.include_hidden && name.starts_with('.') {
                continue;
            }

            if path.is_dir() {
                let mut children = Vec::with_capacity(16); // Pre-allocate for typical directory size
                if let Ok(entries) = fs::read_dir(path).await {
                    let mut entries_list = Vec::with_capacity(32); // Pre-allocate for directory entries
                    let mut entry = entries;
                    while let Ok(Some(file_entry)) = entry.next_entry().await {
                        let entry_name = file_entry.file_name().to_string_lossy().into_owned();
                        if !input.include_hidden && entry_name.starts_with('.') {
                            continue;
                        }
                        if self.should_exclude(&file_entry.path()).await {
                            continue;
                        }
                        let is_dir = file_entry
                            .file_type()
                            .await
                            .map(|ft| ft.is_dir())
                            .unwrap_or(false);
                        entries_list.push((
                            entry_name,
                            if is_dir { "directory" } else { "file" }.to_string(),
                        ));
                    }
                    children = entries_list;
                }

                let relative_path = path
                    .strip_prefix(&self.workspace_root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                dir_contents.insert(relative_path, children);
            }
        }

        // Build tree structure
        let tree_structure = self
            .build_tree_structure(&search_path, &dir_contents, input.include_hidden)
            .await;

        Ok(json!({
            "success": true,
            "tree_structure": tree_structure,
            "path": input.path,
            "mode": "tree",
            "include_hidden": input.include_hidden
        }))
    }

    /// Helper function to build tree structure recursively
    async fn build_tree_structure(
        &self,
        base_path: &std::path::Path,
        dir_contents: &std::collections::HashMap<String, Vec<(String, String)>>,
        include_hidden: bool,
    ) -> Value {
        let relative_path = base_path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(base_path)
            .to_string_lossy()
            .to_string();

        let mut items = Vec::new();

        if let Some(contents) = dir_contents.get(&relative_path) {
            for (name, entry_type) in contents {
                if !include_hidden && name.starts_with('.') {
                    continue;
                }

                let item = if entry_type == "directory" {
                    // Try to get children for this subdirectory
                    let sub_path = base_path.join(name);
                    let sub_relative_path = sub_path
                        .strip_prefix(&self.workspace_root)
                        .unwrap_or(&sub_path)
                        .to_string_lossy()
                        .to_string();

                    let sub_children =
                        if let Some(sub_contents) = dir_contents.get(&sub_relative_path) {
                            let mut sub_items = Vec::new();
                            for (sub_name, sub_type) in sub_contents {
                                if !include_hidden && sub_name.starts_with('.') {
                                    continue;
                                }
                                sub_items.push(json!({
                                    "name": sub_name,
                                    "type": sub_type
                                }));
                            }
                            sub_items
                        } else {
                            Vec::new()
                        };

                    json!({
                        "name": name,
                        "type": entry_type,
                        "children": sub_children,
                        "path": sub_path.to_string_lossy()
                    })
                } else {
                    json!({
                        "name": name,
                        "type": entry_type,
                        "path": base_path.join(name).to_string_lossy()
                    })
                };

                items.push(item);
            }
        }

        json!(items)
    }

    /// Execute find by content pattern
    async fn execute_find_by_content(&self, input: &ListInput) -> Result<Value> {
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
        };

        let result = self
            .grep_manager
            .perform_search(search_input)
            .await
            .context("grep_file search failed for find_content")?;

        let mut seen_paths = std::collections::HashSet::new();
        let mut items = Vec::new();

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

    async fn execute_largest_files(&self, input: &ListInput) -> Result<Value> {
        let search_root = self.workspace_root.join(&input.path);

        if !search_root.exists() {
            return Err(anyhow!("Path '{}' does not exist", input.path));
        }

        if self.should_exclude(&search_root).await {
            return Err(anyhow!(
                "Path '{}' is excluded by .vtcodegitignore",
                input.path
            ));
        }

        let normalize_extension = |value: &str| value.trim_start_matches('.').to_lowercase();
        let extension_filter: Option<HashSet<String>> =
            input.file_extensions.as_ref().map(|exts| {
                exts.iter()
                    .map(|ext| normalize_extension(ext))
                    .collect::<HashSet<_>>()
            });

        let path_has_hidden = |path: &Path| {
            path.components().any(|component| {
                let value = component.as_os_str().to_string_lossy();
                value.starts_with('.') && value != "." && value != ".."
            })
        };

        let mut entries = Vec::new();
        for entry in WalkDir::new(&search_root).into_iter() {
            let entry = entry.map_err(|e| anyhow!("Walk error: {}", e))?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if self.should_exclude(path).await {
                continue;
            }

            if !input.include_hidden
                && path_has_hidden(path.strip_prefix(&self.workspace_root).unwrap_or(path))
            {
                continue;
            }

            if let Some(ref filters) = extension_filter {
                let extension = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(normalize_extension);

                match extension {
                    Some(ext) if filters.contains(&ext) => {}
                    _ => continue,
                }
            }

            let metadata = entry
                .metadata()
                .map_err(|e| anyhow!("Metadata error: {}", e))?;
            let size_bytes = metadata.len();
            if size_bytes == 0 {
                continue;
            }

            let relative_path = path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(path)
                .to_path_buf();
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            entries.push((size_bytes, relative_path, modified));
        }

        if entries.is_empty() {
            return Ok(json!({
                "success": true,
                "items": [],
                "count": 0,
                "total": 0,
                "page": 1,
                "per_page": input.per_page.unwrap_or(50),
                "has_more": false,
                "mode": "largest",
                "message": "No matching files found for the largest-files scan."
            }));
        }

        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        // AGENTS.md requires max 5 items for context optimization
        let effective_max = input.max_items.min(5);
        let selected_total = entries.len().min(effective_max);
        let has_overflow = entries.len() > effective_max;
        let total_entries = entries.len();

        let mut ranked = Vec::with_capacity(selected_total);
        for (idx, (size, rel_path, modified)) in
            entries.into_iter().take(selected_total).enumerate()
        {
            let name = rel_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| rel_path.display().to_string());
            ranked.push(json!({
                "rank": idx + 1,
                "name": name,
                "path": rel_path.to_string_lossy(),
                "size": size,
                "modified": modified
            }));
        }

        let mut output = self.paginate_and_format(ranked, selected_total, input, "largest", None);
        output["sorted_by"] = json!("size_desc");

        // Add overflow indication if we have more items than max_items
        if has_overflow {
            if let Some(obj) = output.as_object_mut() {
                obj.insert(
                    "overflow".to_string(),
                    json!(format!("[+{} more items]", total_entries - effective_max)),
                );
            }
        }

        let note = format!(
            "Results sorted by file size (descending). Showing top {} file(s).",
            output
                .get("count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0)
        );
        output
            .as_object_mut()
            .expect("largest_files output must be an object")
            .entry("message")
            .and_modify(|value| {
                if let Some(existing) = value.as_str() {
                    *value = json!(format!("{existing} {note}"));
                } else {
                    *value = json!(note.clone());
                }
            })
            .or_insert_with(|| json!(note));

        Ok(output)
    }

    /// Read file with intelligent path resolution, paging, and offset functionality
    pub async fn read_file(&self, args: Value) -> Result<Value> {
        let input: Input = serde_json::from_value(args)
            .context("Error: Invalid 'read_file' arguments. Expected JSON object with: path (required, string). Optional: max_bytes, offset_bytes, page_size_bytes, offset_lines, page_size_lines. Example: {\"path\": \"src/main.rs\", \"offset_lines\": 100, \"page_size_lines\": 50}")?;

        // Try to resolve the file path
        let potential_paths = self.resolve_file_path(&input.path)?;

        for candidate_path in &potential_paths {
            if !tokio::fs::try_exists(candidate_path).await? {
                continue;
            }

            let canonical = self
                .normalize_and_validate_candidate(candidate_path, &input.path)
                .await?;

            if self.should_exclude(&canonical).await {
                continue;
            }

            if !tokio::fs::metadata(&canonical).await?.is_file() {
                continue;
            }

            // Check if paging/offset is requested
            let use_paging = input.offset_bytes.is_some()
                || input.page_size_bytes.is_some()
                || input.offset_lines.is_some()
                || input.page_size_lines.is_some();

            let (content, metadata, truncated) = if use_paging {
                self.read_file_paged(&canonical, &input).await?
            } else {
                // Use existing logic for backward compatibility
                self.read_file_legacy(&canonical, &input).await?
            };

            let mut result = json!({
                "success": true,
                "content": content,
                "path": self.workspace_relative_display(&canonical),
                "metadata": metadata
            });

            if let Some(encoding) = result
                .get("metadata")
                .and_then(|meta| meta.get("encoding"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                result["encoding"] = json!(encoding);
            }

            if let Some(content_kind) = result
                .get("metadata")
                .and_then(|meta| meta.get("content_kind"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                result["content_kind"] = json!(content_kind);
                if matches!(content_kind.as_str(), "binary" | "image") {
                    result["binary"] = json!(true);
                }
            }

            if let Some(mime_type) = result
                .get("metadata")
                .and_then(|meta| meta.get("mime_type"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                result["mime_type"] = json!(mime_type);
            }

            // Add paging information if applicable
            if use_paging {
                if let Some(offset_bytes) = input.offset_bytes {
                    result["offset_bytes"] = json!(offset_bytes);
                }
                if let Some(page_size_bytes) = input.page_size_bytes {
                    result["page_size_bytes"] = json!(page_size_bytes);
                }
                if let Some(offset_lines) = input.offset_lines {
                    result["offset_lines"] = json!(offset_lines);
                }
                if let Some(page_size_lines) = input.page_size_lines {
                    result["page_size_lines"] = json!(page_size_lines);
                }
                if truncated {
                    result["truncated"] = json!(true);
                    result["truncation_reason"] = json!("reached_end_of_file");
                }
            }

            return Ok(result);
        }

        Err(anyhow!(
            "Error: File not found: {}. Tried paths: {}. Suggestions: 1) Check the file path and case sensitivity, 2) Use list_files to explore the directory structure, 3) Try case-insensitive search with just the filename. Example: {{\"path\": \"src/main.rs\"}}",
            input.path,
            potential_paths
                .iter()
                .map(|p| self.workspace_relative_display(p))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    /// Create a brand-new file, returning an error if the target already exists.
    pub async fn create_file(&self, args: Value) -> Result<Value> {
        let input: CreateInput = serde_json::from_value(args).context(
            "Error: Invalid 'create_file' arguments. Expected JSON object with: path (required, string), content (required, string). Example: {\"path\": \"src/lib.rs\", \"content\": \"fn main() {}\\n\"}",
        )?;

        let CreateInput {
            path,
            content,
            encoding,
        } = input;

        let file_path = self.normalize_and_validate_user_path(&path).await?;

        if self.should_exclude(&file_path).await {
            return Err(anyhow!(
                "Error: Path '{}' is excluded by .vtcodegitignore",
                path
            ));
        }

        if tokio::fs::try_exists(&file_path).await? {
            return Err(anyhow!(
                "Error: File '{}' already exists. Use write_file with mode='overwrite' to replace it.",
                path
            ));
        }

        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut payload = json!({
            "path": path,
            "content": content,
            "mode": "overwrite"
        });

        if let Some(encoding) = encoding {
            payload["encoding"] = Value::String(encoding);
        }

        let mut result = self.write_file(payload).await?;

        if let Some(map) = result.as_object_mut() {
            map.insert("created".to_string(), Value::Bool(true));
        }

        Ok(result)
    }

    /// Delete a file or directory (with recursive flag).
    pub async fn delete_file(&self, args: Value) -> Result<Value> {
        let input: DeleteInput = serde_json::from_value(args).context(
            "Error: Invalid 'delete_file' arguments. Expected JSON object with: path (required, string). Optional: recursive (bool), force (bool). Example: {\"path\": \"src/lib.rs\"}",
        )?;

        let DeleteInput {
            path,
            recursive,
            force,
        } = input;

        let target_path = self.workspace_root.join(&path);

        let exists = tokio::fs::try_exists(&target_path)
            .await
            .with_context(|| format!("Failed to check if '{}' exists", path))?;

        if !exists {
            if force {
                return Ok(json!({
                    "success": true,
                    "deleted": false,
                    "skipped": true,
                    "reason": "not_found",
                    "path": path,
                }));
            }

            return Err(anyhow!(
                "Error: Path '{}' does not exist. Provide force=true to ignore missing files.",
                path
            ));
        }

        let canonical = tokio::fs::canonicalize(&target_path)
            .await
            .with_context(|| format!("Failed to resolve canonical path for '{}'", path))?;

        if !canonical.starts_with(self.canonical_workspace_root()) {
            return Err(anyhow!(
                "Error: Path '{}' resolves outside the workspace and cannot be deleted.",
                path
            ));
        }

        if self.should_exclude(&canonical).await {
            return Err(anyhow!(
                "Error: Path '{}' is excluded by .vtcodegitignore and cannot be deleted.",
                path
            ));
        }

        let metadata = tokio::fs::metadata(&canonical)
            .await
            .with_context(|| format!("Failed to read metadata for '{}'", path))?;

        let deleted_kind = if metadata.is_dir() {
            if !recursive {
                return Err(anyhow!(
                    "Error: '{}' is a directory. Pass recursive=true to remove directories.",
                    path
                ));
            }

            tokio::fs::remove_dir_all(&canonical)
                .await
                .with_context(|| format!("Failed to remove directory '{}'", path))?;
            "directory"
        } else {
            tokio::fs::remove_file(&canonical)
                .await
                .with_context(|| format!("Failed to remove file '{}'", path))?;
            "file"
        };

        Ok(json!({
            "success": true,
            "deleted": true,
            "path": self.workspace_relative_display(&canonical),
            "kind": deleted_kind,
        }))
    }

    /// Write file with various modes and chunking support for large content
    pub async fn write_file(&self, args: Value) -> Result<Value> {
        let input: WriteInput = serde_json::from_value(args)
            .context("Error: Invalid 'write_file' arguments. Expected JSON object with: path (required, string), content (required, string). Optional: mode (string, one of: overwrite, append, skip_if_exists). Example: {\"path\": \"README.md\", \"content\": \"Hello\", \"mode\": \"overwrite\"}")?;
        let file_path = self.normalize_and_validate_user_path(&input.path).await?;

        if self.should_exclude(&file_path).await {
            return Err(anyhow!(
                "Error: Path '{}' is excluded by .vtcodegitignore",
                input.path
            ));
        }

        // Check if content needs chunking
        let content_size = input.content.len();
        let should_chunk =
            content_size > crate::config::constants::chunking::MAX_WRITE_CONTENT_SIZE;

        if should_chunk {
            return self.write_file_chunked(&file_path, &input).await;
        }

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file_exists = tokio::fs::try_exists(&file_path).await?;

        if input.mode.as_str() == "skip_if_exists" && file_exists {
            return Ok(json!({
                "success": true,
                "skipped": true,
                "reason": "File already exists"
            }));
        }

        let mut existing_content: Option<String> = None;
        let mut diff_preview: Option<Value> = None;

        if file_exists {
            match tokio::fs::read_to_string(&file_path).await {
                Ok(content) => existing_content = Some(content),
                Err(error) => {
                    diff_preview = Some(diff_preview_error_skip(
                        "failed_to_read_existing_content",
                        Some(&format!("{:?}", error.kind())),
                    ));
                }
            }
        }

        match input.mode.as_str() {
            "overwrite" => {
                tokio::fs::write(&file_path, &input.content).await?;
            }
            "append" => {
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path)
                    .await?;
                file.write_all(input.content.as_bytes()).await?;
            }
            "skip_if_exists" => {
                tokio::fs::write(&file_path, &input.content).await?;
            }
            _ => {
                return Err(anyhow!(
                    "Error: Unsupported write mode '{}'. Allowed: overwrite, append, skip_if_exists.",
                    input.mode
                ));
            }
        }

        // Log write operation
        self.log_write_operation(&file_path, content_size, false)
            .await?;

        if diff_preview.is_none() {
            let existing_snapshot = existing_content.as_deref();
            let total_len = if input.mode.as_str() == "append" {
                existing_snapshot
                    .map(|content| content.len())
                    .unwrap_or_default()
                    + input.content.len()
            } else {
                input.content.len()
            };

            if total_len > diff::MAX_PREVIEW_BYTES
                || existing_snapshot
                    .map(|content| content.len() > diff::MAX_PREVIEW_BYTES)
                    .unwrap_or(false)
            {
                diff_preview = Some(diff_preview_size_skip());
            } else {
                let final_snapshot: Cow<'_, str> = if input.mode.as_str() == "append" {
                    if let Some(existing) = existing_snapshot {
                        Cow::Owned(format!("{existing}{}", input.content))
                    } else {
                        Cow::Borrowed(input.content.as_str())
                    }
                } else {
                    Cow::Borrowed(input.content.as_str())
                };

                diff_preview = Some(build_diff_preview(
                    &input.path,
                    existing_snapshot,
                    final_snapshot.as_ref(),
                ));
            }
        }

        let mut response = json!({
            "success": true,
            "path": self.workspace_relative_display(&file_path),
            "mode": input.mode,
            "bytes_written": input.content.len()
        });

        if let Some(preview) = diff_preview {
            if let Some(object) = response.as_object_mut() {
                object.insert("diff_preview".to_string(), preview);
            }
        }

        Ok(response)
    }

    /// Write large file in chunks for atomicity and memory efficiency
    async fn write_file_chunked(&self, file_path: &Path, input: &WriteInput) -> Result<Value> {
        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content_bytes = input.content.as_bytes();
        let chunk_size = crate::config::constants::chunking::WRITE_CHUNK_SIZE;
        let total_size = content_bytes.len();

        match input.mode.as_str() {
            "overwrite" => {
                // Write in chunks for large files
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(file_path)
                    .await?;

                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            "append" => {
                // Append in chunks
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file_path)
                    .await?;

                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            "skip_if_exists" => {
                if file_path.exists() {
                    return Ok(json!({
                        "success": true,
                        "skipped": true,
                        "reason": "File already exists"
                    }));
                }
                // Write in chunks for new file
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::File::create(file_path).await?;
                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            _ => {
                return Err(anyhow!(
                    "Error: Unsupported write mode '{}'. Allowed: overwrite, append, skip_if_exists.",
                    input.mode
                ));
            }
        }

        // Log chunked write operation
        self.log_write_operation(file_path, total_size, true)
            .await?;

        Ok(json!({
            "success": true,
            "path": self.workspace_relative_display(file_path),
            "mode": input.mode,
            "bytes_written": total_size,
            "chunked": true,
            "chunk_size": chunk_size,
            "chunks_written": total_size.div_ceil(chunk_size),
            "diff_preview": diff_preview_size_skip()
        }))
    }

    /// Log write operations for debugging
    async fn log_write_operation(
        &self,
        file_path: &Path,
        bytes_written: usize,
        chunked: bool,
    ) -> Result<()> {
        let log_entry = json!({
            "operation": if chunked { "write_file_chunked" } else { "write_file" },
            "file_path": file_path.to_string_lossy(),
            "bytes_written": bytes_written,
            "chunked": chunked,
            "chunk_size": if chunked { Some(crate::config::constants::chunking::WRITE_CHUNK_SIZE) } else { None },
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        info!(
            "File write operation: {}",
            serde_json::to_string(&log_entry)?
        );
        Ok(())
    }

    fn canonical_workspace_root(&self) -> &PathBuf {
        &self.canonical_workspace_root
    }

    fn workspace_relative_display(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.workspace_root) {
            relative.to_string_lossy().into_owned()
        } else if let Ok(relative) = path.strip_prefix(self.canonical_workspace_root()) {
            relative.to_string_lossy().into_owned()
        } else {
            path.to_string_lossy().into_owned()
        }
    }

    fn absolute_candidate(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        }
    }

    async fn normalize_and_validate_user_path(&self, path: &str) -> Result<PathBuf> {
        self.normalize_and_validate_candidate(Path::new(path), path)
            .await
    }

    async fn normalize_and_validate_candidate(
        &self,
        path: &Path,
        original_display: &str,
    ) -> Result<PathBuf> {
        let absolute = self.absolute_candidate(path);
        let normalized = normalize_path(&absolute);
        let normalized_root = normalize_path(&self.workspace_root);

        if !normalized.starts_with(&normalized_root) {
            return Err(anyhow!(
                "Error: Path '{}' resolves outside the workspace.",
                original_display
            ));
        }

        let canonical = self.canonicalize_allow_missing(&normalized).await?;
        if !canonical.starts_with(self.canonical_workspace_root()) {
            return Err(anyhow!(
                "Error: Path '{}' resolves outside the workspace.",
                original_display
            ));
        }

        Ok(canonical)
    }

    async fn canonicalize_allow_missing(&self, normalized: &Path) -> Result<PathBuf> {
        if tokio::fs::try_exists(normalized).await? {
            return tokio::fs::canonicalize(normalized).await.with_context(|| {
                format!(
                    "Failed to resolve canonical path for '{}'.",
                    normalized.display()
                )
            });
        }

        let mut current = normalized.to_path_buf();
        while let Some(parent) = current.parent() {
            if tokio::fs::try_exists(parent).await? {
                let canonical_parent =
                    tokio::fs::canonicalize(parent).await.with_context(|| {
                        format!(
                            "Failed to resolve canonical path for '{}'.",
                            parent.display()
                        )
                    })?;
                let remainder = normalized
                    .strip_prefix(parent)
                    .unwrap_or_else(|_| Path::new(""));
                return if remainder.as_os_str().is_empty() {
                    Ok(canonical_parent)
                } else {
                    Ok(canonical_parent.join(remainder))
                };
            }
            current = parent.to_path_buf();
        }

        Ok(normalized.to_path_buf())
    }
}

fn diff_preview_size_skip() -> Value {
    json!({
        "skipped": true,
        "reason": "content_exceeds_preview_limit",
        "max_bytes": diff::MAX_PREVIEW_BYTES
    })
}

/// Create a diff preview response when inline diffs are suppressed due to too many changes
fn diff_preview_suppressed(additions: usize, deletions: usize, line_count: usize) -> Value {
    json!({
        "skipped": true,
        "suppressed": true,
        "reason": "too_many_changes",
        "message": diff::SUPPRESSION_MESSAGE,
        "summary": {
            "additions": additions,
            "deletions": deletions,
            "total_lines": line_count
        }
    })
}

fn diff_preview_error_skip(reason: &str, detail: Option<&str>) -> Value {
    match detail {
        Some(value) => json!({
            "skipped": true,
            "reason": reason,
            "detail": value
        }),
        None => json!({
            "skipped": true,
            "reason": reason
        }),
    }
}

fn build_diff_preview(path: &str, before: Option<&str>, after: &str) -> Value {
    let previous = before.unwrap_or("");
    // Use format strings directly - Cow optimization not needed for short-lived strings
    let old_header = format!("a/{path}");
    let new_header = format!("b/{path}");

    // Use git diff algorithm which provides better, unified output format
    // This can be executed via run_pty_cmd for consistency
    let diff_output = generate_git_style_diff(previous, after, &old_header, &new_header);

    if diff_output.trim().is_empty() {
        return json!({
            "content": "",
            "truncated": false,
            "omitted_line_count": 0,
            "skipped": false,
            "is_empty": true
        });
    }

    // Avoid collecting lines into Vec<String> - use line count directly
    let line_count = diff_output.lines().count();

    // Count additions and deletions for suppression check
    let additions = diff_output.lines().filter(|l| l.starts_with('+')).count();
    let deletions = diff_output.lines().filter(|l| l.starts_with('-')).count();
    let total_changes = additions + deletions;

    // Check if we should suppress the diff due to too many changes
    if total_changes > diff::MAX_SINGLE_FILE_CHANGES {
        return diff_preview_suppressed(additions, deletions, line_count);
    }

    // Only collect lines if we need to truncate
    if line_count > diff::MAX_PREVIEW_LINES {
        let lines: Vec<&str> = diff_output.lines().collect();
        let head_count = diff::HEAD_LINE_COUNT.min(lines.len());
        let tail_count = diff::TAIL_LINE_COUNT.min(lines.len().saturating_sub(head_count));
        let omitted = lines.len().saturating_sub(head_count + tail_count);

        let mut condensed = Vec::with_capacity(head_count + tail_count + 1);
        condensed.extend(lines[..head_count].iter().copied());
        if omitted > 0 {
            // Use a static format to avoid allocation in common case
            condensed.push(""); // placeholder, will be replaced
        }
        if tail_count > 0 {
            let tail_start = lines.len().saturating_sub(tail_count);
            condensed.extend(lines[tail_start..].iter().copied());
        }

        let diff_output = if omitted > 0 {
            let mut result = condensed[..head_count].join("\n");
            result.push_str(&format!("\n... {omitted} lines omitted ...\n"));
            result.push_str(&condensed[head_count + 1..].join("\n"));
            result
        } else {
            condensed.join("\n")
        };

        json!({
            "content": diff_output,
            "truncated": true,
            "omitted_line_count": omitted,
            "skipped": false
        })
    } else {
        json!({
            "content": diff_output,
            "truncated": false,
            "omitted_line_count": 0,
            "skipped": false
        })
    }
}

/// Generate unified diff using git diff --no-index command.
///
/// Uses the optimized git diff rendering system unified with run_pty_cmd:
/// - Produces standard unified diff format with file headers ("--- a/", "+++ b/")
/// - Hunk headers with line counts ("@@ -1,3 +1,3 @@")
/// - Line prefixes: '+' (additions), '-' (deletions), ' ' (context)
///
/// The rendering system in src/agent/runloop/tool_output/files.rs detects these
/// markers and applies proper ANSI styling for terminal display.
fn generate_git_style_diff(old: &str, new: &str, old_label: &str, new_label: &str) -> String {
    match git_diff(old, new, old_label, new_label) {
        Ok(output) => output,
        Err(e) => {
            warn!("git diff failed: {}, returning empty diff", e);
            String::new()
        }
    }
}

/// Execute git diff --no-index using temporary files.
///
/// This is the unified approach for diff generation across write_files and run_pty_cmd.
/// Returns standard unified diff format that matches what git shows in terminal.
fn git_diff(old: &str, new: &str, old_label: &str, new_label: &str) -> Result<String> {
    // Create temporary files for old and new content
    let mut old_file = tempfile::NamedTempFile::new()
        .with_context(|| "failed to create temp file for old content")?;
    old_file
        .write_all(old.as_bytes())
        .with_context(|| "failed to write old content to temp file")?;

    let mut new_file = tempfile::NamedTempFile::new()
        .with_context(|| "failed to create temp file for new content")?;
    new_file
        .write_all(new.as_bytes())
        .with_context(|| "failed to write new content to temp file")?;

    let old_path = old_file.path().to_path_buf();
    let new_path = new_file.path().to_path_buf();

    // Execute git diff with context radius matching config
    let output = Command::new("git")
        .args([
            "diff",
            "--no-index",
            &format!("--unified={}", diff::CONTEXT_RADIUS),
        ])
        .arg(&old_path)
        .arg(&new_path)
        .output()
        .with_context(|| "git diff command execution failed")?;

    let mut result = String::from_utf8_lossy(&output.stdout).to_string();

    // Replace temporary file paths with actual labels
    let old_str = old_path.display().to_string();
    let new_str = new_path.display().to_string();
    result = result.replace(&format!("a/{old_str}"), &format!("a/{old_label}"));
    result = result.replace(&format!("b/{new_str}"), &format!("b/{new_label}"));

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut input: ListInput = serde_json::from_value(args).context(
            "Error: Invalid 'list_files' arguments. Expected JSON object with: path (required, string). Optional: mode (string), max_items (number), page (number), per_page (number), include_hidden (bool), response_format (string). Example: {\"path\": \"src\", \"page\": 1, \"per_page\": 50, \"response_format\": \"concise\"}",
        )?;

        // Normalize path: strip /workspace prefix if present (common LLM pattern)
        if input.path.starts_with("/workspace/") {
            input.path = input.path.strip_prefix("/workspace/").unwrap().to_string();
        } else if input.path == "/workspace" {
            input.path = ".".to_string();
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

impl FileOpsTool {
    fn paginate_and_format(
        &self,
        items: Vec<Value>,
        total_count: usize,
        input: &ListInput,
        mode: &str,
        pattern: Option<&str>,
    ) -> Value {
        let (page, per_page) = (
            input.page.unwrap_or(1).max(1),
            input.per_page.unwrap_or(50).max(1),
        );
        let total_capped = total_count.min(input.max_items);
        let start = (page - 1).saturating_mul(per_page);
        let end = (start + per_page).min(total_capped);
        let has_more = end < total_capped;

        // Log pagination operation details
        info!(
            mode = %mode,
            pattern = ?pattern,
            total_items = total_count,
            capped_total = total_capped,
            page = page,
            per_page = per_page,
            start_index = start,
            end_index = end,
            has_more = has_more,
            "Executing paginated search results"
        );

        // Validate pagination parameters
        if page > 1 && start >= total_capped {
            warn!(
                mode = %mode,
                page = page,
                per_page = per_page,
                total_items = total_capped,
                "Requested page exceeds available search results"
            );
        }

        let mut page_items = if start < end {
            items[start..end].to_vec()
        } else {
            warn!(
                mode = %mode,
                page = page,
                per_page = per_page,
                start_index = start,
                end_index = end,
                "Empty page result - no search results in requested range"
            );
            vec![]
        };

        let concise = input
            .response_format
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("concise"))
            .unwrap_or(true);
        if concise {
            for obj in page_items.iter_mut() {
                if let Some(map) = obj.as_object_mut() {
                    map.remove("modified");
                }
            }
        }

        let mut out = json!({
            "success": true,
            "items": page_items,
            "count": page_items.len(),
            "total": total_capped,
            "page": page,
            "per_page": per_page,
            "has_more": has_more,
            "mode": mode,
            "response_format": if concise { "concise" } else { "detailed" }
        });
        if let Some(p) = pattern {
            out["pattern"] = json!(p);
        }
        if has_more || total_capped > 20 {
            out["message"] = json!(format!(
                "Showing {} of {} results. Use 'page' to continue.",
                out["count"].as_u64().unwrap_or(0),
                total_capped
            ));
        }
        out
    }

    /// Read file with chunking (first N + last N lines)
    async fn read_file_chunked(
        &self,
        file_path: &Path,
        input: &Input,
        applied_max_tokens: Option<usize>,
    ) -> Result<(String, bool, Option<usize>)> {
        let content = tokio::fs::read_to_string(file_path).await?;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Use custom chunk sizes or token budget if provided; otherwise use defaults
        let start_chunk = if let Some(chunk_lines) = input.chunk_lines {
            chunk_lines / 2
        } else {
            crate::config::constants::chunking::CHUNK_START_LINES
        };
        let end_chunk = if let Some(chunk_lines) = input.chunk_lines {
            chunk_lines / 2
        } else {
            crate::config::constants::chunking::CHUNK_END_LINES
        };
        if total_lines <= start_chunk + end_chunk {
            // File is small enough, return all content
            self.log_chunking_operation(file_path, false, Some(total_lines))
                .await?;
            return Ok((content, false, Some(total_lines)));
        }

        // Token-aware head+tail chunking. First determine token budget to use.
        let tokens_budget = if let Some(mt) = applied_max_tokens {
            mt
        } else if let Some(mt) = input.max_tokens {
            mt
        } else if let Some(ml) = input.max_lines {
            // Map legacy max_lines to an approximate token count
            ml.saturating_mul(crate::core::token_constants::TOKENS_PER_LINE)
        } else if let Some(chunk_lines) = input.chunk_lines {
            // Convert chunk_lines to tokens (approx)
            chunk_lines.saturating_mul(crate::core::token_constants::TOKENS_PER_LINE)
        } else {
            // Default to reading CHUNK_START_LINES + CHUNK_END_LINES lines; convert to tokens
            (crate::config::constants::chunking::CHUNK_START_LINES
                + crate::config::constants::chunking::CHUNK_END_LINES)
                .saturating_mul(crate::core::token_constants::TOKENS_PER_LINE)
        };

        // Determine if content is code (bracket density heuristic)
        let char_count = content.len();
        let bracket_count: usize = content
            .chars()
            .filter(|c| crate::core::token_constants::CODE_INDICATOR_CHARS.contains(*c))
            .count();
        let is_code =
            bracket_count > (char_count / crate::core::token_constants::CODE_DETECTION_THRESHOLD);
        let head_ratio = if is_code {
            crate::core::token_constants::CODE_HEAD_RATIO_PERCENT
        } else {
            crate::core::token_constants::LOG_HEAD_RATIO_PERCENT
        };

        let head_tokens = (tokens_budget * head_ratio) / 100;
        let tail_tokens = tokens_budget.saturating_sub(head_tokens);

        // Collect head lines until head_tokens reached
        let mut head_lines = Vec::new();
        let mut acc_tokens = 0usize;
        for line in &lines {
            if acc_tokens >= head_tokens {
                break;
            }
            let line_tokens = (line.len() as f64
                / crate::core::token_constants::TOKENS_PER_CHARACTER)
                .ceil() as usize;
            if acc_tokens + line_tokens <= head_tokens || head_lines.is_empty() {
                head_lines.push(*line);
                acc_tokens += line_tokens;
            } else {
                break;
            }
        }

        // Collect tail lines until tail_tokens reached
        let mut tail_lines = Vec::new();
        let mut acc_tail = 0usize;
        for line in lines.iter().rev() {
            if acc_tail >= tail_tokens {
                break;
            }
            let line_tokens = (line.len() as f64
                / crate::core::token_constants::TOKENS_PER_CHARACTER)
                .ceil() as usize;
            if acc_tail + line_tokens <= tail_tokens || tail_lines.is_empty() {
                tail_lines.push(*line);
                acc_tail += line_tokens;
            } else {
                break;
            }
        }
        tail_lines.reverse();

        let mut chunked_content = String::new();
        // Add head lines
        for (i, line) in head_lines.iter().enumerate() {
            if i > 0 {
                chunked_content.push('\n');
            }
            chunked_content.push_str(line);
        }
        // Determine truncated lines count and insert truncation marker
        let head_line_count = head_lines.len();
        let tail_line_count = tail_lines.len();
        let truncated_line_count = total_lines.saturating_sub(head_line_count + tail_line_count);
        if truncated_line_count > 0 {
            let _ = write!(
                chunked_content,
                "\n\n... [{} lines truncated - showing first {} and last {} lines] ...\n\n",
                truncated_line_count, head_line_count, tail_line_count
            );
        } else {
            chunked_content.push_str("\n\n");
        }
        // Add tail lines
        for (i, line) in tail_lines.iter().enumerate() {
            if i > 0 {
                chunked_content.push('\n');
            }
            chunked_content.push_str(line);
        }

        self.log_chunking_operation(file_path, true, Some(total_lines))
            .await?;

        Ok((chunked_content, true, Some(total_lines)))
    }

    /// Legacy file reading with backward compatibility for max_bytes and chunking
    async fn read_file_legacy(
        &self,
        file_path: &Path,
        input: &Input,
    ) -> Result<(String, Value, bool)> {
        // First, check if we should use chunked reading or honor a per-call token budget
        if input.chunk_lines.is_some() || input.max_lines.is_some() || input.max_tokens.is_some() {
            // Determine applied token budget (if provided) or map deprecated max_lines to tokens
            let mut applied_max_tokens: Option<usize> = None;
            if let Some(mt) = input.max_tokens {
                applied_max_tokens = Some(mt);
            } else if let Some(ml) = input.max_lines {
                // Map legacy max_lines to an approximate token count
                let est = ml.saturating_mul(crate::core::token_constants::TOKENS_PER_LINE);
                tracing::warn!(
                    "`max_lines` is deprecated; mapping {} lines -> ~{} tokens for backward compatibility",
                    ml,
                    est
                );
                applied_max_tokens = Some(est);
            }

            let (content, is_truncated, total_lines) = self
                .read_file_chunked(file_path, input, applied_max_tokens)
                .await?;

            // Create metadata object
            let metadata = if let Ok(file_metadata) = tokio::fs::metadata(file_path).await {
                json!({
                    "size_bytes": file_metadata.len(),
                    "size_lines": total_lines,
                    "is_truncated": is_truncated,
                    "applied_max_tokens": applied_max_tokens,
                    "type": "file",
                    "content_kind": "text",
                    "encoding": "utf8",
                })
            } else {
                json!({
                    "size_bytes": 0,
                    "size_lines": total_lines,
                    "is_truncated": is_truncated,
                    "applied_max_tokens": applied_max_tokens,
                    "type": "file",
                    "content_kind": "text",
                    "encoding": "utf8",
                })
            };

            return Ok((content, metadata, is_truncated));
        }

        // Detect image files and return base64 data for them immediately
        if is_image_path(file_path) {
            let image_data = read_image_file(file_path)
                .await
                .with_context(|| format!("Failed to load image file: {}", file_path.display()))?;

            let metadata = json!({
                "size_bytes": image_data.size,
                "is_truncated": false,
                "type": "file",
                "content_kind": "image",
                "encoding": "base64",
                "mime_type": image_data.mime_type,
            });

            return Ok((image_data.base64_data, metadata, false));
        }

        let file_metadata = tokio::fs::metadata(file_path).await.ok();
        let raw_bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        match String::from_utf8(raw_bytes) {
            Ok(content) => {
                let total_lines = content.lines().count();
                let (final_content, truncated) = match input.max_bytes {
                    Some(max_bytes) if content.len() > max_bytes => {
                        let safe_truncate_point = content
                            .char_indices()
                            .take_while(|(index, _)| *index < max_bytes)
                            .map(|(index, _)| index)
                            .last()
                            .unwrap_or(max_bytes);
                        (content[..safe_truncate_point].to_string(), true)
                    }
                    _ => (content, false),
                };

                let size_bytes = file_metadata
                    .as_ref()
                    .map(|meta| meta.len())
                    .unwrap_or_else(|| final_content.len() as u64);

                let metadata = json!({
                    "size_bytes": size_bytes,
                    "size_lines": total_lines,
                    "is_truncated": truncated,
                    "type": "file",
                    "content_kind": "text",
                    "encoding": "utf8",
                });

                Ok((final_content, metadata, truncated))
            }
            Err(err) => {
                let bytes = err.into_bytes();
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);

                let size_bytes = file_metadata
                    .as_ref()
                    .map(|meta| meta.len())
                    .unwrap_or(bytes.len() as u64);

                let metadata = json!({
                    "size_bytes": size_bytes,
                    "is_truncated": false,
                    "type": "file",
                    "content_kind": "binary",
                    "encoding": "base64",
                });

                Ok((base64_data, metadata, false))
            }
        }
    }

    /// Read file with paged/offset functionality for bytes and lines
    async fn read_file_paged(
        &self,
        file_path: &Path,
        input: &Input,
    ) -> Result<(String, Value, bool)> {
        // Get file metadata to verify file exists and get size
        let file_metadata = tokio::fs::metadata(file_path).await.with_context(|| {
            format!("Failed to read metadata for file: {}", file_path.display())
        })?;

        if !file_metadata.is_file() {
            return Err(anyhow!("Path is not a file: {}", file_path.display()));
        }

        let file_size = file_metadata.len();

        // Calculate the final content based on whether we're using byte or line-based paging
        let (final_content, is_truncated) =
            if input.offset_lines.is_some() || input.page_size_lines.is_some() {
                // Line-based paging
                self.read_file_by_lines(file_path, input, file_size as usize)
                    .await?
            } else {
                // Byte-based paging (default)
                self.read_file_by_bytes(file_path, input, file_size).await?
            };

        // Create metadata object
        let metadata = json!({
            "size_bytes": file_size,
            "size_lines": final_content.lines().count(),
            "is_truncated": is_truncated,
            "type": "file",
            "content_kind": "text",
            "encoding": "utf8",
        });

        Ok((final_content, metadata, is_truncated))
    }

    /// Read file content by lines with offset and page size
    async fn read_file_by_lines(
        &self,
        file_path: &Path,
        input: &Input,
        _file_size: usize,
    ) -> Result<(String, bool)> {
        // Validate the offset and page size parameters
        let offset_lines = input.offset_lines.unwrap_or(0);
        let page_size_lines = input.page_size_lines.unwrap_or(usize::MAX); // Default to read all lines from offset

        if offset_lines > usize::MAX / 2 {
            // Prevent potential overflow
            return Err(anyhow!(
                "Offset_lines parameter too large: {}",
                offset_lines
            ));
        }

        let content = tokio::fs::read_to_string(file_path)
            .await
            .with_context(|| format!("Failed to read file content: {}", file_path.display()))?;

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();

        // Handle empty file case
        if total_lines == 0 {
            return Ok((String::new(), false));
        }

        // Validate offset is not beyond the file size
        if offset_lines >= total_lines {
            if offset_lines == 0 {
                // Special case: if offset is 0 but file is empty, return empty string
                return Ok((String::new(), false));
            }
            return Ok((String::new(), false)); // Return empty if offset is beyond file size
        }

        // Calculate the end position (don't exceed file boundaries)
        let end_pos = std::cmp::min(offset_lines + page_size_lines, total_lines);
        let selected_lines = &all_lines[offset_lines..end_pos];

        let final_content = selected_lines.join("\n");
        let is_truncated = end_pos < total_lines; // indicate if we didn't read all lines

        Ok((final_content, is_truncated))
    }

    /// Read file content by bytes with offset and page size
    async fn read_file_by_bytes(
        &self,
        file_path: &Path,
        input: &Input,
        file_size: u64,
    ) -> Result<(String, bool)> {
        use tokio::io::AsyncReadExt;

        let offset_bytes = input.offset_bytes.unwrap_or(0);
        let page_size_bytes = input.page_size_bytes.unwrap_or(file_size as usize);

        // Validate offset is not beyond the file size
        if offset_bytes >= file_size {
            if offset_bytes == 0 && file_size == 0 {
                // Special case: empty file with offset 0
                return Ok((String::new(), false));
            }
            return Ok((String::new(), false)); // Return empty if offset is beyond file size
        }

        // Prevent potential overflow when calculating end position
        let page_size_u64 = page_size_bytes as u64;
        if offset_bytes > u64::MAX - page_size_u64 {
            return Err(anyhow!(
                "Offset_bytes + page_size_bytes would overflow: {} + {}",
                offset_bytes,
                page_size_bytes
            ));
        }

        // Open the file and seek to the offset
        let mut file = tokio::fs::File::open(file_path)
            .await
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

        // Calculate the end position (don't exceed file boundaries)
        let end_pos = std::cmp::min(offset_bytes + page_size_u64, file_size);
        let actual_read_size = (end_pos - offset_bytes) as usize;

        // Seek to the offset position
        file.seek(std::io::SeekFrom::Start(offset_bytes))
            .await
            .with_context(|| {
                format!(
                    "Failed to seek to offset {} in file: {}",
                    offset_bytes,
                    file_path.display()
                )
            })?;

        // Read the specified number of bytes
        let mut buffer = vec![0; actual_read_size];
        let mut bytes_read = 0;

        if actual_read_size > 0 {
            bytes_read = file.read_exact(&mut buffer).await.with_context(|| {
                format!(
                    "Failed to read {} bytes from offset {} in file: {}",
                    actual_read_size,
                    offset_bytes,
                    file_path.display()
                )
            })?;
        }

        // Convert to string, handling potential UTF-8 errors gracefully
        let content = String::from_utf8_lossy(&buffer[..bytes_read]);
        let final_content = content.to_string();
        let is_truncated = end_pos < file_size; // indicate if we didn't read the entire file

        Ok((final_content, is_truncated))
    }

    /// Log chunking operations for debugging
    async fn log_chunking_operation(
        &self,
        file_path: &Path,
        truncated: bool,
        total_lines: Option<usize>,
    ) -> Result<()> {
        if truncated {
            let log_entry = json!({
                "operation": "read_file_chunked",
                "file_path": file_path.to_string_lossy(),
                "truncated": true,
                "total_lines": total_lines,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });

            info!(
                "File chunking operation: {}",
                serde_json::to_string(&log_entry)?
            );
        }
        Ok(())
    }

    fn resolve_file_path(&self, path: &str) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();

        // Try exact path first
        paths.push(self.workspace_root.join(path));

        // If it's just a filename, try common directories that exist in most projects
        if !path.contains('/') && !path.contains('\\') {
            // Generic source directories found in most projects
            paths.push(self.workspace_root.join("src").join(path));
            paths.push(self.workspace_root.join("lib").join(path));
            paths.push(self.workspace_root.join("bin").join(path));
            paths.push(self.workspace_root.join("app").join(path));
            paths.push(self.workspace_root.join("source").join(path));
            paths.push(self.workspace_root.join("sources").join(path));
            paths.push(self.workspace_root.join("include").join(path));
            paths.push(self.workspace_root.join("docs").join(path));
            paths.push(self.workspace_root.join("doc").join(path));
            paths.push(self.workspace_root.join("examples").join(path));
            paths.push(self.workspace_root.join("example").join(path));
            paths.push(self.workspace_root.join("tests").join(path));
            paths.push(self.workspace_root.join("test").join(path));
        }

        // Try case-insensitive variants for filenames
        if !path.contains('/')
            && !path.contains('\\')
            && let Ok(entries) = std::fs::read_dir(&self.workspace_root)
        {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string()
                    && name.to_lowercase() == path.to_lowercase()
                {
                    paths.push(entry.path());
                }
            }
        }

        Ok(paths)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn is_image_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    let lowercase = extension.to_ascii_lowercase();
    matches!(
        lowercase.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tiff" | "tif" | "svg"
    )
}

#[cfg(test)]
mod paging_tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file_paging_lines() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let test_file = workspace_root.join("test_file.txt");

        // Create test content with 10 lines
        let test_content =
            "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n";
        fs::write(&test_file, test_content).unwrap();

        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        // Test basic paging functionality: offset_lines=2, page_size_lines=3
        // Should return lines 3, 4, 5 (0-indexed: 2, 3, 4)
        let args = json!({
            "path": test_file.to_string_lossy().into_owned(),
            "offset_lines": 2,
            "page_size_lines": 3
        });

        let result = file_ops.read_file(args).await.unwrap();
        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["content"].as_str().unwrap(), "line3\nline4\nline5");
    }

    #[tokio::test]
    async fn test_read_file_paging_bytes() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let test_file = workspace_root.join("test_file.txt");

        let test_content = "line1\nline2\nline3\nline4\nline5\n";
        fs::write(&test_file, test_content).unwrap();

        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        // Test byte-based paging: skip first 6 bytes ("line1\n") and read next 6 bytes
        let args = json!({
            "path": test_file.to_string_lossy().into_owned(),
            "offset_bytes": 6,
            "page_size_bytes": 6
        });

        let result = file_ops.read_file(args).await.unwrap();
        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["content"].as_str().unwrap(), "line2\n");
    }

    #[tokio::test]
    async fn test_read_file_offset_beyond_size() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let test_file = workspace_root.join("test_file.txt");

        let test_content = "line1\nline2\nline3\n";
        fs::write(&test_file, test_content).unwrap();

        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        // Test when offset is beyond file size
        let args = json!({
            "path": test_file.to_string_lossy().into_owned(),
            "offset_lines": 100,
            "page_size_lines": 10
        });

        let result = file_ops.read_file(args).await.unwrap();
        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["content"].as_str().unwrap(), "");
    }

    #[tokio::test]
    async fn test_read_file_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let test_file = workspace_root.join("empty_file.txt");

        fs::write(&test_file, "").unwrap();

        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        // Test reading empty file with paging
        let args = json!({
            "path": test_file.to_string_lossy().into_owned(),
            "offset_lines": 0,
            "page_size_lines": 10
        });

        let result = file_ops.read_file(args).await.unwrap();
        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["content"].as_str().unwrap(), "");
    }

    #[tokio::test]
    async fn test_read_file_legacy_functionality() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let test_file = workspace_root.join("test_file.txt");

        let test_content = "line1\nline2\nline3\nline4\nline5\n";
        fs::write(&test_file, test_content).unwrap();

        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        // Test legacy functionality with max_bytes
        let args = json!({
            "path": test_file.to_string_lossy().into_owned(),
            "max_bytes": 10
        });

        let result = file_ops.read_file(args).await.unwrap();
        assert!(result["success"].as_bool().unwrap());
        let content = result["content"].as_str().unwrap();
        assert!(content.len() <= 10);
        assert!(content.starts_with("line1"));
    }

    #[tokio::test]
    async fn test_read_file_legacy_token_chunking() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let test_file = workspace_root.join("test_file.txt");

        // Create test content with 50 lines
        let test_content = (1..=50)
            .map(|i| format!("line-{}", i))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&test_file, test_content).unwrap();

        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        // Token budget small enough to keep roughly first+last 5-10 lines
        let max_tokens = 15 * 12; // ~12 lines worth using TOKENS_PER_LINE

        let args = json!({
            "path": test_file.to_string_lossy().into_owned(),
            "max_tokens": max_tokens
        });

        let result = file_ops.read_file(args).await.unwrap();
        assert!(result["success"].as_bool().unwrap());
        let content = result["content"].as_str().unwrap();
        // Should contain first and last lines
        assert!(content.contains("line-1"));
        assert!(content.contains("line-50"));
        // Should indicate truncation
        assert!(result["is_truncated"].as_bool().unwrap());
        // Should report applied token budget
        assert_eq!(
            result["metadata"]["applied_max_tokens"].as_u64().unwrap(),
            max_tokens as u64
        );
    }
}
