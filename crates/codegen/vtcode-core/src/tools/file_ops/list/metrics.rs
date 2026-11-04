use super::FileOpsTool;
use crate::tools::file_ops::path_policy::PathSuggestionKind;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Result, anyhow};
use hashbrown::HashSet;
use serde_json::{Value, json};
use std::path::PathBuf;
use vtcode_commons::walk::build_walker_single_threaded;

impl FileOpsTool {
    pub(crate) async fn execute_largest_files(&self, input: &ListInput) -> Result<Value> {
        let search_root = self.normalize_list_path(input).await?;

        if !tokio::fs::try_exists(&search_root).await.unwrap_or(false) {
            let suggestion = self.missing_path_suggestion_suffix(&input.path, PathSuggestionKind::Any).await;
            return Err(anyhow!("Path '{}' does not exist{}", input.path, suggestion,));
        }

        if self.should_exclude(&search_root).await {
            return Err(anyhow!("Path '{}' is excluded by .vtcodegitignore", input.path));
        }

        let normalize_extension = |value: &str| value.trim_start_matches('.').to_lowercase();
        let extension_filter: Option<HashSet<String>> = input
            .file_extensions
            .as_ref()
            .map(|exts| exts.iter().map(|ext| normalize_extension(ext)).collect::<HashSet<_>>());

        // Workspace-relative rendering must run inside the blocking scan, and it
        // needs the resolved root: the walker reports paths through the
        // canonical alias (`/private/var/...` on macOS) while `workspace_root`
        // may keep the user-facing form (`/var/...`). Hand-rolling
        // `strip_prefix(workspace_root)` silently failed on that mismatch and
        // then treated the aliased prefix as a hidden component.
        let path_display = self.path_display.clone();
        let include_hidden = input.include_hidden;

        // Perform directory traversal in a blocking task to avoid blocking the async executor
        let search_root_clone = search_root.clone();
        let extension_filter_clone = extension_filter.clone();

        /// Any `.`-prefixed component other than `.` and `..` marks the path hidden.
        ///
        /// Walks components rather than splitting text so the platform separator
        /// is handled, and inspects raw bytes so no allocation is paid per entry.
        fn has_hidden_component(relative: &std::path::Path) -> bool {
            relative.components().any(|component| {
                let bytes = component.as_os_str().as_encoded_bytes();
                bytes.first() == Some(&b'.') && bytes != b"." && bytes != b".."
            })
        }

        let raw_entries = tokio::task::spawn_blocking(move || {
            let mut entries = Vec::new();
            for entry in build_walker_single_threaded(&search_root_clone).build() {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("Walk error: {}", e);
                        continue;
                    }
                };
                let path = entry.path();

                // A regular file needs no `is_file()` probe: the dirent type
                // from `readdir` already settled it, saving one stat for the
                // common case. Symlinks and special files keep the original
                // follow-then-decide gate, because only the target can say
                // whether they are a file.
                let file_type = entry.file_type();
                if !file_type.is_some_and(|file_type| file_type.is_file()) && !path.is_file() {
                    continue;
                }

                // Render once: the same relative form feeds both the hidden
                // check and the response path.
                let relative_path = PathBuf::from(path_display.render_entry(path, file_type));
                if !include_hidden && has_hidden_component(&relative_path) {
                    continue;
                }

                if let Some(ref filters) = extension_filter_clone {
                    let extension = path.extension().and_then(|ext| ext.to_str()).map(normalize_extension);

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
        .map_err(|e| anyhow!("Blocking task join error: {e}"))?;

        // AGENTS.md requires max 5 items for context optimization.
        let effective_max = input.max_items.min(5);

        // Keep only the top `effective_max` rows instead of collecting and
        // sorting every file in the tree: the answer is a bounded ranking, and
        // the caller only needs a count of the rest.
        let mut entries: Vec<(u64, PathBuf, Option<u64>)> = Vec::with_capacity(effective_max.min(8));
        let mut total_entries = 0usize;
        for (size, rel_path, modified, abs_path) in raw_entries {
            // Every row here already passed the `is_file` gate above, so the
            // directory flag is known: pass it instead of letting the matcher
            // stat each path again.
            if self.should_exclude_with_is_dir(&abs_path, false).await {
                continue;
            }
            total_entries += 1;

            // Descending size, then ascending path — the same order the full
            // sort produced, applied while inserting.
            let position = entries
                .iter()
                .position(|(existing_size, existing_path, _)| {
                    size > *existing_size || (size == *existing_size && rel_path < *existing_path)
                })
                .unwrap_or(entries.len());
            if position < effective_max {
                entries.insert(position, (size, rel_path, modified));
                entries.truncate(effective_max);
            }
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

        // Already ordered by the bounded insert above.
        let selected_total = entries.len();
        let has_overflow = total_entries > effective_max;

        let mut ranked = Vec::with_capacity(selected_total);
        for (idx, (size, rel_path, modified)) in entries.into_iter().enumerate() {
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
            obj.insert("overflow".to_string(), json!(format!("[+{} more items]", total_entries - effective_max)));
        }

        let note = format!(
            "Results sorted by file size (descending). Showing top {} file(s).",
            output.get("count").and_then(|value| value.as_u64()).unwrap_or(0)
        );
        if let Some(obj) = output.as_object_mut() {
            obj.entry("message".to_string())
                .and_modify(|value| {
                    if let Some(existing) = value.as_str() {
                        *value = json!(format!("{existing} {note}"));
                    } else {
                        *value = json!(note.clone());
                    }
                })
                .or_insert_with(|| json!(note));
        } else {
            tracing::warn!("largest_files output was not an object; skipped message merge");
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::grep_file::GrepSearchManager;
    use crate::tools::traits::Tool;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_tool(workspace: &TempDir) -> FileOpsTool {
        FileOpsTool::new(
            workspace.path().to_path_buf(),
            Arc::new(GrepSearchManager::new(workspace.path().to_path_buf())),
        )
    }

    fn write_sized(dir: &Path, name: &str, size: usize) {
        std::fs::write(dir.join(name), vec![b'x'; size]).expect("fixture file");
    }

    fn ranked(result: &Value) -> Vec<(String, u64)> {
        result["items"]
            .as_array()
            .expect("items array")
            .iter()
            .map(|item| (item["name"].as_str().expect("name").to_string(), item["size"].as_u64().expect("size")))
            .collect()
    }

    #[tokio::test]
    async fn largest_ranks_by_size_desc_then_path_asc_and_caps_to_five() {
        let workspace = TempDir::new().unwrap();
        let scan = workspace.path().join("scan");
        std::fs::create_dir(&scan).unwrap();

        // Eight files; only the five largest may be returned.
        write_sized(&scan, "tiny.txt", 1);
        write_sized(&scan, "a.txt", 40);
        write_sized(&scan, "b.txt", 40);
        write_sized(&scan, "c.txt", 30);
        write_sized(&scan, "d.txt", 20);
        write_sized(&scan, "e.txt", 10);
        write_sized(&scan, "f.txt", 5);
        write_sized(&scan, "g.txt", 2);

        let result = make_tool(&workspace)
            .execute(json!({"path": "scan", "mode": "largest", "max_items": 20}))
            .await
            .unwrap();

        // Equal sizes break ties by ascending path, and the cap holds at five.
        assert_eq!(
            ranked(&result),
            vec![
                ("a.txt".to_string(), 40),
                ("b.txt".to_string(), 40),
                ("c.txt".to_string(), 30),
                ("d.txt".to_string(), 20),
                ("e.txt".to_string(), 10),
            ]
        );
        assert_eq!(result["count"], 5);
        // The reported total counts every candidate, not just the kept page.
        assert_eq!(result["total"], 5);
        assert_eq!(result["has_more"], false);
        assert_eq!(result["overflow"], "[+3 more items]");
    }

    #[tokio::test]
    async fn largest_skips_zero_byte_and_directory_entries() {
        let workspace = TempDir::new().unwrap();
        let scan = workspace.path().join("scan");
        std::fs::create_dir(&scan).unwrap();
        std::fs::create_dir(scan.join("subdir")).unwrap();
        write_sized(&scan, "empty.txt", 0);
        write_sized(&scan, "kept.txt", 7);

        let result = make_tool(&workspace)
            .execute(json!({"path": "scan", "mode": "largest"}))
            .await
            .unwrap();

        assert_eq!(ranked(&result), vec![("kept.txt".to_string(), 7)]);
    }
}
