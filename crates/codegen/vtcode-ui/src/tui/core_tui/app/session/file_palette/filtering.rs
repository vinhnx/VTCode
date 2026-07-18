use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::{FileEntry, FilePalette, PickerMode};

impl FilePalette {
    pub(super) fn should_exclude_file(path: &Path) -> bool {
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if vtcode_commons::exclusions::is_sensitive_file(file_name) || file_name == ".git" {
                return true;
            }

            if file_name.starts_with('.') {
                return true;
            }
        }

        if path.components().any(|c| c.as_os_str() == ".git") {
            return true;
        }

        false
    }

    pub fn load_files(&mut self, files: Vec<String>) {
        self.populate_all_files(files);
        self.current_dir = self.workspace_root.clone();
        self.set_filter(String::new());
    }

    /// Build `all_files` and the full `dir_index` from a complete recursive file
    /// list. Used by [`Self::load_files`] (tests); the runtime path calls
    /// [`Self::set_search_index`] and lets `dir_index` build lazily via
    /// `ensure_dir_listing` so Browse mode never walks the whole workspace.
    pub(super) fn populate_all_files(&mut self, files: Vec<String>) {
        self.build_entries(files, true);
        self.build_dir_index();
    }

    /// Convert a raw recursive file list into [`FileEntry`] values. When
    /// `detect_dirs` is false (the runtime Search path, whose input comes from
    /// `discover_files` which already filters to regular files), the per-file
    /// `is_dir()` stat is skipped since every entry is known to be a file.
    pub(super) fn build_entries(&mut self, files: Vec<String>, detect_dirs: bool) {
        self.all_files = files
            .into_iter()
            .filter(|path| !Self::should_exclude_file(Path::new(path)))
            .map(|path| {
                let relative_path = Self::make_relative(&self.workspace_root, &path);
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

        self.all_files
            .sort_by(|a, b| a.relative_path.to_lowercase().cmp(&b.relative_path.to_lowercase()));
    }

    /// Build `dir_index`: for every directory, the list of its *direct* children
    /// (subdirectories and files). Derived once from `all_files` so that
    /// navigating the tree never re-scans the whole workspace. Hidden directories
    /// (names starting with `.`) are pruned, matching the browse exclusion rules.
    fn build_dir_index(&mut self) {
        let root = self.workspace_root.clone();
        let mut index: BTreeMap<PathBuf, Vec<FileEntry>> = BTreeMap::new();

        for entry in &self.all_files {
            let path = Path::new(&entry.path);
            if let Some(parent) = path.parent() {
                Self::insert_child(&mut index, parent, &root, entry.path.clone(), false);
            }

            // Register every ancestor directory as a child of its own parent so
            // chains of nested directories are all navigable.
            let mut ancestor = path.parent();
            while let Some(dir) = ancestor {
                if dir == root {
                    break;
                }
                if let Some(grandparent) = dir.parent() {
                    Self::insert_child(&mut index, grandparent, &root, dir.display().to_string(), true);
                }
                ancestor = dir.parent();
            }
        }

        self.dir_index = index;
    }

    /// Insert a child entry under `parent` in `index`, de-duplicating directories
    /// (a directory is reached via multiple descendant files) and skipping hidden
    /// directories.
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
        let relative_path = Self::make_relative(root, &child_path);

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

    fn make_relative(workspace: &Path, file_path: &str) -> String {
        let path = Path::new(file_path);
        path.strip_prefix(workspace).unwrap_or(path).to_string_lossy().to_string()
    }

    pub fn set_filter(&mut self, query: String) {
        self.filter_query = query;
        if self.filter_query.is_empty() {
            self.mode = PickerMode::Browse;
            self.rebuild_dir_listing();
        } else {
            self.mode = PickerMode::Search;
            self.rebuild_search();
        }
    }

    /// Build the current directory's contents. The listing is fetched lazily from
    /// `dir_lister` (cached per directory), so navigating never walks the whole
    /// workspace — only the directory the user is currently viewing.
    pub(super) fn rebuild_dir_listing(&mut self) {
        let cur = self.current_dir.clone();
        let root = self.workspace_root.clone();

        let mut children = self.ensure_dir_listing(&cur);
        children.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
        });

        let mut listing: Vec<FileEntry> = Vec::with_capacity(children.len() + 1);
        if cur != root {
            let parent_path = cur.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| root.clone());
            let relative_path = Self::make_relative(&root, &parent_path.display().to_string());
            listing.push(FileEntry {
                path: parent_path.display().to_string(),
                display_name: "..".to_string(),
                relative_path,
                is_dir: true,
                is_parent: true,
            });
        }
        listing.extend(children);

        self.filtered_files = listing;
        self.select_first();
    }

    /// Return the immediate children of `dir`, loading and caching them via
    /// `dir_lister` on first access. The lister is supplied by the runloop and
    /// performs a shallow, ignore-aware directory read.
    fn ensure_dir_listing(&mut self, dir: &Path) -> Vec<FileEntry> {
        if let Some(cached) = self.dir_index.get(dir) {
            return cached.clone();
        }

        let raw = self.dir_lister.list(dir);
        let entries: Vec<FileEntry> = raw
            .into_iter()
            .map(|(path, is_dir)| {
                let display_name = if is_dir {
                    format!("{}/", path.file_name().and_then(|n| n.to_str()).unwrap_or_default())
                } else {
                    path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string()
                };
                let relative_path = Self::make_relative(&self.workspace_root, &path.display().to_string());
                FileEntry {
                    path: path.display().to_string(),
                    display_name,
                    relative_path,
                    is_dir,
                    is_parent: false,
                }
            })
            .collect();

        self.dir_index.insert(dir.to_path_buf(), entries.clone());
        entries
    }

    pub(super) fn rebuild_search(&mut self) {
        let query = self.filter_query.clone();
        let query_lower = query.to_lowercase();

        struct ScoredPath {
            score: usize,
            index: usize,
            is_dir: bool,
            path_lower: String,
        }

        // The recursive index is filled in asynchronously; until then Search mode
        // has no corpus to match against.
        if self.all_files.is_empty() {
            self.filtered_files.clear();
            self.select_first();
            return;
        }

        let mut scored: Vec<ScoredPath> = Vec::with_capacity(self.all_files.len() / 2);
        let mut buffer = Vec::new();
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(&query_lower, CaseMatching::Ignore, Normalization::Smart);

        for (idx, entry) in self.all_files.iter().enumerate() {
            let path_lower = entry.relative_path.to_lowercase();

            let score = if let Some(fuzzy_score) =
                Self::simple_fuzzy_match_with_buffer(&path_lower, &mut matcher, &pattern, &query_lower, &mut buffer)
            {
                let mut score = fuzzy_score;
                if !path_lower.contains('/') {
                    score += 1000;
                }
                if path_lower == query_lower {
                    score += 10000;
                } else if let Some(file_name) = path_lower.rsplit('/').next() {
                    if file_name == query_lower {
                        score += 5000;
                    } else if file_name.starts_with(&query_lower) {
                        score += 2000;
                    }
                }
                score
            } else if path_lower.contains(&query_lower) {
                let mut score = Self::calculate_match_score(&path_lower, &query_lower);
                if !path_lower.contains('/') {
                    score += 1000;
                }
                score
            } else {
                continue;
            };

            scored.push(ScoredPath {
                score,
                index: idx,
                is_dir: entry.is_dir,
                path_lower,
            });
        }

        scored.sort_unstable_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.score.cmp(&a.score).then_with(|| a.path_lower.cmp(&b.path_lower)),
        });

        self.filtered_files = scored
            .into_iter()
            .map(|scored_path| self.all_files[scored_path.index].clone())
            .collect();
        self.select_first();
    }

    pub(super) fn select_first(&mut self) {
        if self.filtered_files.is_empty() {
            self.selected = None;
            return;
        }
        // Skip the synthetic `..` parent entry so the first real item is selected.
        let start = if self.filtered_files.first().is_some_and(|e| e.is_parent) {
            1
        } else {
            0
        };
        self.selected = Some(start.min(self.filtered_files.len() - 1));
    }

    #[expect(dead_code)]
    pub(super) fn simple_fuzzy_match(path: &str, query: &str) -> Option<usize> {
        if query.is_empty() {
            return Some(1000);
        }
        let mut buffer = Vec::new();
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let query_lower = query.to_lowercase();
        Self::simple_fuzzy_match_with_buffer(path, &mut matcher, &pattern, &query_lower, &mut buffer)
    }

    /// Scores `path` against a pre-parsed `pattern`/`matcher` pair. Callers own
    /// the matcher, pattern, and lowercased query so they can be built once and
    /// reused across an entire candidate list. `query_lower` must already be
    /// lowercased and non-empty.
    fn simple_fuzzy_match_with_buffer(
        path: &str,
        matcher: &mut Matcher,
        pattern: &Pattern,
        query_lower: &str,
        buffer: &mut Vec<char>,
    ) -> Option<usize> {
        if query_lower.is_empty() {
            return Some(1000);
        }

        let utf32_path = Utf32Str::new(path, buffer);
        let score = pattern.score(utf32_path, matcher)?;

        let mut adjusted_score = score as usize;

        if let Some(filename) = path.rsplit('/').next()
            && filename.to_lowercase().contains(query_lower)
        {
            adjusted_score += 500;
        }

        Some(adjusted_score)
    }

    fn calculate_match_score(path: &str, query: &str) -> usize {
        let mut score: usize = 0;

        if path == query {
            return 10000;
        }

        if path.starts_with(query) {
            score += 1000;
        }

        if let Some(file_name) = path.rsplit('/').next() {
            if file_name == query {
                score += 2000;
            } else if file_name.contains(query) {
                score += 500;
            }
            if file_name.starts_with(query) {
                score += 200;
            }
        }

        for segment in path.split('/') {
            if segment.contains(query) {
                score += 50;
            }
        }

        let depth = path.matches('/').count();
        score = score.saturating_sub(depth * 5);

        let matches = path.matches(query).count();
        score += matches * 10;

        score
    }
}
