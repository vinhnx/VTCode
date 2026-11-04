use super::FileOpsTool;
use super::walk_entry::walk_entries;
use crate::tools::file_ops::path_policy::PathSuggestionKind;
use crate::tools::traits::FileTool;
use crate::tools::types::ListInput;
use anyhow::{Result, anyhow};
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::path::Path;
use tokio::fs;

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

    let walker_entries = walk_entries(search_path.clone()).await?;

    // One pass builds the child map: the walker already enumerated every
    // directory, so re-reading each one with `read_dir` doubled the syscalls
    // for no new information.
    for entry in walker_entries {
        if entry.depth == 0 {
            continue;
        }
        let path = entry.path;
        let is_dir = entry.is_dir;
        // The walker's own type is already in hand, so the ignore matcher does
        // not need to stat again to learn `is_dir`.
        if tool.should_exclude_with_file_type(&path, entry.file_type).await {
            continue;
        }

        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !input.include_hidden && name.starts_with('.') {
            continue;
        }

        let Some(parent) = path.parent() else {
            continue;
        };
        // Key by workspace-relative form through the shared helper: the walker
        // reports resolved paths, so a bare `strip_prefix(workspace_root)`
        // silently falls back to absolute keys when the workspace is reached
        // through an alias (`/var` vs macOS's `/private/var`).
        let parent_key = tool.relative_key_json(parent);
        dir_contents
            .entry(parent_key)
            .or_insert_with(|| Vec::with_capacity(16))
            .push((name.into_owned(), if is_dir { "directory" } else { "file" }.to_string()));
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
    // Must match how `dir_contents` keys were produced, or an aliased workspace
    // root (`/var` vs `/private/var`) looks up nothing and emits an empty tree.
    let relative_path = tool.relative_key_json(base_path);

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
                let sub_relative_path = tool.relative_key_json(&sub_path);

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
                    "path": tool.relative_key_json(&sub_path)
                })
            } else {
                *budget = budget.saturating_sub(1);
                let file_path = base_path.join(name);
                json!({
                    "name": name,
                    "type": entry_type,
                    "path": tool.relative_key_json(&file_path)
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

    #[tokio::test]
    async fn tree_mode_builds_children_from_a_single_walk() {
        let workspace = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(workspace.path().join("src/nested")).unwrap();
        std::fs::write(workspace.path().join("top.rs"), "x").unwrap();
        std::fs::write(workspace.path().join("src/lib.rs"), "x").unwrap();
        std::fs::write(workspace.path().join("src/nested/deep.rs"), "x").unwrap();

        let tool = FileOpsTool::new(
            workspace.path().to_path_buf(),
            Arc::new(GrepSearchManager::new(workspace.path().to_path_buf())),
        );
        let tree = execute_tree_view(
            &tool,
            &ListInput {
                path: String::new(),
                max_items: 20,
                page: None,
                per_page: None,
                response_format: None,
                include_hidden: false,
                glob_pattern: None,
                mode: Some("tree".to_string()),
                name_pattern: None,
                content_pattern: None,
                file_extensions: None,
                case_sensitive: None,
            },
        )
        .await
        .unwrap();

        let items = tree["tree_structure"].as_array().expect("tree array");
        let names: Vec<&str> = items.iter().filter_map(|item| item["name"].as_str()).collect();
        assert!(names.contains(&"top.rs"), "{names:?}");
        assert!(names.contains(&"src"), "{names:?}");

        // Nested children must be populated from the walk, not a second readdir.
        let src = items.iter().find(|item| item["name"] == "src").expect("src node");
        let children = src["children"].as_array().expect("src children");
        let child_names: Vec<&str> = children.iter().filter_map(|c| c["name"].as_str()).collect();
        assert!(child_names.contains(&"lib.rs"), "{child_names:?}");
        assert!(child_names.contains(&"nested"), "{child_names:?}");
        assert_eq!(src["type"], "directory");

        // Emitted paths must be workspace-relative. The walker reports resolved
        // paths while `workspace_root` keeps the tempdir spelling, so a bare
        // `strip_prefix(workspace_root)` used to leak absolute paths here.
        for item in items {
            let path = item["path"].as_str().expect("path field");
            assert!(!Path::new(path).is_absolute(), "tree path must be workspace-relative, got {path:?}");
        }
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
