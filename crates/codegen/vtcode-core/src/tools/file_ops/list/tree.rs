use super::FileOpsTool;
use crate::tools::file_ops::path_policy::PathSuggestionKind;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::task::spawn_blocking;
use vtcode_commons::walk::build_default_walker;

/// Hard cap on emitted tree nodes. A depth-10 whole-repo walk otherwise
/// serializes the entire workspace tree into one tool result and is then
/// fuse-truncated mid-structure (unusable) or floods the model context.
const TREE_MAX_NODES: usize = 200;
/// Per-directory child cap. Intentionally not `ListInput::max_items` (default
/// 20) — that field is a flat list/page size; coupling it to tree made the
/// default tree far sparser than the pre-cap behavior.
const TREE_MAX_PER_DIR: usize = 50;

pub(super) async fn execute_tree_view(tool: &FileOpsTool, input: &ListInput) -> Result<Value> {
    let search_path = tool.normalize_list_path(input).await?;

    if !fs::try_exists(&search_path).await.unwrap_or(false) {
        let suggestion = tool.missing_path_suggestion_suffix(&input.path, PathSuggestionKind::Any).await;
        return Err(anyhow!("Path '{}' does not exist{}", input.path, suggestion,));
    }

    if tool.should_exclude(&search_path).await {
        return Err(anyhow!("Path '{}' is excluded by .vtcodegitignore", input.path));
    }

    let mut dir_contents: HashMap<String, Vec<(String, String)>> = HashMap::new(); // path -> [(name, type)]

    // Collect walker entries in a blocking task, then process async
    let walker_entries: Vec<(PathBuf, bool)> = spawn_blocking({
        let search_path = search_path.clone();
        move || {
            let mut entries = Vec::new();
            for entry in build_default_walker(&search_path).max_depth(Some(10)).build() {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let path = entry.path().to_path_buf();
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                entries.push((path, is_dir));
            }
            entries
        }
    })
    .await
    .context("Failed to spawn walker task")?;

    for (path, is_dir) in walker_entries {
        if tool.should_exclude(&path).await {
            continue;
        }

        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !input.include_hidden && name.starts_with('.') {
            continue;
        }

        if is_dir {
            let mut children = Vec::with_capacity(16);
            if let Ok(entries) = fs::read_dir(&path).await {
                let mut entries_list = Vec::with_capacity(32);
                let mut entry = entries;
                while let Ok(Some(file_entry)) = entry.next_entry().await {
                    let entry_name = file_entry.file_name().to_string_lossy().into_owned();
                    if !input.include_hidden && entry_name.starts_with('.') {
                        continue;
                    }
                    if tool.should_exclude(&file_entry.path()).await {
                        continue;
                    }
                    let is_dir = file_entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false);

                    entries_list.push((entry_name, if is_dir { "directory" } else { "file" }.to_string()));
                }
                children = entries_list;
            }

            let relative_path = path
                .strip_prefix(&tool.workspace_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            dir_contents.insert(relative_path, children);
        }
    }

    let mut budget = TREE_MAX_NODES;
    let mut truncated = false;
    let tree_structure =
        build_tree_structure(tool, &search_path, &dir_contents, input.include_hidden, &mut budget, &mut truncated);

    let mut response = json!({
        "success": true,
        "tree_structure": tree_structure,
        "path": input.path,
        "mode": "tree",
        "include_hidden": input.include_hidden
    });
    if truncated {
        response["tree_truncated"] = Value::Bool(true);
        response["tree_note"] = Value::String(format!(
            "tree truncated (max {TREE_MAX_NODES} nodes / {TREE_MAX_PER_DIR} per directory); use mode=list or mode=recursive for the remainder"
        ));
    }
    Ok(response)
}

