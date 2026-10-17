use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::tui::ui::FileColorizer;

mod filtering;
mod kind;
mod listing;
mod navigation;
mod references;
mod search;

pub use kind::{DirEntryInfo, FileKind};
pub(crate) use references::extract_file_reference;

/// Lists the immediate children of a directory without recursing, so the picker
/// only touches the directories the user actually opens. Returns [`DirEntryInfo`]
/// rows with absolute paths, including symlink and file-kind metadata. Supplied
/// by the runloop (which owns the indexer) so the UI crate stays free of
/// indexing logic and dependencies.
#[derive(Clone)]
#[allow(
    clippy::type_complexity,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub struct DirLister(Arc<dyn Fn(&Path) -> Vec<DirEntryInfo> + Send + Sync>);

impl DirLister {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&Path) -> Vec<DirEntryInfo> + Send + Sync + 'static,
    {
        Self(Arc::new(f))
    }

    fn list(&self, dir: &Path) -> Vec<DirEntryInfo> {
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
    path: String,
    pub(crate) display_name: String,
    pub(crate) relative_path: String,
    pub(crate) is_dir: bool,
    /// `true` for the synthetic `..` entry that ascends one directory.
    pub(crate) is_parent: bool,
    /// Type classification driving the row glyph.
    pub(crate) kind: FileKind,
    /// Resolved symlink target, rendered as `→ target` when present.
    pub(crate) symlink_target: Option<PathBuf>,
    /// `true` when the entry is a symlink whose target is missing.
    pub(crate) symlink_broken: bool,
}

impl FileEntry {
    /// Build a plain entry (no symlink metadata) with a classified kind.
    ///
    /// Search mode flattens the recursive file list, which carries no per-file
    /// metadata, so entries built here are classified without the POSIX exec
    /// bit. Only the `Executable`-by-bit distinction is lost; extension-based
    /// kinds (code/image/executable-by-extension) stay accurate.
    pub(crate) fn new(
        path: String,
        display_name: String,
        relative_path: String,
        is_dir: bool,
        is_parent: bool,
    ) -> Self {
        let kind = FileKind::classify(Path::new(&path), is_dir, false);
        Self {
            path,
            display_name,
            relative_path,
            is_dir,
            is_parent,
            kind,
            symlink_target: None,
            symlink_broken: false,
        }
    }
}

/// Whether the palette is browsing a single directory or searching across the
/// whole workspace. The mode is derived from the filter query: an empty query
/// browses the current directory, a non-empty query searches every file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerMode {
    Browse,
    Search,
}

#[derive(Debug)]
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
    last_entered: Option<PathBuf>,
    workspace_root: PathBuf,
    file_colorizer: FileColorizer,
    /// Supplies immediate directory contents on demand (see [`DirLister`]).
    dir_lister: DirLister,
    /// Whether the background recursive index has been delivered. Tracked
    /// explicitly (rather than inferred from `all_files`) so an empty or
    /// fully-ignored workspace is not mistaken for "still indexing".
    search_index_delivered: bool,
}

impl FilePalette {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
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
            search_index_delivered: false,
        }
    }

    /// Configure the picker for a live workspace. The current directory's contents
    /// are loaded immediately (a single shallow listing); deeper directories are
    /// listed on demand as the user navigates, and the full recursive file list
    /// is filled in later via [`Self::set_search_index`] for Search mode.
    pub(crate) fn configure(&mut self, workspace_root: PathBuf, dir_lister: DirLister) {
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
        self.search_index_delivered = false;
        self.rebuild_dir_listing();
    }

    /// Provide the full recursive file list (used by Search mode). Supplied by the
    /// runloop after its background discovery task finishes; Browse mode does not
    /// require it. Rebuilds the search view if the user is already searching.
    pub(crate) fn set_search_index(&mut self, files: Vec<String>) {
        listing::build_entries(self, files, false);
        self.search_index_delivered = true;
        if self.mode == PickerMode::Search {
            self.rebuild_search();
        }
    }

    fn rebuild_dir_listing(&mut self) {
        listing::rebuild_dir_listing(self);
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
        self.search_index_delivered = false;
    }

    pub(crate) fn list_entries(&self) -> &[FileEntry] {
        &self.filtered_files
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }

    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    pub(crate) fn is_search_mode(&self) -> bool {
        self.mode == PickerMode::Search
    }

    /// Whether the background recursive index has arrived. Search cannot return
    /// results until it does; Browse mode does not need it. Distinct from an
    /// empty `all_files`, which also occurs for a genuinely empty or fully
    /// ignored workspace.
    pub(crate) fn search_index_loaded(&self) -> bool {
        self.search_index_delivered
    }

    /// Human-readable breadcrumb of the current directory relative to the
    /// workspace root (or `/` when at the root).
    pub(crate) fn breadcrumb(&self) -> String {
        match self.current_dir.strip_prefix(&self.workspace_root) {
            Ok(rel) if rel.as_os_str().is_empty() => "/".to_string(),
            Ok(rel) => format!("/{}", rel.display()),
            Err(_) => self.current_dir.display().to_string(),
        }
    }

    pub(crate) fn style_for_entry(&self, entry: &FileEntry) -> Option<anstyle::Style> {
        let path = Path::new(&entry.path);
        self.file_colorizer.style_for_path(path)
    }
}

#[cfg(test)]
mod tests;
