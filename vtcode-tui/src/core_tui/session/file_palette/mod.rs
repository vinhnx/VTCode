use std::path::PathBuf;

use crate::ui::FileColorizer;

mod filtering;
mod navigation;
mod palette_item;
mod references;

pub use references::extract_file_reference;

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    #[allow(dead_code)]
    pub display_name: String,
    pub relative_path: String,
    pub is_dir: bool,
}

pub struct FilePalette {
    all_files: Vec<FileEntry>,
    filtered_files: Vec<FileEntry>,
    selected_index: usize,
    current_page: usize,
    filter_query: String,
    workspace_root: PathBuf,
    filter_cache: std::collections::HashMap<String, Vec<FileEntry>>,
    #[allow(dead_code)]
    file_colorizer: FileColorizer,
}

impl FilePalette {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            all_files: Vec::new(),
            filtered_files: Vec::new(),
            selected_index: 0,
            current_page: 0,
            filter_query: String::new(),
            workspace_root,
            filter_cache: std::collections::HashMap::new(),
            file_colorizer: FileColorizer::new(),
        }
    }

    /// Reset selection and filter (call when opening file browser)
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.selected_index = 0;
        self.current_page = 0;
        self.filter_query.clear();
        self.apply_filter(); // Refresh filtered_files to show all
    }

    /// Clean up resources to free memory (call when closing file browser)
    pub fn cleanup(&mut self) {
        self.filter_cache.clear();
        self.filtered_files.clear();
        self.filtered_files.shrink_to_fit();
    }
}

#[cfg(test)]
mod tests;
