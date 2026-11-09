#![allow(
    missing_docs,
    dead_code,
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
#![expect(
    unused_results,
    reason = "Indexer cache and walker updates intentionally use mutation APIs for side effects and discard prior entries."
)]

//! Workspace-friendly file indexer and file utilities for VT Code.
//!
//! `vtcode-indexer` provides:
//! - A lightweight workspace file indexer with markdown-backed persistence
//! - Fast parallel fuzzy file search (via `file_search` module)
//! - Markdown-backed storage utilities (via `markdown_store` module)

pub mod file_search;
pub mod markdown_store;

mod atomic_write;
mod snapshot;
mod storage;

pub use storage::{IndexStorage, MarkdownIndexStorage};

use anyhow::Result;
use hashbrown::HashMap;
use ignore::{DirEntry, Walk};
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Directory traversal filter hook for [`SimpleIndexer`].
pub trait TraversalFilter: Send + Sync {
    /// Determine if the indexer should descend into the provided directory.
    fn should_descend(&self, path: &Path, config: &SimpleIndexerConfig) -> bool;

    /// Determine if the indexer should process the provided file.
    fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool;

    /// Metadata-aware variant of [`TraversalFilter::should_index_file`].
    ///
    /// The default implementation delegates to [`TraversalFilter::should_index_file`],
    /// so existing filters keep their exact behavior. Filters whose file check
    /// starts with an existence test should override this to reuse `metadata`
    /// instead of stat-ing the path a second time.
    fn should_index_file_with_metadata(
        &self,
        path: &Path,
        metadata: &fs::Metadata,
        config: &SimpleIndexerConfig,
    ) -> bool {
        let _ = metadata;
        self.should_index_file(path, config)
    }
}

/// Default traversal filter powered by [`SimpleIndexerConfig`].
#[derive(Debug, Default, Clone)]
pub struct ConfigTraversalFilter;

impl TraversalFilter for ConfigTraversalFilter {
    fn should_descend(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
        !should_skip_dir(path, config)
    }

    fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
        path.is_file() && snapshot::has_indexable_file_name(path, config)
    }

    fn should_index_file_with_metadata(
        &self,
        path: &Path,
        metadata: &fs::Metadata,
        config: &SimpleIndexerConfig,
    ) -> bool {
        metadata.is_file() && snapshot::has_indexable_file_name(path, config)
    }
}

/// Configuration for [`SimpleIndexer`].
#[derive(Clone, Debug)]
pub struct SimpleIndexerConfig {
    workspace_root: PathBuf,
    index_dir: PathBuf,
    ignore_hidden: bool,
    excluded_dirs: Vec<PathBuf>,
    allowed_dirs: Vec<PathBuf>,
}

impl SimpleIndexerConfig {
    /// Builds a configuration using VT Code's legacy layout as defaults.
    fn new(workspace_root: PathBuf) -> Self {
        let index_dir = workspace_root.join(".vtcode").join("index");
        let vtcode_dir = workspace_root.join(".vtcode");
        let external_dir = vtcode_dir.join("external");

        let mut excluded_dirs: Vec<PathBuf> = vtcode_commons::exclusions::DEFAULT_EXCLUDED_DIRS
            .iter()
            .map(|name| workspace_root.join(name))
            .collect();
        excluded_dirs.push(index_dir.clone());
        excluded_dirs.push(vtcode_dir);

        excluded_dirs.dedup();

        Self {
            workspace_root,
            index_dir,
            ignore_hidden: true,
            excluded_dirs,
            allowed_dirs: vec![external_dir],
        }
    }

    /// Updates the index directory used for persisted metadata.
    pub fn with_index_dir(mut self, index_dir: impl Into<PathBuf>) -> Self {
        let index_dir = index_dir.into();
        self.index_dir = index_dir.clone();
        self.push_unique_excluded(index_dir);
        self
    }

    /// Adds an allowed directory that should be indexed even if hidden or inside an excluded parent.
    pub fn add_allowed_dir(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if !self.allowed_dirs.iter().any(|existing| existing == &path) {
            self.allowed_dirs.push(path);
        }
        self
    }

    /// Adds an additional excluded directory to skip during traversal.
    pub fn add_excluded_dir(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.push_unique_excluded(path);
        self
    }