fn build_tree_structure(
    tool: &FileOpsTool,
    base_path: &Path,
    dir_contents: &HashMap<String, Vec<(String, String)>>,
    include_hidden: bool,
    budget: &mut usize,
    truncated: &mut bool,
) -> Value {
    let relative_path = base_path
        .strip_prefix(&tool.workspace_root)
        .unwrap_or(base_path)
        .to_string_lossy()
        .to_string();

    let mut items = Vec::new();

    if let Some(contents) = dir_contents.get(&relative_path) {
        let cap = TREE_MAX_PER_DIR;
        let mut emitted = 0usize;
        for (name, entry_type) in contents {
            if !include_hidden && name.starts_with('.') {
                continue;
            }
            if *budget == 0 {
                *truncated = true;
                break;
            }
            if emitted >= cap {
                *truncated = true;
                break;
            }

            let item = if entry_type == "directory" {
                let sub_path = base_path.join(name);
                let sub_relative_path = sub_path
                    .strip_prefix(&tool.workspace_root)
                    .unwrap_or(&sub_path)
                    .to_string_lossy()
                    .to_string();

                *budget = budget.saturating_sub(1);
                let sub_children = if let Some(sub_contents) = dir_contents.get(&sub_relative_path) {
                    let mut sub_items = Vec::new();
                    let sub_cap = TREE_MAX_PER_DIR;
                    let mut sub_emitted = 0usize;
                    for (sub_name, sub_type) in sub_contents {
                        if !include_hidden && sub_name.starts_with('.') {
                            continue;
                        }
                        if *budget == 0 {
                            *truncated = true;
                            break;
                        }
                        if sub_emitted >= sub_cap {
                            *truncated = true;
                            break;
                        }
                        *budget = budget.saturating_sub(1);
                        sub_emitted += 1;
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
                *budget = budget.saturating_sub(1);
                json!({
                    "name": name,
                    "type": entry_type,
                    "path": base_path.join(name).to_string_lossy()
                })
            };

            emitted += 1;
            items.push(item);
        }
    }

    json!(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::grep_file::GrepSearchManager;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn test_tool() -> FileOpsTool {
        let root = PathBuf::from("/workspace");
        FileOpsTool::new(root, Arc::new(GrepSearchManager::new(PathBuf::from("/workspace"))))
    }

    fn flat_dir_entries(count: usize) -> HashMap<String, Vec<(String, String)>> {
        let mut map = HashMap::new();
        let mut children = Vec::with_capacity(count);
        for i in 0..count {
            children.push((format!("f{i:04}.rs"), "file".to_string()));
        }
        map.insert(String::new(), children);
        map
    }

    #[test]
    fn tree_structure_caps_nodes_and_flags_truncation() {
        // Root has 6 directories × 50 children = 6 dir nodes + 300 file nodes.
        // Budget (200) must stop before the full fan-out and flag truncation.
        let tool = test_tool();
        let mut contents = HashMap::new();
        let mut root = Vec::new();
        for d in 0..6 {
            let name = format!("d{d}");
            root.push((name.clone(), "directory".to_string()));
            let children: Vec<(String, String)> =
                (0..50).map(|i| (format!("{name}/f{i:02}.rs"), "file".to_string())).collect();
            contents.insert(name, children);
        }
        contents.insert(String::new(), root);

        let mut budget = TREE_MAX_NODES;
        let mut truncated = false;
        let tree = build_tree_structure(&tool, Path::new("/workspace"), &contents, true, &mut budget, &mut truncated);

        let items = tree.as_array().expect("tree is an array");
        assert!(truncated, "overflow must be flagged");
        let mut nodes = 0usize;
        for item in items {
            nodes += 1;
            if let Some(children) = item.get("children").and_then(Value::as_array) {
                nodes += children.len();
            }
        }
        assert!(nodes <= TREE_MAX_NODES, "emitted {nodes} nodes");
        assert!(nodes >= TREE_MAX_NODES.saturating_sub(1), "budget should be nearly exhausted, got {nodes}");
    }

    #[test]
    fn tree_structure_respects_per_dir_cap() {
        let tool = test_tool();
        let contents = flat_dir_entries(TREE_MAX_PER_DIR + 20);
        let mut budget = TREE_MAX_NODES;
        let mut truncated = false;
        let tree = build_tree_structure(&tool, Path::new("/workspace"), &contents, true, &mut budget, &mut truncated);

        let items = tree.as_array().expect("tree is an array");
        assert_eq!(items.len(), TREE_MAX_PER_DIR);
        assert!(truncated);
    }

    #[test]
    fn tree_ignores_list_max_items_for_per_dir_children() {
        // tree previously ignored ListInput::max_items; keep that so the list
        // default of 20 does not silently sparsify directory trees.
        let tool = test_tool();
        let contents = flat_dir_entries(40);
        let mut budget = TREE_MAX_NODES;
        let mut truncated = false;
        let tree = build_tree_structure(&tool, Path::new("/workspace"), &contents, true, &mut budget, &mut truncated);
        let items = tree.as_array().expect("tree is an array");
        assert_eq!(items.len(), 40, "40 children fit under TREE_MAX_PER_DIR");
        assert!(!truncated);
    }
}
