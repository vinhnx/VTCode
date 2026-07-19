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
//! use vtcode_indexer::file_search::run;
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

use rayon::prelude::*;
use vtcode_commons::StringId;

/// Pre-computed file index for instant queries.
///
/// This index is built in the background and cached to avoid
/// repeated directory traversals on every search.
pub struct FileIndex {
    files: Vec<StringId>,
    directories: Vec<StringId>,
    interner: Arc<Mutex<vtcode_commons::StringInterner>>,
    last_built: std::time::Instant,
}

/// Build a parallel walker with the given configuration.
fn build_parallel_walker(
    search_directory: &Path,
    exclude: &[String],
    threads: usize,
    respect_gitignore: bool,
    follow_links: bool,
) -> anyhow::Result<ignore::WalkParallel> {
    let mut walk_builder = ignore::WalkBuilder::new(search_directory);
    vtcode_commons::walk::apply_defaults(&mut walk_builder);

    // File-search-specific overrides
    walk_builder.threads(threads);
    walk_builder.follow_links(follow_links);
    walk_builder.require_git(false); // Search works outside git repos

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
            let pattern = format!("!{exclude_pattern}");
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
        let walker = build_parallel_walker(search_directory, exclude, threads, respect_gitignore, true)?;

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
                if let Some(rel_path) = entry.path().strip_prefix(&search_dir).ok().and_then(|p| p.to_str())
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
                anyhow::anyhow!("failed to unwrap files arc, {} references remain", Arc::strong_count(&arc))
            })?
            .into_inner();
        let directories = Arc::try_unwrap(dirs_arc)
            .map_err(|arc| anyhow::anyhow!("failed to unwrap dirs arc, {} references remain", Arc::strong_count(&arc)))?
            .into_inner();

        let mut interner = vtcode_commons::StringInterner::new();
        let interned_files: Vec<StringId> = files.iter().map(|s| interner.intern(s)).collect();
        let interned_dirs: Vec<StringId> = directories.iter().map(|s| interner.intern(s)).collect();

        Ok(Self {
            files: interned_files,
            directories: interned_dirs,
            interner: Arc::new(Mutex::new(interner)),
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
    ) -> Vec<(u32, StringId, MatchType)> {
        // `query` stays serial and declarative: the parallel scoring strategy
        // is isolated behind `score_paths_top_k`, and the per-chunk top-K heaps
        // are merged by the shared `merge_top_k` helper. This keeps the index
        // query logic testable without a rayon runtime in the loop.
        let mut heaps = Vec::new();

        if match_type_filter.is_none_or(|t| t == MatchType::File) {
            heaps.push(score_paths_top_k(&self.files, &self.interner, limit, pattern_text, MatchType::File));
        }

        if match_type_filter.is_none_or(|t| t == MatchType::Directory) {
            heaps.push(score_paths_top_k(&self.directories, &self.interner, limit, pattern_text, MatchType::Directory));
        }

        merge_top_k(heaps, &self.interner, limit)
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse(item)| item)
            .collect()
    }
}

/// Score `paths` in parallel rayon chunks, returning the worker-merged top-K
/// heap for `match_type`.
///
/// This is the single boundary for the parallel scoring strategy: each worker
/// thread gets its own `BestMatchesList` (matcher + haystack buffer reused via
/// `map_init`), keeps its own top-K heap, and the partial heaps are merged by
/// `merge_top_k`. Callers must not depend on equal-score ordering.
fn score_paths_top_k(
    paths: &[StringId],
    interner: &Arc<Mutex<vtcode_commons::StringInterner>>,
    limit: usize,
    pattern_text: &str,
    match_type: MatchType,
) -> BinaryHeap<Reverse<(u32, StringId, MatchType)>> {
    const CHUNK: usize = 1024;

    // Serial fast path for small inputs: avoids the rayon thread-pool spawn
    // overhead and keeps equal-score ordering deterministic.
    if paths.len() <= CHUNK {
        let mut list = BestMatchesList::new(limit, pattern_text, interner);
        for &path_id in paths {
            let path_opt = interner.lock().get(path_id).map(|s| s.to_string());
            if let Some(path) = path_opt {
                list.record_match(&path, match_type);
            }
        }
        return list.matches;
    }

    let heaps: Vec<_> = paths
        .par_chunks(CHUNK)
        .map_init(
            || BestMatchesList::new(limit, pattern_text, interner),
            |list, chunk| {
                for &path_id in chunk {
                    let path_opt = interner.lock().get(path_id).map(|s| s.to_string());
                    if let Some(path) = path_opt {
                        list.record_match(&path, match_type);
                    }
                }
                std::mem::take(&mut list.matches)
            },
        )
        .collect();

    merge_top_k(heaps, interner, limit)
}

