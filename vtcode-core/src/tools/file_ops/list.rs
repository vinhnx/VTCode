use super::FileOpsTool;
use crate::tools::grep_file::GrepSearchInput;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use walkdir::WalkDir;

mod tree;

impl FileOpsTool {
    pub(super) async fn execute_basic_list(&self, input: &ListInput) -> Result<Value> {
        use crate::tools::cache::FILE_CACHE;

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

        // Try to get result from cache first for directories
        let cache_key = format!("dir_list:{}:hidden={}", input.path, input.include_hidden);
        if base.is_dir()
            && let Some(cached_result) = FILE_CACHE.get_directory(&cache_key).await
        {
            return Ok(cached_result);
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
                "Empty directory".to_string()
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

        // Cache the result for directories (TTL is 5 minutes in FILE_CACHE)
        if base.is_dir() {
            FILE_CACHE.put_directory(cache_key, out.clone()).await;
        }

        Ok(out)
    }

    /// Execute recursive file search
    pub(super) async fn execute_recursive_search(&self, input: &ListInput) -> Result<Value> {
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
    pub(super) async fn execute_find_by_name(&self, input: &ListInput) -> Result<Value> {
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

    /// Execute tree view of directory structure
    pub(super) async fn execute_tree_view(&self, input: &ListInput) -> Result<Value> {
        tree::execute_tree_view(self, input).await
    }

    /// Execute find by content pattern
    pub(super) async fn execute_find_by_content(&self, input: &ListInput) -> Result<Value> {
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

    pub(super) async fn execute_largest_files(&self, input: &ListInput) -> Result<Value> {
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

        // Perform directory traversal in a blocking task to avoid blocking the async executor
        let search_root_clone = search_root.clone();
        let workspace_root = self.workspace_root.clone();
        let include_hidden = input.include_hidden;
        let extension_filter_clone = extension_filter.clone();

        let raw_entries = tokio::task::spawn_blocking(move || {
            let mut entries = Vec::new();
            for entry in WalkDir::new(&search_root_clone).into_iter() {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("Walk error: {}", e);
                        continue;
                    }
                };
                let path = entry.path();

                if !path.is_file() {
                    continue;
                }

                if !include_hidden
                    && path_has_hidden(path.strip_prefix(&workspace_root).unwrap_or(path))
                {
                    continue;
                }

                if let Some(ref filters) = extension_filter_clone {
                    let extension = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(normalize_extension);

                    match extension {
                        Some(ext) if filters.contains(&ext) => {}
                        _ => continue,
                    }
                }

                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Metadata error for {:?}: {}", path, e);
                        continue;
                    }
                };
                let size_bytes = metadata.len();
                if size_bytes == 0 {
                    continue;
                }

                let relative_path = path
                    .strip_prefix(&workspace_root)
                    .unwrap_or(path)
                    .to_path_buf();
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());

                let absolute_path = path.to_path_buf();
                entries.push((size_bytes, relative_path, modified, absolute_path));
            }
            entries
        })
        .await
        .map_err(|e| anyhow!("Blocking task join error: {}", e))?;

        // Filter excluded paths asynchronously
        let mut entries = Vec::new();
        for (size, rel_path, modified, abs_path) in raw_entries {
            if self.should_exclude(&abs_path).await {
                continue;
            }
            entries.push((size, rel_path, modified));
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
        if has_overflow && let Some(obj) = output.as_object_mut() {
            obj.insert(
                "overflow".to_string(),
                json!(format!("[+{} more items]", total_entries - effective_max)),
            );
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

    pub(super) fn paginate_and_format(
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

}
