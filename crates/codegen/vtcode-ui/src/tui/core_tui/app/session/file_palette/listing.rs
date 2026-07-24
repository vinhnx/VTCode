use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{FileEntry, FilePalette};

fn make_relative(workspace: &Path, file_path: &str) -> String {
    let path = Path::new(file_path);
    path.strip_prefix(workspace)
        .or_else(|_| path.strip_prefix("/"))
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

pub(super) fn build_entries(palette: &mut FilePalette, files: Vec<String>, detect_dirs: bool) {
    palette.all_files = files
        .into_iter()
        .filter(|path| !FilePalette::should_exclude_file(Path::new(path)))
        .map(|path| {
            let relative_path = make_relative(&palette.workspace_root, &path);
            let is_dir = detect_dirs && Path::new(&path).is_dir();
            let display_name = if is_dir {
                format!("{relative_path}/")
            } else {
                relative_path.clone()
            };
            FileEntry {
                path,
                display_name,
                relative_path,
                is_dir,
                is_parent: false,
            }
        })
        .collect();

    palette
        .all_files
        .sort_by(|a, b| a.relative_path.to_lowercase().cmp(&b.relative_path.to_lowercase()));
}

pub(super) fn build_dir_index(palette: &mut FilePalette) {
    let root = palette.workspace_root.clone();
    let mut index: BTreeMap<PathBuf, Vec<FileEntry>> = BTreeMap::new();

    for entry in &palette.all_files {
        let path = Path::new(&entry.path);
        if let Some(parent) = path.parent() {
            insert_child(&mut index, parent, &root, entry.path.clone(), false);
        }

        let mut ancestor = path.parent();
        while let Some(dir) = ancestor {
            if dir == root {
                break;
            }
            if let Some(grandparent) = dir.parent() {
                insert_child(&mut index, grandparent, &root, dir.display().to_string(), true);
            }
            ancestor = dir.parent();
        }
    }

    palette.dir_index = index;
}

fn insert_child(
    index: &mut BTreeMap<PathBuf, Vec<FileEntry>>,
    parent: &Path,
    root: &Path,
    child_path: String,
    is_dir: bool,
) {
    let path = Path::new(&child_path);
    let child_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.to_string(),
        None => return,
    };
    if is_dir && child_name.starts_with('.') {
        return;
    }

    let display_name = if is_dir { format!("{child_name}/") } else { child_name };
    let relative_path = make_relative(root, &child_path);

    let entry = FileEntry {
        path: child_path,
        display_name,
        relative_path,
        is_dir,
        is_parent: false,
    };

    let children = index.entry(parent.to_path_buf()).or_default();
    if is_dir && children.iter().any(|c| c.display_name == entry.display_name) {
        return;
    }
    children.push(entry);
}

fn ensure_dir_listing(palette: &mut FilePalette, dir: &Path) -> Vec<FileEntry> {
    if let Some(cached) = palette.dir_index.get(dir) {
        return cached.clone();
    }

    let raw = palette.dir_lister.list(dir);
    let entries: Vec<FileEntry> = raw
        .into_iter()
        .map(|(path, is_dir)| {
            let display_name = if is_dir {
                format!("{}/", path.file_name().and_then(|n| n.to_str()).unwrap_or_default())
            } else {
                path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string()
            };
            let relative_path = make_relative(&palette.workspace_root, &path.display().to_string());
            FileEntry {
                path: path.display().to_string(),
                display_name,
                relative_path,
                is_dir,
                is_parent: false,
            }
        })
        .collect();

    palette.dir_index.insert(dir.to_path_buf(), entries.clone());
    entries
}

pub(super) fn rebuild_dir_listing(palette: &mut FilePalette) {
    let cur = palette.current_dir.clone();
    let root = palette.workspace_root.clone();

    let mut children = ensure_dir_listing(palette, &cur);
    children.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
    });

    let mut listing: Vec<FileEntry> = Vec::with_capacity(children.len() + 1);
    if cur != root {
        let parent_path = cur.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| root.clone());
        let relative_path = make_relative(&root, &parent_path.display().to_string());
        listing.push(FileEntry {
            path: parent_path.display().to_string(),
            display_name: "..".to_string(),
            relative_path,
            is_dir: true,
            is_parent: true,
        });
    }
    listing.extend(children);

    palette.filtered_files = listing;
    palette.select_first();
}
