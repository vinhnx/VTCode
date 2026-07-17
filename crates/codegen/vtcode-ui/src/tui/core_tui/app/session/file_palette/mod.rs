use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::tui::ui::FileColorizer;

mod filtering;
mod navigation;
mod references;

pub use references::extract_file_reference;

/// Lists the immediate children of a directory without recursing, so the picker
/// only touches the directories the user actually opens. Returns `(path, is_dir)`
/// pairs with absolute paths. Supplied by the runloop (which owns the indexer) so
/// the UI crate stays free of indexing logic and dependencies.
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct DirLister(Arc<dyn Fn(&Path) -> Vec<(PathBuf, bool)> + Send + Sync>);

impl DirLister {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&Path) -> Vec<(PathBuf, bool)> + Send + Sync + 'static,
    {
        Self(Arc::new(f))
    }

    pub fn list(&self, dir: &Path) -> Vec<(PathBuf, bool)> {
        (self.0)(dir)
    }
}

impl std::fmt::Debug for DirLister {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DirLister")
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub display_name: String,
    pub relative_path: String,
    pub is_dir: bool,
    /// `true` for the synthetic `..` entry that ascends one directory.
    pub is_parent: bool,
}

/// Whether the palette is browsing a single directory or searching across the
/// whole workspace. The mode is derived from the filter query: an empty query
/// browses the current directory, a non-empty query searches every file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerMode {
    Browse,
    Search,
}

pub struct FilePalette {
    /// Every file in the workspace, populated lazily once the full recursive
    /// discovery finishes (Search mode). Empty until then — Browse mode never
    /// needs it because it lists one directory at a time via `dir_lister`.
    all_files: Vec<FileEntry>,
    /// Direct children of each directory that has been visited, keyed by the
    /// directory's absolute path. Filled on demand by `dir_lister`, so navigation
    /// is O(children) and the entire workspace is never walked up front.
    dir_index: BTreeMap<PathBuf, Vec<FileEntry>>,
    /// The entries currently shown — either the current directory's contents
    /// (Browse mode) or the fuzzy search results (Search mode).
    filtered_files: Vec<FileEntry>,
    current_dir: PathBuf,
    selected: Option<usize>,
    filter_query: String,
    mode: PickerMode,
    /// Name of the directory most recently entered, used to reselect it after
    /// ascending with `go_up`.
    last_entered: Option<String>,
    workspace_root: PathBuf,
    file_colorizer: FileColorizer,
    /// Supplies immediate directory contents on demand (see [`DirLister`]).
    dir_lister: DirLister,
}

impl FilePalette {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            all_files: Vec::new(),
            dir_index: BTreeMap::new(),
            filtered_files: Vec::new(),
            current_dir: workspace_root.clone(),
            selected: None,
            filter_query: String::new(),
            mode: PickerMode::Browse,
            last_entered: None,
            workspace_root,
            file_colorizer: FileColorizer::new(),
            dir_lister: DirLister::new(|_| Vec::new()),
        }
    }

    /// Configure the picker for a live workspace. The current directory's contents
    /// are loaded immediately (a single shallow listing); deeper directories are
    /// listed on demand as the user navigates, and the full recursive file list
    /// is filled in later via [`Self::set_search_index`] for Search mode.
    pub fn configure(&mut self, workspace_root: PathBuf, dir_lister: DirLister) {
        self.workspace_root = workspace_root.clone();
        self.current_dir = workspace_root;
        self.dir_lister = dir_lister;
        self.all_files.clear();
        self.all_files.shrink_to_fit();
        self.dir_index.clear();
        self.filter_query.clear();
        self.mode = PickerMode::Browse;
        self.last_entered = None;
        self.selected = None;
        self.rebuild_dir_listing();
    }

    /// Provide the full recursive file list (used by Search mode). Supplied by the
    /// runloop after its background discovery task finishes; Browse mode does not
    /// require it. Rebuilds the search view if the user is already searching.
    pub fn set_search_index(&mut self, files: Vec<String>) {
        // Search mode scans `all_files` directly and never touches `dir_index`;
        // the lazy flow builds `dir_index` on demand via `ensure_dir_listing`
        // when the user browses, so skip the O(n) `dir_index` construction.
        // `discover_files` already returns regular files only, so `detect_dirs`
        // is false and no per-file `is_dir()` stat is performed.
        self.build_entries(files, false);
        if self.mode == PickerMode::Search {
            self.rebuild_search();
        }
    }

    pub fn reset(&mut self) {
        self.filter_query.clear();
        self.current_dir = self.workspace_root.clone();
        self.last_entered = None;
        self.mode = PickerMode::Browse;
        self.rebuild_dir_listing();
    }

    pub fn cleanup(&mut self) {
        self.filtered_files.clear();
        self.filtered_files.shrink_to_fit();
        self.all_files.clear();
        self.all_files.shrink_to_fit();
        self.dir_index.clear();
        self.current_dir = self.workspace_root.clone();
        self.selected = None;
        self.last_entered = None;
        self.mode = PickerMode::Browse;
    }

    pub fn list_entries(&self) -> &[FileEntry] {
        &self.filtered_files
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }

    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    pub fn is_search_mode(&self) -> bool {
        self.mode == PickerMode::Search
    }

    /// Human-readable breadcrumb of the current directory relative to the
    /// workspace root (or `/` when at the root).
    pub fn breadcrumb(&self) -> String {
        match self.current_dir.strip_prefix(&self.workspace_root) {
            Ok(rel) if rel.as_os_str().is_empty() => "/".to_string(),
            Ok(rel) => format!("/{}", rel.display()),
            Err(_) => self.current_dir.display().to_string(),
        }
    }

    pub fn style_for_entry(&self, entry: &FileEntry) -> Option<anstyle::Style> {
        let path = Path::new(&entry.path);
        self.file_colorizer.style_for_path(path)
    }
}

#[cfg(test)]
mod tests;