    /// Toggles whether hidden directories (prefix `.`) are ignored.
    fn ignore_hidden(mut self, ignore_hidden: bool) -> Self {
        self.ignore_hidden = ignore_hidden;
        self
    }

    /// Workspace root accessor.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Index directory accessor.
    fn index_dir(&self) -> &Path {
        &self.index_dir
    }

    fn push_unique_excluded(&mut self, path: PathBuf) {
        if !self.excluded_dirs.iter().any(|existing| existing == &path) {
            self.excluded_dirs.push(path);
        }
    }
}

/// Simple file index entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    /// File path.
    path: String,
    /// File content hash for change detection.
    hash: String,
    /// Last modified timestamp.
    modified: u64,
    /// File size.
    size: u64,
    /// Language/extension.
    language: String,
    /// Simple tags.
    tags: Vec<String>,
}

impl FileIndex {
    /// Crate-internal constructor so the snapshot parser (a sibling module)
    /// can rebuild entries from disk; fields stay private to this module.
    pub(crate) fn from_parts(
        path: String,
        hash: String,
        modified: u64,
        size: u64,
        language: String,
        tags: Vec<String>,
    ) -> Self {
        Self { path, hash, modified, size, language, tags }
    }
}

/// Simple search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    file_path: String,
    line_number: usize,
    line_content: String,
    matches: Vec<String>,
}

/// Simple file indexer.
pub struct SimpleIndexer {
    config: SimpleIndexerConfig,
    index_cache: HashMap<String, FileIndex>,
    content_cache: HashMap<String, (String, Arc<str>)>,
    storage: Arc<dyn IndexStorage>,
    filter: Arc<dyn TraversalFilter>,
}

