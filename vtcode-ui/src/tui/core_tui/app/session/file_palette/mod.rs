use std::path::PathBuf;

use ratatui_cheese::tree::{TreeGroup, TreeState};

use crate::tui::ui::FileColorizer;

mod filtering;
mod navigation;
mod references;

pub use references::extract_file_reference;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub display_name: String,
    pub relative_path: String,
    pub is_dir: bool,
}

pub struct FilePalette {
    all_files: Vec<FileEntry>,
    filtered_files: Vec<FileEntry>,
    tree_groups: Vec<TreeGroup>,
    group_entries: Vec<Vec<FileEntry>>,
    tree_state: TreeState,
    filter_query: String,
    workspace_root: PathBuf,
    filter_cache: hashbrown::HashMap<String, Vec<FileEntry>>,
    file_colorizer: FileColorizer,
}

impl FilePalette {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            all_files: Vec::new(),
            filtered_files: Vec::new(),
            tree_groups: Vec::new(),
            group_entries: Vec::new(),
            tree_state: TreeState::new(0),
            filter_query: String::new(),
            workspace_root,
            filter_cache: hashbrown::HashMap::new(),
            file_colorizer: FileColorizer::new(),
        }
    }

    pub fn reset(&mut self) {
        self.filter_query.clear();
        self.apply_filter();
    }

    pub fn cleanup(&mut self) {
        self.filter_cache.clear();
        self.filtered_files.clear();
        self.filtered_files.shrink_to_fit();
        self.tree_groups.clear();
        self.group_entries.clear();
        self.tree_state = TreeState::new(0);
    }

    pub fn tree_groups(&self) -> &[TreeGroup] {
        &self.tree_groups
    }

    pub fn tree_state(&self) -> &TreeState {
        &self.tree_state
    }

    pub fn tree_state_mut(&mut self) -> &mut TreeState {
        &mut self.tree_state
    }
}

#[cfg(test)]
mod tests;
