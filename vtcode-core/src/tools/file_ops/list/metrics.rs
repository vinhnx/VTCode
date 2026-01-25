use super::FileOpsTool;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;
use walkdir::WalkDir;

impl FileOpsTool {
    pub(crate) async fn execute_largest_files(&self, input: &ListInput) -> Result<Value> {
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
}