impl SimpleIndexer {
    /// Create a new simple indexer with default VT Code paths.
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_components(
            SimpleIndexerConfig::new(workspace_root),
            Arc::new(MarkdownIndexStorage),
            Arc::new(ConfigTraversalFilter),
        )
    }

    /// Create a simple indexer with the provided configuration.
    fn with_config(config: SimpleIndexerConfig) -> Self {
        Self::with_components(config, Arc::new(MarkdownIndexStorage), Arc::new(ConfigTraversalFilter))
    }

    /// Create a new simple indexer using a custom index directory.
    pub fn with_index_dir(workspace_root: PathBuf, index_dir: PathBuf) -> Self {
        let config = SimpleIndexerConfig::new(workspace_root).with_index_dir(index_dir);
        Self::with_config(config)
    }

    /// Create an indexer with explicit storage and traversal filter implementations.
    fn with_components(
        config: SimpleIndexerConfig,
        storage: Arc<dyn IndexStorage>,
        filter: Arc<dyn TraversalFilter>,
    ) -> Self {
        Self {
            config,
            index_cache: HashMap::new(),
            content_cache: HashMap::new(),
            storage,
            filter,
        }
    }

    /// Replace the storage backend used to persist index entries.
    fn with_storage(self, storage: Arc<dyn IndexStorage>) -> Self {
        Self { storage, ..self }
    }

    /// Replace the traversal filter used to decide which files and directories are indexed.
    fn with_filter(self, filter: Arc<dyn TraversalFilter>) -> Self {
        Self { filter, ..self }
    }

    /// Initialize the index directory.
    pub fn init(&self) -> Result<()> {
        self.storage.init(self.config.index_dir())
    }

    /// Populate the index cache from the persisted snapshot so re-index runs
    /// can skip unchanged files (see [`Self::index_directory`]). Entries
    /// outside the workspace root, in excluded directories, with hidden
    /// or sensitive names, or with `..` segments are discarded so a tampered
    /// snapshot can never surface files the indexer would refuse to index.
    /// Existence is deliberately unchecked: the re-index diff needs deleted
    /// files' entries to prune them. A missing or malformed snapshot leaves
    /// the cache empty and never fails. Returns the hydrated count.
    pub fn hydrate_from_snapshot(&mut self) -> usize {
        let root = self.config.workspace_root().to_path_buf();
        let mut hydrated = 0;
        for entry in snapshot::load_snapshot_entries(self.config.index_dir()) {
            let path = Path::new(&entry.path);
            if path.starts_with(&root)
                && !snapshot::has_parent_reference(path)
                && (self.is_allowed_path(path) || !self.is_excluded_path(path))
                && snapshot::has_indexable_file_name(path, &self.config)
            {
                self.index_cache.insert(entry.path.clone(), entry);
                hydrated += 1;
            }
        }
        tracing::debug!(hydrated, total = self.index_cache.len(), "index cache hydrated from snapshot");
        hydrated
    }

    /// Get the workspace root path.
    pub fn workspace_root(&self) -> &Path {
        self.config.workspace_root()
    }

    /// Get the index directory used for persisted metadata.
    pub fn index_dir(&self) -> &Path {
        self.config.index_dir()
    }

    /// Index a single file.
    fn index_file(&mut self, file_path: &Path) -> Result<()> {
        let cache_key = file_path.to_string_lossy().into_owned();

        if self.storage.prefers_snapshot_persistence() {
            let next_entry = if file_path.exists() && self.should_process_file_path(file_path) {
                self.build_file_index(file_path)?
            } else {
                None
            };

            self.apply_snapshot_file_update(cache_key, next_entry)?;
            return Ok(());
        }

        if !file_path.exists() || !self.should_process_file_path(file_path) {
            self.index_cache.remove(cache_key.as_str());
            self.storage.remove(self.config.index_dir(), file_path)?;
            return Ok(());
        }

        if let Some(index) = self.build_file_index(file_path)? {
            self.storage.persist(self.config.index_dir(), &index)?;
            self.index_cache.insert(index.path.clone(), index);
        } else {
            self.index_cache.remove(cache_key.as_str());
            self.storage.remove(self.config.index_dir(), file_path)?;
        }

        Ok(())
    }

    /// Index all files in directory recursively.
    /// Respects .gitignore, .ignore, and other ignore files.
    /// SECURITY: Always skips hidden files and sensitive data (.env, .git, etc.)
    ///
    /// Hydrated entries matching size and mtime are served without a read or
    /// hash (see `cached_or_built_index`); an unchanged directory also skips
    /// the whole-snapshot rewrite.
    pub fn index_directory(&mut self, dir_path: &Path) -> Result<()> {
        let walker = self.build_walker(dir_path);

        let mut entries = Vec::new();
        let mut rebuilt_any = false;

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Only index files, not directories
            if entry.file_type().is_some_and(|ft| ft.is_file())
                && let Some(index) = self.cached_or_built_index(path, &mut rebuilt_any)?
            {
                entries.push(index);
            }
        }

        if self.storage.prefers_snapshot_persistence() {
            if rebuilt_any || self.cached_subdirectory_differs(dir_path, &entries) {
                self.apply_snapshot_directory_update(dir_path, &entries)?;
            }
            self.prune_orphaned_content(dir_path);
            return Ok(());
        }

        entries.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        self.storage.persist_batch(self.config.index_dir(), &entries)?;
        self.replace_cached_entries(dir_path, &entries);

        Ok(())
    }

    /// Serve a cached [`FileIndex`] when size and mtime still match the
    /// hydrated entry, avoiding a full read + SHA-256 per unchanged file;
    /// otherwise build. Sets `rebuilt_any` when a file is built so the caller
    /// can skip no-op snapshot rewrites (entry-set drops are caught by the
    /// caller's directory diff). `modified` is second-granularity: a
    /// same-second, same-size rewrite is picked up by the next run.
    fn cached_or_built_index(&mut self, file_path: &Path, rebuilt_any: &mut bool) -> Result<Option<FileIndex>> {
        // Stat once and reuse the metadata for the unchanged screen, the
        // traversal filter, and the rebuilt entry instead of stat-ing up to
        // four times per file (filter, screen, filter, mtime).
        let metadata = match fs::metadata(file_path) {
            Ok(metadata) => metadata,
            Err(_) => {
                // Missing/unreadable: preserve `build_file_index` semantics,
                // which maps missing files to `None` unless a custom filter
                // claims the path (in which case the read error propagates).
                let built = self.build_file_index(file_path)?;
                *rebuilt_any |= built.is_some();
                return Ok(built);
            }
        };

        let cache_key = file_path.to_string_lossy();
        if let Some(cached) = self.index_cache.get(cache_key.as_ref())
            && self.should_process_file_path_with_metadata(file_path, &metadata)
            && let Ok(modified) = metadata.modified()
            && let Ok(elapsed) = modified.duration_since(SystemTime::UNIX_EPOCH)
            && elapsed.as_secs() == cached.modified
            && metadata.len() == cached.size
        {
            return Ok(Some(cached.clone()));
        }

        let built = self.build_file_index_with_metadata(file_path, &metadata)?;
        *rebuilt_any |= built.is_some();
        Ok(built)
    }

    /// Whether the walked entries differ from the cached entries under
    /// `dir_path`. Deletions show up here; additions and content changes set
    /// `rebuilt_any` in [`Self::cached_or_built_index`] instead.
    ///
    /// Compares by count plus membership instead of materializing two `HashSet`s.
    /// Requires: `entries` holds at most one entry per path, as produced by the
    /// [`Self::index_directory`] walker loop — equal counts plus every walked path
    /// present in the cache then means the sets match.
    /// Uniqueness precondition shared by [`Self::cached_subdirectory_differs`].
    #[cfg(debug_assertions)]
    fn walked_paths_are_unique(entries: &[FileIndex]) -> bool {
        let mut seen = std::collections::HashSet::new();
        entries.iter().all(|entry| seen.insert(entry.path.as_str()))
    }

    #[must_use]
    fn cached_subdirectory_differs(&self, dir_path: &Path, entries: &[FileIndex]) -> bool {
        debug_assert!(
            Self::walked_paths_are_unique(entries),
            "cached_subdirectory_differs requires at most one entry per path"
        );
        let cached_count = self
            .index_cache
            .keys()
            .filter(|path| Path::new(path).starts_with(dir_path))
            .count();
        if cached_count != entries.len() {
            return true;
        }
        entries.iter().any(|entry| !self.index_cache.contains_key(entry.path.as_str()))
    }

    /// Discover all files in directory recursively without indexing them.
    /// This is much faster than `index_directory` as it avoids hashing and persistence.
    pub fn discover_files(&self, dir_path: &Path) -> Vec<String> {
        let walker = self.build_walker(dir_path);

        let mut files = walker
            .filter_map(|e| e.ok())
            .filter(|e| {
                if !e.file_type().is_some_and(|ft| ft.is_file()) {
                    return false;
                }

                self.should_process_file_path(e.path())
            })
            .map(|e| e.path().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        files.sort_unstable();
        files
    }

    /// List the immediate children of `dir_path` (one level deep) without
    /// recursing into subdirectories.
    ///
    /// Uses the same ignore/hidden/excluded-directory rules as [`Self::discover_files`]
    /// (via the shared traversal filter), so expensive subtrees such as
    /// `node_modules`, `.git`, and build directories are never listed. This lets
    /// callers build a directory navigator that only touches the directories the
    /// user actually opens, instead of walking the entire workspace up front.
    ///
    /// Returns `(path, is_dir)` pairs with directories sorted before files.
    pub fn discover_dir_entries(&self, dir_path: &Path) -> Vec<(PathBuf, bool)> {
        let walker = self.build_shallow_walker(dir_path);

        let mut entries: Vec<(PathBuf, bool)> = walker
            .filter_map(|e| e.ok())
            .filter(|e| e.path() != dir_path)
            .map(|e| {
                let path = e.path().to_path_buf();
                let is_dir = e.file_type().is_some_and(|ft| ft.is_dir());
                (path, is_dir)
            })
            .filter(|(path, is_dir)| {
                if *is_dir {
                    !should_skip_dir(path, &self.config)
                } else {
                    self.should_process_file_path(path)
                }
            })
            .collect();

        entries.sort_by_cached_key(|(path, is_dir)| {
            // Cache the lowercased path once per entry: a plain `sort_by` would
            // allocate two temporary strings on every one of the ~N·log N
            // comparisons, and this runs on the interactive file-palette path.
            (std::cmp::Reverse(*is_dir), path.to_string_lossy().to_lowercase())
        });
        entries
    }

    /// Internal helper for regex-based file content search.
    /// Used by both `search()` and `grep()` to avoid code duplication.
    fn search_files_internal(
        &self,
        regex: &Regex,
        path_filter: Option<&str>,
        extract_matches: bool,
    ) -> Vec<SearchResult> {
        const PARALLEL_THRESHOLD: usize = 64;

        let candidate_paths: Vec<&String> = self
            .index_cache
            .keys()
            .filter(|file_path| path_filter.is_none_or(|filter| file_path.contains(filter)))
            .collect();

        let mut content_lookup: HashMap<&String, Arc<str>> = HashMap::with_capacity(candidate_paths.len());
        for path in &candidate_paths {
            if let Some(index) = self.index_cache.get(*path)
                && let Some((hash, content)) = self.content_cache.get(*path)
                && hash == &index.hash
            {
                content_lookup.insert(*path, content.clone());
            }
        }

        let map_file = move |file_path: &&String| -> Vec<SearchResult> {
            if let Some(cached) = content_lookup.get(file_path) {
                return Self::search_content(cached.as_ref(), regex, file_path, extract_matches);
            }
            let text = match fs::read_to_string(file_path) {
                Ok(text) => text,
                Err(_) => return Vec::new(),
            };
            Self::search_content(&text, regex, file_path, extract_matches)
        };

        let mut results: Vec<SearchResult> = if candidate_paths.len() <= PARALLEL_THRESHOLD {
            candidate_paths.iter().flat_map(map_file).collect()
        } else {
            candidate_paths.par_iter().flat_map(map_file).collect()
        };

        results.sort_unstable_by(|left, right| {
            left.file_path
                .cmp(&right.file_path)
                .then_with(|| left.line_number.cmp(&right.line_number))
        });
        results
    }

    #[inline]
    fn search_content(content: &str, regex: &Regex, file_path: &&String, extract_matches: bool) -> Vec<SearchResult> {
        let mut local = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            if regex.is_match(line) {
                let line_content = line.to_string();
                let matches = if extract_matches {
                    regex.find_iter(line).map(|m| m.as_str().to_string()).collect()
                } else {
                    vec![line_content.clone()]
                };

                local.push(SearchResult {
                    file_path: (*file_path).clone(),
                    line_number: line_num + 1,
                    line_content,
                    matches,
                });
            }
        }
        local
    }

    /// Search files using regex pattern.
    pub fn search(&self, pattern: &str, path_filter: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;
        Ok(self.search_files_internal(&regex, path_filter, true))
    }

    /// Find files by name pattern.
    fn find_files(&self, pattern: &str) -> Result<Vec<String>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::with_capacity(self.index_cache.len());

        for file_path in self.index_cache.keys() {
            if regex.is_match(file_path) {
                results.push(file_path.clone());
            }
        }

        results.sort_unstable();
        Ok(results)
    }

    /// Get all indexed files without pattern matching.
    /// This is more efficient than using find_files(".*").
    fn all_files(&self) -> Vec<String> {
        let mut files = self.index_cache.keys().cloned().collect::<Vec<_>>();
        files.sort_unstable();
        files
    }

    /// Get file content with line numbers.
    fn get_file_content(&self, file_path: &str, start_line: Option<usize>, end_line: Option<usize>) -> Result<String> {
        let content = fs::read_to_string(file_path)?;
        let start = start_line.unwrap_or(1).max(1);
        let end = end_line.unwrap_or(usize::MAX);

        if start > end {
            return Ok(String::new());
        }

        let mut result = String::with_capacity(content.len());
        for (line_number, line) in content.lines().enumerate() {
            let line_number = line_number + 1;
            if line_number < start {
                continue;
            }
            if line_number > end {
                break;
            }
            writeln!(&mut result, "{line_number}: {line}")?;
        }

        Ok(result)
    }

    /// List files in directory (like ls).
    pub fn list_files(&self, dir_path: &str, show_hidden: bool) -> Result<Vec<String>> {
        let path = Path::new(dir_path);
        if !path.exists() {
            return Ok(vec![]);
        }

        let mut files = Vec::new();

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().into_owned();

            if !show_hidden && file_name.starts_with('.') {
                continue;
            }

            files.push(file_name);
        }

        files.sort_unstable();
        Ok(files)
    }

    /// Grep-like search (like grep command).
    pub fn grep(&self, pattern: &str, file_pattern: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;
        Ok(self.search_files_internal(&regex, file_pattern, false))
    }

    fn is_allowed_path(&self, path: &Path) -> bool {
        self.config.allowed_dirs.iter().any(|allowed| path.starts_with(allowed))
    }

    #[inline]
    fn get_modified_time(&self, file_path: &Path) -> Result<u64> {
        metadata_modified_secs(&fs::metadata(file_path)?)
    }

    #[inline]
    fn detect_language(&self, file_path: &Path) -> String {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    fn build_file_index(&mut self, file_path: &Path) -> Result<Option<FileIndex>> {
        if !self.should_process_file_path(file_path) {
            return Ok(None);
        }

        let content = match fs::read_to_string(file_path) {
            Ok(text) => text,
            Err(err) => {
                if err.kind() == ErrorKind::InvalidData {
                    return Ok(None);
                }
                return Err(err.into());
            }
        };

        let modified = self.get_modified_time(file_path)?;
        Ok(Some(self.finish_file_index(file_path, content, modified)))
    }

    /// [`Self::build_file_index`] reusing an already-observed `metadata`.
    ///
    /// Used by the directory-walk hot loop so each file is stat-ed once.
    /// A file that vanishes between the stat and the read maps to `None`,
    /// matching the legacy path's existence re-check. (A custom filter that
    /// accepts nonexistent paths would have surfaced the read error in the
    /// legacy path; here the transient delete is skipped instead.)
    fn build_file_index_with_metadata(
        &mut self,
        file_path: &Path,
        metadata: &fs::Metadata,
    ) -> Result<Option<FileIndex>> {
        if !self.should_process_file_path_with_metadata(file_path, metadata) {
            return Ok(None);
        }

        let content = match fs::read_to_string(file_path) {
            Ok(text) => text,
            Err(err) => {
                if err.kind() == ErrorKind::InvalidData || err.kind() == ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(err.into());
            }
        };

        let modified = metadata_modified_secs(metadata)?;
        Ok(Some(self.finish_file_index(file_path, content, modified)))
    }

    /// Assemble a [`FileIndex`] from already-read content and mtime, and cache
    /// the content for hash-verified searches.
    fn finish_file_index(&mut self, file_path: &Path, content: String, modified: u64) -> FileIndex {
        let index = FileIndex {
            path: file_path.to_string_lossy().into_owned(),
            hash: calculate_hash(&content),
            modified,
            size: content.len() as u64,
            language: self.detect_language(file_path),
            tags: vec![],
        };

        self.content_cache
            .insert(index.path.clone(), (index.hash.clone(), Arc::from(content)));

        index
    }

    #[inline]
    fn is_excluded_path(&self, path: &Path) -> bool {
        self.config.excluded_dirs.iter().any(|excluded| path.starts_with(excluded))
    }

    #[inline]
    fn should_index_file_path(&self, path: &Path) -> bool {
        self.filter.should_index_file(path, &self.config)
    }

    #[inline]
    fn should_process_file_path(&self, path: &Path) -> bool {
        self.should_process_with(path, || self.should_index_file_path(path))
    }

    #[inline]
    fn should_process_file_path_with_metadata(&self, path: &Path, metadata: &fs::Metadata) -> bool {
        self.should_process_with(path, || self.filter.should_index_file_with_metadata(path, metadata, &self.config))
    }

    /// Allowed/excluded scope gate shared by both file filters.
    ///
    /// Allowed directories bypass exclusion; everywhere else an excluded path
    /// short-circuits before the (potentially stat-ing) filter runs.
    #[inline]
    fn should_process_with(&self, path: &Path, indexable: impl FnOnce() -> bool) -> bool {
        if self.is_allowed_path(path) {
            return indexable();
        }

        !self.is_excluded_path(path) && indexable()
    }

    fn build_walker(&self, dir_path: &Path) -> Walk {
        let walk_root = dir_path.to_path_buf();
        let config = self.config.clone();
        let filter = Arc::clone(&self.filter);

        let mut builder = vtcode_commons::walk::build_default_walker(dir_path);
        builder.filter_entry(move |entry| should_visit_entry(entry, walk_root.as_path(), &config, filter.as_ref()));
        builder.build()
    }

    fn build_shallow_walker(&self, dir_path: &Path) -> Walk {
        let walk_root = dir_path.to_path_buf();
        let config = self.config.clone();
        let filter = Arc::clone(&self.filter);

        let mut builder = vtcode_commons::walk::build_default_walker(dir_path);
        // Only immediate children — directory navigation lists one level at a time.
        builder.max_depth(Some(1));
        builder.filter_entry(move |entry| should_visit_entry(entry, walk_root.as_path(), &config, filter.as_ref()));
        builder.build()
    }

    fn replace_cached_entries(&mut self, dir_path: &Path, entries: &[FileIndex]) {
        self.index_cache.retain(|path, _| !Path::new(path).starts_with(dir_path));
        self.content_cache.retain(|path, _| !Path::new(path).starts_with(dir_path));

        self.index_cache
            .extend(entries.iter().cloned().map(|entry| (entry.path.clone(), entry)));
    }

    /// Drop content entries under `dir_path` without an index entry (deleted,
    /// rejected, or unindexable); content for still-indexed files is kept so
    /// searches avoid a disk read.
    fn prune_orphaned_content(&mut self, dir_path: &Path) {
        self.content_cache
            .retain(|path, _| !Path::new(path).starts_with(dir_path) || self.index_cache.contains_key(path.as_str()));
    }

    fn apply_snapshot_file_update(&mut self, cache_key: String, next_entry: Option<FileIndex>) -> Result<()> {
        let previous_entry = match next_entry {
            Some(entry) => self.index_cache.insert(cache_key.clone(), entry),
            None => self.index_cache.remove(cache_key.as_str()),
        };
        let previous_content = self.content_cache.remove(cache_key.as_str());

        if let Err(err) = self.persist_current_snapshot() {
            match previous_entry {
                Some(entry) => {
                    self.index_cache.insert(cache_key.clone(), entry);
                }
                None => {
                    self.index_cache.remove(cache_key.as_str());
                }
            }
            if let Some(content) = previous_content {
                self.content_cache.insert(cache_key, content);
            }
            return Err(err);
        }

        Ok(())
    }

    fn apply_snapshot_directory_update(&mut self, dir_path: &Path, entries: &[FileIndex]) -> Result<()> {
        let previous_entries = self.take_cached_entries(dir_path);
        self.index_cache
            .extend(entries.iter().cloned().map(|entry| (entry.path.clone(), entry)));

        if let Err(err) = self.persist_current_snapshot() {
            self.index_cache.retain(|path, _| !Path::new(path).starts_with(dir_path));
            self.index_cache
                .extend(previous_entries.into_iter().map(|entry| (entry.path.clone(), entry)));
            return Err(err);
        }

        Ok(())
    }

    fn take_cached_entries(&mut self, dir_path: &Path) -> Vec<FileIndex> {
        let keys = self
            .index_cache
            .keys()
            .filter(|path| Path::new(path).starts_with(dir_path))
            .cloned()
            .collect::<Vec<_>>();

        keys.into_iter()
            .filter_map(|path| self.index_cache.remove(path.as_str()))
            .collect()
    }

    fn persist_current_snapshot(&self) -> Result<()> {
        let mut snapshot = self.index_cache.values().collect::<Vec<_>>();
        snapshot.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        self.storage.persist_batch_refs(self.config.index_dir(), &snapshot)
    }
}

