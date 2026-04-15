//! Fast fuzzy file search library for VT Code.
//!
//! Uses the `ignore` crate (same as ripgrep) for parallel directory traversal
//! and `nucleo-matcher` for fuzzy matching.
//!
//! # Example
//!
//! ```ignore
//! use std::num::NonZero;
//! use std::path::Path;
//! use std::sync::Arc;
//! use std::sync::atomic::AtomicBool;
//! use vtcode_file_search::run;
//!
//! let results = run(
//!     "main",
//!     NonZero::new(100).unwrap(),
//!     Path::new("."),
//!     vec![],
//!     NonZero::new(4).unwrap(),
//!     Arc::new(AtomicBool::new(false)),
//!     false,
//!     true,
//! )?;
//!
//! for m in results.matches {
//!     println!("{}: {}", m.path, m.score);
//! }
//! # Ok::<(), anyhow::Error>(())
//! ```

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::RwLock;

/// Pre-computed file index for instant queries.
///
/// This index is built in the background and cached to avoid
/// repeated directory traversals on every search.
pub struct FileIndex {
    /// All file paths in the workspace
    files: Vec<String>,
    /// All directory paths in the workspace
    directories: Vec<String>,
    /// When this index was last built
    last_built: std::time::Instant,
}

/// Build a parallel walker with the given configuration.
fn build_parallel_walker(
    search_directory: &Path,
    exclude: &[String],
    threads: usize,
    respect_gitignore: bool,
) -> anyhow::Result<ignore::WalkParallel> {
    let mut walk_builder = ignore::WalkBuilder::new(search_directory);
    walk_builder
        .threads(threads)
        .hidden(false)
        .follow_links(true)
        .require_git(false);

    if !respect_gitignore {
        walk_builder
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false);
    }

    if !exclude.is_empty() {
        let mut override_builder = ignore::overrides::OverrideBuilder::new(search_directory);
        for exclude_pattern in exclude {
            let pattern = format!("!{}", exclude_pattern);
            override_builder.add(&pattern)?;
        }
        walk_builder.overrides(override_builder.build()?);
    }

    Ok(walk_builder.build_parallel())
}

impl FileIndex {
    /// Build a file index by traversing the directory tree.
    /// This is expensive but only done once.
    fn build_from_directory(
        search_directory: &Path,
        exclude: &[String],
        respect_gitignore: bool,
        threads: usize,
    ) -> anyhow::Result<Self> {
        let walker = build_parallel_walker(search_directory, exclude, threads, respect_gitignore)?;

        // Collect all files and directories
        let files_arc = Arc::new(Mutex::new(Vec::new()));
        let dirs_arc = Arc::new(Mutex::new(Vec::new()));

        walker.run(|| {
            let files_clone = files_arc.clone();
            let dirs_clone = dirs_arc.clone();
            let search_dir = search_directory.to_path_buf();

            Box::new(move |result| {
                let entry = match result {
                    Ok(e) => e,
                    Err(_) => return ignore::WalkState::Continue,
                };

                // Make path relative to search directory
                if let Some(rel_path) = entry
                    .path()
                    .strip_prefix(&search_dir)
                    .ok()
                    .and_then(|p| p.to_str())
                    && !rel_path.is_empty()
                {
                    if entry.path().is_dir() {
                        dirs_clone.lock().push(rel_path.to_string());
                    } else {
                        files_clone.lock().push(rel_path.to_string());
                    }
                }

                ignore::WalkState::Continue
            })
        });

        let files = Arc::try_unwrap(files_arc)
            .map_err(|arc| {
                anyhow::anyhow!(
                    "failed to unwrap files arc, {} references remain",
                    Arc::strong_count(&arc)
                )
            })?
            .into_inner();
        let directories = Arc::try_unwrap(dirs_arc)
            .map_err(|arc| {
                anyhow::anyhow!(
                    "failed to unwrap dirs arc, {} references remain",
                    Arc::strong_count(&arc)
                )
            })?
            .into_inner();

        Ok(Self {
            files,
            directories,
            last_built: std::time::Instant::now(),
        })
    }

