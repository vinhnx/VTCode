use super::FileOpsTool;
use crate::tools::builder::ToolResponseBuilder;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tracing::{info, warn};

mod metrics;
mod search;
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
            input.per_page.unwrap_or(20).max(1), // Keep bounded but avoid overly sparse default pages
        );
        let start = (page - 1).saturating_mul(per_page);
        let end = (start + per_page).min(capped_total);
        let has_more = end < capped_total;
        let has_overflow = all_items.len() > input.max_items;

        let mut page_items = if start < end {
            all_items[start..end].to_vec()
        } else {
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

        let mut builder = ToolResponseBuilder::new("list_files")
            .success()
            .field("items", json!(page_items))
            .field("count", json!(page_items.len()))
            .field("total", json!(capped_total))
            .field("page", json!(page))
            .field("per_page", json!(per_page))
            .field("has_more", json!(has_more))
            .field("mode", json!("list"))
            .field(
                "response_format",
                json!(if concise { "concise" } else { "detailed" }),
            );

        if let Some(msg) = guidance {
            builder = builder.message(msg);
        }

        let out = builder.build_json();

        // Cache the result for directories (TTL is 5 minutes in FILE_CACHE)
        if base.is_dir() {
            FILE_CACHE.put_directory(cache_key, out.clone()).await;
        }

        Ok(out)
    }

    /// Execute tree view of directory structure
    pub(super) async fn execute_tree_view(&self, input: &ListInput) -> Result<Value> {
        tree::execute_tree_view(self, input).await
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
