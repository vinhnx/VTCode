//! File-tree projection: the workspace tree rows built from a flat entry list.
//!
//! Both the files panel and the attachment picker render the same tree, so
//! the projection lives here instead of in either surface.

use std::collections::{HashMap, HashSet};

use gpui_kit::component::tree::TreeItem;

use crate::desktop::files::FileEntry;

/// Tree rows for the entries under `parent`, with expanded directories
/// expanded. Entries keep their scan order at every level.
pub(super) fn file_tree_items(
    entries: &[FileEntry],
    parent: Option<&str>,
    expanded: &HashSet<String>,
) -> Vec<TreeItem> {
    // Group once by parent directory so each level below walks only its own
    // children instead of re-scanning every entry; pushes preserve entry
    // order within each level, and output order never depends on map order.
    let mut by_parent: HashMap<Option<&str>, Vec<&FileEntry>> = HashMap::new();
    for entry in entries {
        by_parent
            .entry(entry.rel_path.rsplit_once('/').map(|(dir, _)| dir))
            .or_default()
            .push(entry);
    }
    file_tree_items_grouped(&by_parent, parent, expanded)
}

fn file_tree_items_grouped(
    by_parent: &HashMap<Option<&str>, Vec<&FileEntry>>,
    parent: Option<&str>,
    expanded: &HashSet<String>,
) -> Vec<TreeItem> {
    by_parent.get(&parent).map_or_else(Vec::new, |children| {
        children
            .iter()
            .map(|entry| {
                let nested = if entry.is_dir {
                    file_tree_items_grouped(by_parent, Some(&entry.rel_path), expanded)
                } else {
                    Vec::new()
                };
                TreeItem::new(entry.rel_path.clone(), entry.rel_path.clone())
                    .children(nested)
                    .expanded(expanded.contains(&entry.rel_path))
            })
            .collect()
    })
}

/// The tree row for `path`, searched depth-first through the nested rows.
pub(super) fn find_file_tree_item<'a>(items: &'a [TreeItem], path: &str) -> Option<&'a TreeItem> {
    items.iter().find_map(|item| {
        if item.id.as_ref() == path {
            Some(item)
        } else {
            find_file_tree_item(&item.children, path)
        }
    })
}