    /// Query the index for matching paths.
    /// Much faster than re-traversing the filesystem.
    fn query(
        &self,
        pattern_text: &str,
        limit: usize,
        match_type_filter: Option<MatchType>,
    ) -> Vec<(u32, String, MatchType)> {
        let mut results = BinaryHeap::with_capacity(limit);

        // Normalize pattern to lowercase to work around a nucleo-matcher bug:
        // its prefilter only does case-insensitive search for lowercase needle
        // chars, not uppercase. See https://github.com/openai/codex/pull/15772.
        let pattern_storage = if pattern_text.is_ascii() {
            PatternStorage::Ascii(pattern_text.to_ascii_lowercase().into_bytes())
        } else {
            PatternStorage::Unicode(pattern_text.to_lowercase().chars().collect())
        };

        // Reuse single matcher across all queries (mem-reuse-collections)
        let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
        let mut haystack_buf = Vec::with_capacity(256);

        // Iterate over files
        if match_type_filter.is_none_or(|t| t == MatchType::File) {
            for path in &self.files {
                if let Some(score) =
                    self.score_path(path, &pattern_storage, &mut matcher, &mut haystack_buf)
                {
                    push_top_match(&mut results, limit, score, path.clone(), MatchType::File);
                }
            }
        }

        // Iterate over directories
        if match_type_filter.is_none_or(|t| t == MatchType::Directory) {
            for path in &self.directories {
                if let Some(score) =
                    self.score_path(path, &pattern_storage, &mut matcher, &mut haystack_buf)
                {
                    push_top_match(
                        &mut results,
                        limit,
                        score,
                        path.clone(),
                        MatchType::Directory,
                    );
                }
            }
        }

        results
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse(item)| item)
            .collect()
    }

    fn score_path(
        &self,
        path: &str,
        pattern: &PatternStorage,
        matcher: &mut nucleo_matcher::Matcher,
        haystack_buf: &mut Vec<char>,
    ) -> Option<u32> {
        let haystack = nucleo_matcher::Utf32Str::new(path, haystack_buf);

        let needle = match pattern {
            PatternStorage::Ascii(bytes) => nucleo_matcher::Utf32Str::Ascii(bytes),
            PatternStorage::Unicode(chars) => nucleo_matcher::Utf32Str::Unicode(chars),
        };

        matcher.fuzzy_match(haystack, needle).map(|s| s as u32)
    }
}

/// A cached file index that can be shared across searches.
pub struct FileIndexCache {
    cache: Arc<RwLock<Option<Arc<FileIndex>>>>,
    search_directory: std::path::PathBuf,
    exclude: Vec<String>,
    respect_gitignore: bool,
    threads: usize,
}

impl FileIndexCache {
    pub fn new(
        search_directory: std::path::PathBuf,
        exclude: impl IntoIterator<Item = String>,
        respect_gitignore: bool,
        threads: usize,
    ) -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
            search_directory,
            exclude: exclude.into_iter().collect(),
            respect_gitignore,
            threads,
        }
    }

    /// Get or build the file index.
    pub async fn get_or_build(&self) -> anyhow::Result<Arc<FileIndex>> {
        // Check if we have a cached index
        {
            let guard = self.cache.read().await;
            if let Some(index) = guard.as_ref() {
                // Check if index is stale (older than 5 minutes)
                if index.last_built.elapsed() < std::time::Duration::from_secs(300) {
                    return Ok(Arc::clone(index));
                }
            }
        }

        // Build a new index
        let index = Arc::new(FileIndex::build_from_directory(
            &self.search_directory,
            &self.exclude,
            self.respect_gitignore,
            self.threads,
        )?);

        // Cache and return
        {
            let mut guard = self.cache.write().await;
            *guard = Some(Arc::clone(&index));
        }
        Ok(index)
    }

    /// Force refresh the index in the background.
    /// Returns the old index immediately while rebuilding happens asynchronously.
    pub fn refresh_background(&self) -> Option<Arc<FileIndex>> {
        // Build new index asynchronously
        let search_directory = self.search_directory.clone();
        let exclude = self.exclude.clone();
        let respect_gitignore = self.respect_gitignore;
        let threads = self.threads;
        let cache = self.cache.clone();

        tokio::spawn(async move {
            match FileIndex::build_from_directory(
                &search_directory,
                &exclude,
                respect_gitignore,
                threads,
            ) {
                Ok(new_index) => {
                    let mut guard = cache.write().await;
                    *guard = Some(Arc::new(new_index));
                }
                Err(e) => {
                    tracing::error!("failed to rebuild file index: {e}");
                }
            }
        });

        // Return old index if available
        let guard = self.cache.blocking_read();
        guard.as_ref().map(Arc::clone)
    }

    /// Incrementally update the index when a file change is detected.
    /// This is faster than a full rebuild for single file changes.
    pub fn update_file(&self, path: &str, is_added: bool) {
        let mut guard = self.cache.blocking_write();
        let Some(existing) = guard.take() else { return };

        let mut index = Arc::try_unwrap(existing).unwrap_or_else(|arc| (*arc).clone());
        if is_added {
            if Path::new(path).is_dir() {
                index.directories.push(path.to_string());
            } else {
                index.files.push(path.to_string());
            }
        } else {
            index.files.retain(|p| p != path);
            index.directories.retain(|p| p != path);
        }
        index.last_built = std::time::Instant::now();
        *guard = Some(Arc::new(index));
    }

    /// Get the age of the current index.
    pub async fn index_age(&self) -> Option<std::time::Duration> {
        let guard = self.cache.read().await;
        guard.as_ref().map(|idx| idx.last_built.elapsed())
    }
}

