use super::FileOpsTool;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use walkdir::WalkDir;

pub(super) async fn execute_tree_view(tool: &FileOpsTool, input: &ListInput) -> Result<Value> {
    let search_path = tool.workspace_root.join(&input.path);

    if tool.should_exclude(&search_path).await {
        return Err(anyhow!(
            "Path '{}' is excluded by .vtcodegitignore",
            input.path
        ));
    }

    let mut dir_contents: HashMap<String, Vec<(String, String)>> = HashMap::new(); // path -> [(name, type)]

    // Walk the directory structure up to max_depth
    for entry in WalkDir::new(&search_path).max_depth(10).follow_links(false) {
        let entry = entry.map_err(|e| anyhow!("Walk error: {}", e))?;
        let path = entry.path();

        if tool.should_exclude(path).await {
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
                    if tool.should_exclude(&file_entry.path()).await {
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
                .strip_prefix(&tool.workspace_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            dir_contents.insert(relative_path, children);
        }
    }

    // Build tree structure
    let tree_structure =
        build_tree_structure(tool, &search_path, &dir_contents, input.include_hidden).await;

    Ok(json!({
        "success": true,
        "tree_structure": tree_structure,
        "path": input.path,
        "mode": "tree",
        "include_hidden": input.include_hidden
    }))
}

async fn build_tree_structure(
    tool: &FileOpsTool,
    base_path: &Path,
    dir_contents: &HashMap<String, Vec<(String, String)>>,
    include_hidden: bool,
) -> Value {
    let relative_path = base_path
        .strip_prefix(&tool.workspace_root)
        .unwrap_or(base_path)
        .to_string_lossy()
        .to_string();

    let mut items = Vec::with_capacity(
        dir_contents
            .get(&relative_path as &str)
            .map_or(0, |c| c.len()),
    );

    if let Some(contents) = dir_contents.get(&relative_path) {
        for (name, entry_type) in contents {
            if !include_hidden && name.starts_with('.') {
                continue;
            }

            let item = if entry_type == "directory" {
                // Try to get children for this subdirectory
                let sub_path = base_path.join(name);
                let sub_relative_path = sub_path
                    .strip_prefix(&tool.workspace_root)
                    .unwrap_or(&sub_path)
                    .to_string_lossy()
                    .to_string();

                let sub_children = if let Some(sub_contents) = dir_contents.get(&sub_relative_path) {
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