/// Merge worker-local top-K heaps into a single top-K heap.
///
/// Because each input heap already holds only its own highest-scoring `limit`
/// entries, the global top-K is a subset of their union; merging and re-keeping
/// the top-K yields the correct global result.
fn merge_top_k(
    heaps: Vec<BinaryHeap<Reverse<(u32, StringId, MatchType)>>>,
    _interner: &Arc<Mutex<vtcode_commons::StringInterner>>,
    limit: usize,
) -> BinaryHeap<Reverse<(u32, StringId, MatchType)>> {
    let mut merged = BinaryHeap::with_capacity(limit);
    for heap in heaps {
        for Reverse(item) in heap.into_vec() {
            push_top_match(&mut merged, limit, item.0, item.1, item.2);
        }
    }
    merged
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
            match FileIndex::build_from_directory(&search_directory, &exclude, respect_gitignore, threads) {
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
        let path_id = index.interner.lock().intern(path);
        if is_added {
            if Path::new(path).is_dir() {
                index.directories.push(path_id);
            } else {
                index.files.push(path_id);
            }
        } else {
            index.files.retain(|&p| p != path_id);
            index.directories.retain(|&p| p != path_id);
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
            interner: self.interner.clone(),
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
    matches: BinaryHeap<Reverse<(u32, StringId, MatchType)>>,
    limit: usize,
    matcher: nucleo_matcher::Matcher,
    haystack_buf: Vec<char>,
    /// Pre-computed pattern - avoids per-match UTF-32 conversion
    pattern: PatternStorage,
    interner: Arc<Mutex<vtcode_commons::StringInterner>>,
}

/// Stores a pattern in the optimal form for Utf32Str creation.
enum PatternStorage {
    /// ASCII pattern - can be used directly with Utf32Str::Ascii
    Ascii(Vec<u8>),
    /// Unicode pattern - stored as chars for Utf32Str::Unicode
    Unicode(Vec<char>),
}

impl BestMatchesList {
    fn new(limit: usize, pattern_text: &str, interner: &Arc<Mutex<vtcode_commons::StringInterner>>) -> Self {
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
            interner: interner.clone(),
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

        let path_id = self.interner.lock().intern(path);
        push_top_match(&mut self.matches, self.limit, score as u32, path_id, match_type);
        true
    }
}

fn push_top_match(
    matches: &mut BinaryHeap<Reverse<(u32, StringId, MatchType)>>,
    limit: usize,
    score: u32,
    path: StringId,
    match_type: MatchType,
) -> bool {
    let candidate = (score, path, match_type);
    if matches.len() < limit {
        matches.push(Reverse(candidate));
        return true;
    }

    let Some(minimum) = matches.peek().map(|entry| &entry.0) else {
        return false;
    };

    if &candidate <= minimum {
        return false;
    }

    matches.pop();
    matches.push(Reverse(candidate));
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
        return Ok(FileSearchResults { matches: Vec::new(), total_match_count: 0 });
    }

    // Query the index off the async runtime thread to avoid stalling
    // the tokio worker while rayon parallel-scoring runs.
    let index_for_results = index.clone();
    let matched_paths = tokio::task::spawn_blocking({
        let pattern_text = config.pattern_text.clone();
        move || Ok::<_, anyhow::Error>(index.query(&pattern_text, limit, None))
    })
    .await??;

    let total_match_count = matched_paths.len();

    // Build final results
    let matches = matched_paths
        .into_iter()
        .filter_map(|(score, path_id, match_type)| {
            let path = index_for_results.interner.lock().get(path_id)?.to_string();
            Some(FileMatch {
                score,
                path,
                match_type,
                indices: if compute_indices { Some(Vec::new()) } else { None },
            })
        })
        .collect();

    Ok(FileSearchResults { matches, total_match_count })
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
    run_with_policy(config, true, false)
}

/// Run a bounded fuzzy path search without following symbolic links.
///
/// This focused route is intended for request-scoped code search. It traverses
/// eligible paths in deterministic order and stops at the candidate cap. It
/// deliberately avoids the persistent [`FileIndexCache`].
pub fn run_bounded_no_follow(config: FileSearchConfig) -> anyhow::Result<FileSearchResults> {
    run_bounded_no_follow_with_visit(config, |_| {})
}

fn run_bounded_no_follow_with_visit(
    config: FileSearchConfig,
    mut visit: impl FnMut(&Path),
) -> anyhow::Result<FileSearchResults> {
    let limit = config.limit.get();
    let search_directory = &config.search_directory;
    let mut walk_builder = ignore::WalkBuilder::new(search_directory);
    vtcode_commons::walk::apply_defaults(&mut walk_builder);
    walk_builder
        .follow_links(false)
        .require_git(false)
        .sort_by_file_path(|left, right| left.cmp(right));

    if !config.respect_gitignore {
        walk_builder
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false);
    }

    if !config.exclude.is_empty() {
        let mut override_builder = ignore::overrides::OverrideBuilder::new(search_directory);
        for exclude_pattern in &config.exclude {
            override_builder.add(&format!("!{exclude_pattern}"))?;
        }
        walk_builder.overrides(override_builder.build()?);
    }

    let interner = Arc::new(Mutex::new(vtcode_commons::StringInterner::new()));
    let mut matches = BestMatchesList::new(limit, &config.pattern_text, &interner);
    let mut matching_count = 0usize;
    for result in walk_builder.build() {
        if config.cancel_flag.load(Ordering::Relaxed) {
            break;
        }
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        visit(entry.path());
        if !entry.file_type().is_some_and(|file_type| file_type.is_file()) {
            continue;
        }
        let Some(relative_path) = entry
            .path()
            .strip_prefix(search_directory)
            .ok()
            .and_then(|path| path.to_str())
            .filter(|path| !path.is_empty())
        else {
            continue;
        };
        if matches.record_match(relative_path, MatchType::File) {
            matching_count += 1;
            if matching_count >= limit {
                break;
            }
        }
    }

    let interner_guard = interner.lock();
    let matches = matches
        .matches
        .into_sorted_vec()
        .into_iter()
        .filter_map(|Reverse((score, path_id, match_type))| {
            let path = interner_guard.get(path_id)?.to_string();
            Some(FileMatch {
                score,
                path,
                match_type,
                indices: config.compute_indices.then(Vec::new),
            })
        })
        .collect();

    Ok(FileSearchResults {
        matches,
        // Reaching the cap terminates traversal, so report conservative
        // truncation without scanning the rest of the tree for an exact total.
        total_match_count: matching_count + usize::from(matching_count >= limit),
    })
}

