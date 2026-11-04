use super::walk_entry::{walk_entries, walk_first_matching};
use super::{FileOpsTool, ListNameMatcher, compile_list_glob, list_entry_matches};
use crate::tools::file_ops::path_policy::PathSuggestionKind;
use crate::tools::grep_file::GrepSearchInput;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use crate::types::CompactStr;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;

impl FileOpsTool {
    /// Execute recursive file search
    pub(crate) async fn execute_recursive_search(&self, input: &ListInput) -> Result<Value> {
        let list_glob = compile_list_glob(input)?;
        let pattern = input.glob_pattern.as_deref().or(input.name_pattern.as_deref()).unwrap_or("*");
        let search_path = self.normalize_list_path(input).await?;

        // Check if path exists before walking
        if !tokio::fs::try_exists(&search_path).await.unwrap_or(false) {
            let suggestion = self.missing_path_suggestion_suffix(&input.path, PathSuggestionKind::Any).await;
            return Err(anyhow!(
                "Path '{}' does not exist. Workspace root: {}{}",
                input.path,
                self.workspace_root.display(),
                suggestion,
            ));
        }

        // Built after the existence check so missing paths skip even this one allocation.
        let name_matcher = ListNameMatcher::new(input);

        // Pre-allocate with max_items capacity to avoid reallocations - AGENTS.md max 5 items
        let mut items = Vec::with_capacity(input.max_items.min(5));
        let mut count = 0;
        let mut matched_total = 0;

        let walker_entries = walk_entries(search_path.clone()).await?;

        for entry in walker_entries {
            let path = entry.path;
            let depth = entry.depth;
            let is_dir = entry.is_dir;
            if self.should_exclude(&path).await {
                continue;
            }

            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !input.include_hidden && name.starts_with('.') {
                continue;
            }

            let relative_path = self.relative_path_json_for_entry(&path, entry.file_type);
            if !list_entry_matches(&name_matcher, &relative_path, &name, list_glob.as_ref()) {
                continue;
            }

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

            matched_total += 1;
            if count >= input.max_items {
                continue;
            }

            items.push(json!({
                "name": name,
                "path": relative_path,
                "type": if is_dir { "directory" } else { "file" },
                "depth": depth
            }));
            count += 1;
        }

        // Add overflow indication if we found more items than max_items
        let mut result = self.paginate_and_format(items, count, input, "recursive", Some(pattern));
        if matched_total > input.max_items
            && let Some(obj) = result.as_object_mut()
        {
            obj.insert("overflow".to_string(), json!(format!("[+{} more items]", matched_total - input.max_items)));
        }
        Ok(result)
    }

    /// Execute find by exact name
    pub(crate) async fn execute_find_by_name(&self, input: &ListInput) -> Result<Value> {
        let file_name = input
            .name_pattern
            .as_ref()
            .ok_or_else(|| anyhow!("Error: Invalid 'list_files' arguments. When mode='find_name', must provide name_pattern (string). Example: {{\"path\": \".\", \"mode\": \"find_name\", \"name_pattern\": \"Cargo.toml\"}}"))?;
        let search_path = self.normalize_list_path(input).await?;
        // Lower the wanted name once: the loop below runs per directory entry.
        let wanted_lower: Option<CompactStr> = if input.case_sensitive.unwrap_or(true) {
            None
        } else {
            Some(CompactStr::from(file_name.to_lowercase()))
        };

        // This mode answers with one path, so the walk stops at the first
        // surviving match instead of collecting the whole tree. Exclusion is an
        // independent per-entry predicate, so evaluating it inside the walk
        // cannot change which entry wins; the ignore snapshot is taken once and
        // moved in so no lock is held across the traversal.
        let gitignore = crate::utils::vtcodegitignore::snapshot_global_vtcode_gitignore().await;
        let wanted = file_name.clone();
        let found = walk_first_matching(search_path.clone(), move |entry| {
            let name = entry.file_name();
            let matches = match &wanted_lower {
                Some(lowered) => name.to_string_lossy().to_lowercase() == lowered.as_str(),
                None => name == std::ffi::OsStr::new(wanted.as_str()),
            };
            matches && !gitignore.should_exclude_with_file_type(entry.path(), entry.file_type())
        })
        .await?;

        if let Some(entry) = found {
            let path = entry.path;
            return Ok(json!({
                "success": true,
                "found": true,
                "name": path.file_name().unwrap_or_default().to_string_lossy(),
                "path": self.relative_path_json_for_entry(&path, entry.file_type),
                "type": if entry.is_dir { "directory" } else { "file" },
                "mode": "find_name"
            }));
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

        let search_root = self.normalize_list_path(input).await?;
        if self.should_exclude(&search_root).await {
            return Err(anyhow!("Path '{}' is excluded by .vtcodegitignore", input.path));
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

        let mut seen_paths = hashbrown::HashSet::with_capacity(result.matches.len());
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
                // Render through the shared helper rather than a bare
                // `strip_prefix(workspace_root)`: grep reports resolved paths,
                // and a workspace reached through an alias (`/var` vs macOS's
                // `/private/var`) would otherwise stay absolute here while the
                // same file renders relative in `list` mode.
                items.push(json!({
                    "name": absolute_path.file_name().unwrap_or_default().to_string_lossy(),
                    "path": self.relative_path_json(&absolute_path),
                    "type": "file",
                    "pattern_found": true
                }));
            }
        }

        let total_count = items.len();
        Ok(self.paginate_and_format(items, total_count, input, "find_content", Some(content_pattern)))
    }
}