// Make FileIndex cloneable
impl Clone for FileIndex {
    fn clone(&self) -> Self {
        Self {
            files: self.files.clone(),
            directories: self.directories.clone(),
            last_built: self.last_built,
        }
    }
}

/// A single file match result.
///
/// Fields:
/// - `score`: Relevance score from fuzzy matching (higher is better)
/// - `path`: Path relative to the search directory
/// - `match_type`: Whether the match is a file or directory
/// - `indices`: Optional character positions for highlighting matched characters
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum MatchType {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMatch {
    pub score: u32,
    pub path: String,
    pub match_type: MatchType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indices: Option<Vec<u32>>,
}

/// Complete search results with total match count.
#[derive(Debug)]
pub struct FileSearchResults {
    pub matches: Vec<FileMatch>,
    pub total_match_count: usize,
}

/// Configuration for file search operations.
pub struct FileSearchConfig {
    pub pattern_text: String,
    pub limit: NonZero<usize>,
    pub search_directory: std::path::PathBuf,
    pub exclude: Vec<String>,
    pub threads: NonZero<usize>,
    pub cancel_flag: Arc<AtomicBool>,
    pub compute_indices: bool,
    pub respect_gitignore: bool,
}

pub use vtcode_commons::paths::file_name_from_path;

/// Best matches list per worker thread (lock-free collection).
///
/// Each worker thread gets its own instance to avoid locking during
/// directory traversal. Results are merged at the end.
struct BestMatchesList {
    matches: BinaryHeap<Reverse<(u32, String, MatchType)>>,
    limit: usize,
    matcher: nucleo_matcher::Matcher,
    haystack_buf: Vec<char>,
    /// Pre-computed pattern - avoids per-match UTF-32 conversion
    pattern: PatternStorage,
}

/// Stores a pattern in the optimal form for Utf32Str creation.
enum PatternStorage {
    /// ASCII pattern - can be used directly with Utf32Str::Ascii
    Ascii(Vec<u8>),
    /// Unicode pattern - stored as chars for Utf32Str::Unicode
    Unicode(Vec<char>),
}

impl BestMatchesList {
    fn new(limit: usize, pattern_text: &str) -> Self {
        // Normalize pattern to lowercase to work around a nucleo-matcher bug:
        // its prefilter only does case-insensitive search for lowercase needle
        // chars, not uppercase. See https://github.com/openai/codex/pull/15772.
        let pattern = if pattern_text.is_ascii() {
            PatternStorage::Ascii(pattern_text.to_ascii_lowercase().into_bytes())
        } else {
            PatternStorage::Unicode(pattern_text.to_lowercase().chars().collect())
        };

        Self {
            matches: BinaryHeap::new(),
            limit,
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            haystack_buf: Vec::with_capacity(256),
            pattern,
        }
    }

    /// Record a matching path while preserving the worker-local top-K heap.
    ///
    /// Returns true when the path matches the search pattern, even if it
    /// does not survive the top-K cutoff.
    fn record_match(&mut self, path: &str, match_type: MatchType) -> bool {
        // Use pre-computed pattern directly - zero allocation per match
        let haystack = nucleo_matcher::Utf32Str::new(path, &mut self.haystack_buf);
        let needle = match &self.pattern {
            PatternStorage::Ascii(bytes) => nucleo_matcher::Utf32Str::Ascii(bytes),
            PatternStorage::Unicode(chars) => nucleo_matcher::Utf32Str::Unicode(chars),
        };
        let Some(score) = self.matcher.fuzzy_match(haystack, needle) else {
            return false;
        };

        push_top_match(
            &mut self.matches,
            self.limit,
            score as u32,
            path.to_string(),
            match_type,
        );
        true
    }
}

fn push_top_match(
    matches: &mut BinaryHeap<Reverse<(u32, String, MatchType)>>,
    limit: usize,
    score: u32,
    path: String,
    match_type: MatchType,
) -> bool {
    if matches.len() < limit {
        matches.push(Reverse((score, path, match_type)));
        return true;
    }

    let Some(min_score) = matches.peek().map(|entry| entry.0.0) else {
        return false;
    };

    if score <= min_score {
        return false;
    }

    matches.pop();
    matches.push(Reverse((score, path, match_type)));
    true
}

/// Run fuzzy file search using a pre-computed file index.
///
/// This is much faster than `run()` for repeated queries on the same
/// directory because it avoids re-traversing the filesystem.
///
/// # Arguments
///
/// * `config` - File search configuration
/// * `index_cache` - Shared cache for the pre-computed file index
///
/// # Returns
///
/// FileSearchResults containing matched files and total match count.
pub async fn run_with_index(
    config: FileSearchConfig,
    index_cache: &FileIndexCache,
) -> anyhow::Result<FileSearchResults> {
    let limit = config.limit.get();
    let cancel_flag = &config.cancel_flag;
    let compute_indices = config.compute_indices;

    // Get or build the file index
    let index = index_cache.get_or_build().await?;

    // Check cancellation
    if cancel_flag.load(Ordering::Relaxed) {
        return Ok(FileSearchResults {
            matches: Vec::new(),
            total_match_count: 0,
        });
    }

    // Query the index
    let matched_paths = index.query(&config.pattern_text, limit, None);
    let total_match_count = matched_paths.len();

    // Build final results
    let matches = matched_paths
        .into_iter()
        .map(|(score, path, match_type)| FileMatch {
            score,
            path,
            match_type,
            indices: if compute_indices {
                Some(Vec::new())
            } else {
                None
            },
        })
        .collect();

    Ok(FileSearchResults {
        matches,
        total_match_count,
    })
}

/// Run fuzzy file search with parallel traversal.
///
/// # Arguments
///
/// * `config` - File search configuration containing all search parameters
///
/// # Returns
///
/// FileSearchResults containing matched files and total match count.
pub fn run(config: FileSearchConfig) -> anyhow::Result<FileSearchResults> {
    let limit = config.limit.get();
    let search_directory = &config.search_directory;
    let exclude = &config.exclude;
    let threads = config.threads.get();
    let cancel_flag = &config.cancel_flag;
    let compute_indices = config.compute_indices;
    let respect_gitignore = config.respect_gitignore;

    let walker = build_parallel_walker(search_directory, exclude, threads, respect_gitignore)?;

    // Create per-worker result collection using Arc + Mutex for thread safety.
    // Each worker gets exactly one instance - no sharing between workers.
    let best_matchers_per_worker: Vec<Arc<Mutex<BestMatchesList>>> = (0..threads)
        .map(|_| {
            Arc::new(Mutex::new(BestMatchesList::new(
                limit,
                &config.pattern_text,
            )))
        })
        .collect();

    let total_match_count = Arc::new(AtomicUsize::new(0));

    // Run parallel traversal - the closure is called once per worker thread.
    // We use a local counter to assign each worker a unique index.
    let worker_counter = AtomicUsize::new(0);
    let worker_count = best_matchers_per_worker.len();
    walker.run(|| {
        let worker_id = worker_counter.fetch_add(1, Ordering::Relaxed) % worker_count;
        let best_list = best_matchers_per_worker[worker_id].clone();
        let cancel_flag_clone = cancel_flag.clone();
        let total_match_count_clone = total_match_count.clone();

        Box::new(move |result| {
            // Check cancellation flag periodically
            if cancel_flag_clone.load(Ordering::Relaxed) {
                return ignore::WalkState::Quit;
            }

            let entry = match result {
                Ok(e) => e,
                Err(_) => return ignore::WalkState::Continue,
            };

            // Make path relative to search directory
            let relative_path = entry
                .path()
                .strip_prefix(search_directory)
                .ok()
                .and_then(|p| p.to_str());

            let path_to_match = match relative_path {
                Some(p) if !p.is_empty() => p,
                _ => return ignore::WalkState::Continue, // Skip root and non-relative paths
            };

            let match_type = if entry.path().is_dir() {
                MatchType::Directory
            } else {
                MatchType::File
            };

            // Try to add to results - no contention with other workers
            {
                let mut list = best_list.lock();
                if list.record_match(path_to_match, match_type) {
                    total_match_count_clone.fetch_add(1, Ordering::Relaxed);
                }
            }

            ignore::WalkState::Continue
        })
    });

    // Merge worker-local top-K heaps into one final top-K heap.
    let mut merged_matches = BinaryHeap::with_capacity(limit);
    for arc in best_matchers_per_worker {
        let mut list = arc.lock();
        for Reverse((score, path, match_type)) in std::mem::take(&mut list.matches).into_vec() {
            push_top_match(&mut merged_matches, limit, score, path, match_type);
        }
    }

    // Build final results
    let matches = merged_matches
        .into_sorted_vec()
        .into_iter()
        .map(|Reverse((score, path, match_type))| FileMatch {
            score,
            path,
            match_type,
            indices: if compute_indices {
                Some(Vec::new())
            } else {
                None
            },
        })
        .collect();

    Ok(FileSearchResults {
        matches,
        total_match_count: total_match_count.load(Ordering::Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_file_name_from_path() {
        assert_eq!(file_name_from_path("src/main.rs"), "main.rs");
        assert_eq!(file_name_from_path("Cargo.toml"), "Cargo.toml");
        assert_eq!(file_name_from_path("/absolute/path/file.txt"), "file.txt");
        assert_eq!(file_name_from_path("file.txt"), "file.txt");
        assert_eq!(file_name_from_path(""), "");
    }

    #[test]
    fn test_run_search() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("hello.rs"), "fn main() {}")?;
        fs::write(temp.path().join("world.txt"), "world")?;

        let results = run(FileSearchConfig {
            pattern_text: "hello".to_string(),
            limit: NonZero::new(10).unwrap(),
            search_directory: temp.path().to_path_buf(),
            exclude: vec![],
            threads: NonZero::new(1).unwrap(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: false,
        })?;

        assert_eq!(results.matches.len(), 1);
        assert!(results.matches[0].path.contains("hello"));
        assert_eq!(results.matches[0].match_type, MatchType::File);

        Ok(())
    }

    #[test]
    fn test_multiple_matches() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("test1.rs"), "")?;
        fs::write(temp.path().join("test2.rs"), "")?;
        fs::write(temp.path().join("test3.rs"), "")?;
        fs::write(temp.path().join("other.txt"), "")?;

        let results = run(FileSearchConfig {
            pattern_text: "test".to_string(),
            limit: NonZero::new(10).unwrap(),
            search_directory: temp.path().to_path_buf(),
            exclude: vec![],
            threads: NonZero::new(2).unwrap(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: false,
        })?;

        assert_eq!(results.matches.len(), 3);
        assert!(results.matches.iter().all(|m| m.path.contains("test")));
        assert!(
            results
                .matches
                .iter()
                .all(|m| matches!(m.match_type, MatchType::File))
        );

        Ok(())
    }

    #[test]
    fn test_limit_is_respected_across_workers() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        for name in ["alpha.rs", "alphabet.rs", "alphanumeric.rs", "alpaca.rs"] {
            fs::write(temp.path().join(name), "")?;
        }

        let results = run(FileSearchConfig {
            pattern_text: "alpha".to_string(),
            limit: NonZero::new(2).unwrap(),
            search_directory: temp.path().to_path_buf(),
            exclude: vec![],
            threads: NonZero::new(4).unwrap(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: false,
        })?;

        assert_eq!(results.matches.len(), 2);
        assert!(
            results
                .matches
                .windows(2)
                .all(|window| window[0].score >= window[1].score)
        );

        Ok(())
    }

    #[test]
    fn test_exclusion_patterns() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("keep.rs"), "")?;
        fs::create_dir(temp.path().join("target"))?;
        fs::write(temp.path().join("target/ignore.rs"), "")?;

        let results = run(FileSearchConfig {
            pattern_text: "rs".to_string(),
            limit: NonZero::new(10).unwrap(),
            search_directory: temp.path().to_path_buf(),
            exclude: vec!["target/**".to_string()],
            threads: NonZero::new(2).unwrap(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: false,
        })?;

        assert_eq!(results.matches.len(), 1);
        assert!(results.matches[0].path.contains("keep.rs"));
        assert_eq!(results.matches[0].match_type, MatchType::File);

        Ok(())
    }

    #[test]
    fn test_cancellation() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        for i in 0..10 {
            fs::write(temp.path().join(format!("file{}.rs", i)), "")?;
        }

        let cancel_flag = Arc::new(AtomicBool::new(true));
        let results = run(FileSearchConfig {
            pattern_text: "file".to_string(),
            limit: NonZero::new(10).unwrap(),
            search_directory: temp.path().to_path_buf(),
            exclude: vec![],
            threads: NonZero::new(1).unwrap(),
            cancel_flag,
            compute_indices: false,
            respect_gitignore: false,
        })?;

        // Should return early due to cancellation
        assert!(results.matches.is_empty());

        Ok(())
    }

    #[test]
    fn test_directory_matches_are_returned() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::create_dir_all(temp.path().join("docs/guides"))?;
        fs::write(temp.path().join("docs/guides/intro.md"), "intro")?;
        fs::write(temp.path().join("docs/readme.md"), "readme")?;

        let results = run(FileSearchConfig {
            pattern_text: "guides".to_string(),
            limit: NonZero::new(10).unwrap(),
            search_directory: temp.path().to_path_buf(),
            exclude: vec![],
            threads: NonZero::new(2).unwrap(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: false,
        })?;

        assert!(results.matches.iter().any(
            |m| m.path.ends_with("docs/guides") && matches!(m.match_type, MatchType::Directory)
        ));

        Ok(())
    }

    #[test]
    fn test_file_index_cache_basic() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("main.rs"), "")?;
        fs::write(temp.path().join("lib.rs"), "")?;
        fs::create_dir(temp.path().join("src"))?;

        let cache = FileIndexCache::new(temp.path().to_path_buf(), vec![], false, 2);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        // First call should build the index
        let index = rt.block_on(cache.get_or_build())?;
        assert_eq!(index.files.len(), 2);
        assert_eq!(index.directories.len(), 1);

        // Second call should return cached index
        let index2 = rt.block_on(cache.get_or_build())?;
        assert_eq!(index2.files.len(), 2);

        Ok(())
    }

    #[test]
    fn test_file_index_incremental_update() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("main.rs"), "")?;

        let cache = FileIndexCache::new(temp.path().to_path_buf(), vec![], false, 1);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let _ = rt.block_on(cache.get_or_build())?;

        // Add a new file
        fs::write(temp.path().join("new.rs"), "")?;
        let new_path = temp.path().join("new.rs").to_string_lossy().to_string();
        cache.update_file(&new_path, true);

        // Verify index was updated
        let index = rt.block_on(cache.get_or_build())?;
        assert!(index.files.iter().any(|p| p.contains("new.rs")));

        Ok(())
    }

    #[test]
    fn test_file_index_query() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("hello_world.rs"), "")?;
        fs::write(temp.path().join("hello_test.rs"), "")?;
        fs::write(temp.path().join("other.txt"), "")?;

        let cache = FileIndexCache::new(temp.path().to_path_buf(), vec![], false, 1);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let index = rt.block_on(cache.get_or_build())?;

        // Query for "hello" should match both hello files
        let results = index.query("hello", 10, None);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, path, _)| path.contains("hello")));

        // Query with limit
        let results = index.query("hello", 1, None);
        assert_eq!(results.len(), 1);

        // Query for non-existent pattern
        let results = index.query("nonexistent", 10, None);
        assert!(results.is_empty());

        Ok(())
    }

    #[test]
    fn test_run_with_index() -> anyhow::Result<()> {
        let temp = TempDir::new()?;
        fs::write(temp.path().join("main.rs"), "fn main() {}")?;
        fs::write(temp.path().join("lib.rs"), "pub fn lib() {}")?;

        let cache = FileIndexCache::new(temp.path().to_path_buf(), vec![], false, 1);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let config = FileSearchConfig {
            pattern_text: "main".to_string(),
            limit: NonZero::new(10).unwrap(),
            search_directory: temp.path().to_path_buf(),
            exclude: vec![],
            threads: NonZero::new(1).unwrap(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: false,
        };

        let results = rt.block_on(run_with_index(config, &cache))?;
        assert_eq!(results.matches.len(), 1);
        assert!(results.matches[0].path.contains("main.rs"));

        Ok(())
    }
}