fn run_with_policy(
    config: FileSearchConfig,
    follow_links: bool,
    files_only: bool,
) -> anyhow::Result<FileSearchResults> {
    let limit = config.limit.get();
    let search_directory = &config.search_directory;
    let exclude = &config.exclude;
    let threads = config.threads.get();
    let cancel_flag = &config.cancel_flag;
    let compute_indices = config.compute_indices;
    let respect_gitignore = config.respect_gitignore;

    let walker = build_parallel_walker(search_directory, exclude, threads, respect_gitignore, follow_links)?;

    let interner = Arc::new(Mutex::new(vtcode_commons::StringInterner::new()));

    // Create per-worker result collection using Arc + Mutex for thread safety.
    // Each worker gets exactly one instance - no sharing between workers.
    let best_matchers_per_worker: Vec<Arc<Mutex<BestMatchesList>>> = (0..threads)
        .map(|_| Arc::new(Mutex::new(BestMatchesList::new(limit, &config.pattern_text, &interner))))
        .collect();

    let interner_for_merge = interner.clone();
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
        let _interner = interner.clone();

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
            let relative_path = entry.path().strip_prefix(search_directory).ok().and_then(|p| p.to_str());

            let path_to_match = match relative_path {
                Some(p) if !p.is_empty() => p,
                _ => return ignore::WalkState::Continue, // Skip root and non-relative paths
            };

            let match_type = if entry.path().is_dir() {
                MatchType::Directory
            } else {
                MatchType::File
            };

            if files_only && match_type == MatchType::Directory {
                return ignore::WalkState::Continue;
            }

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
    let worker_heaps: Vec<BinaryHeap<Reverse<(u32, StringId, MatchType)>>> = best_matchers_per_worker
        .into_iter()
        .map(|arc| std::mem::take(&mut arc.lock().matches))
        .collect();
    let merged_matches = merge_top_k(worker_heaps, &interner_for_merge, limit);

    // Build final results
    let interner_guard = interner_for_merge.lock();
    let matches = merged_matches
        .into_sorted_vec()
        .into_iter()
        .filter_map(|Reverse((score, path_id, match_type))| {
            let path = interner_guard.get(path_id)?.to_string();
            Some(FileMatch {
                score,
                path,
                match_type,
                indices: if compute_indices { Some(Vec::new()) } else { None },
            })
        })
        .collect();

    Ok(FileSearchResults {
        matches,
        total_match_count: total_match_count.load(Ordering::Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::{FileSearchConfig, run_bounded_no_follow, run_bounded_no_follow_with_visit};
    use std::num::NonZero;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    fn bounded_paths(workspace: &std::path::Path) -> Vec<String> {
        run_bounded_no_follow(FileSearchConfig {
            pattern_text: "widget".to_string(),
            limit: NonZero::new(2).expect("non-zero limit"),
            search_directory: workspace.to_path_buf(),
            exclude: Vec::new(),
            threads: NonZero::new(4).expect("non-zero threads"),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: true,
        })
        .expect("bounded path search")
        .matches
        .into_iter()
        .map(|candidate| candidate.path)
        .collect()
    }

    #[test]
    fn bounded_path_selection_is_stable_across_repeated_walks() {
        let workspace = TempDir::new().expect("workspace");
        for directory in ["z", "a", "m", "b", "y"] {
            let directory = workspace.path().join(directory);
            std::fs::create_dir(&directory).expect("fixture directory");
            std::fs::write(directory.join("widget.rs"), "fn widget() {}\n").expect("fixture source");
        }

        let expected = bounded_paths(workspace.path());
        assert_eq!(expected.len(), 2);
        for _ in 0..20 {
            assert_eq!(bounded_paths(workspace.path()), expected);
        }
    }

    #[test]
    fn bounded_path_selection_is_the_sorted_prefix_and_stops_early() {
        let workspace = TempDir::new().expect("workspace");
        for directory in ["z", "a", "m", "b", "y"] {
            let directory = workspace.path().join(directory);
            std::fs::create_dir(&directory).expect("fixture directory");
            std::fs::write(directory.join("widget.rs"), "fn widget() {}\n").expect("fixture source");
        }
        let mut visited = Vec::new();

        let results = run_bounded_no_follow_with_visit(
            FileSearchConfig {
                pattern_text: "widget".to_string(),
                limit: NonZero::new(2).expect("non-zero limit"),
                search_directory: workspace.path().to_path_buf(),
                exclude: Vec::new(),
                threads: NonZero::new(4).expect("non-zero threads"),
                cancel_flag: Arc::new(AtomicBool::new(false)),
                compute_indices: false,
                respect_gitignore: true,
            },
            |path| visited.push(path.to_path_buf()),
        )
        .expect("bounded path search");
        let mut paths = results.matches.into_iter().map(|candidate| candidate.path).collect::<Vec<_>>();
        paths.sort();

        assert_eq!(paths, vec!["a/widget.rs", "b/widget.rs"]);
        assert!(visited.len() < 11, "the bounded route must stop before traversing the complete fixture tree");
        assert_eq!(results.total_match_count, 3);
    }
}