impl Clone for SimpleIndexer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            index_cache: self.index_cache.clone(),
            content_cache: self.content_cache.clone(),
            storage: self.storage.clone(),
            filter: self.filter.clone(),
        }
    }
}

fn should_skip_dir(path: &Path, config: &SimpleIndexerConfig) -> bool {
    if is_allowed_path_or_ancestor(path, config) {
        return false;
    }

    if config.excluded_dirs.iter().any(|excluded| path.starts_with(excluded)) {
        return true;
    }

    if config.ignore_hidden
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name_str| name_str.starts_with('.'))
    {
        return true;
    }

    false
}

fn is_allowed_path_or_ancestor(path: &Path, config: &SimpleIndexerConfig) -> bool {
    config
        .allowed_dirs
        .iter()
        .any(|allowed| path.starts_with(allowed) || allowed.starts_with(path))
}

fn should_visit_entry(
    entry: &DirEntry,
    walk_root: &Path,
    config: &SimpleIndexerConfig,
    filter: &dyn TraversalFilter,
) -> bool {
    if entry.path() == walk_root {
        return true;
    }

    if !entry.file_type().is_some_and(|file_type| file_type.is_dir()) {
        return true;
    }

    filter.should_descend(entry.path(), config)
}

#[inline]
fn calculate_hash(content: &str) -> String {
    vtcode_commons::utils::calculate_sha256(content.as_bytes())
}

/// Second-granularity mtime shared by the index builder and the unchanged
/// screen so both observe the same clock reading for one stat.
#[inline]
fn metadata_modified_secs(metadata: &fs::Metadata) -> Result<u64> {
    Ok(metadata.modified()?.duration_since(SystemTime::UNIX_EPOCH)?.as_secs())
}

#[cfg(test)]
mod tests;
