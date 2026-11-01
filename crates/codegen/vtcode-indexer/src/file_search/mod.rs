#![expect(
    unused_results,
    reason = "File-search walker and heap operations intentionally use fluent/configuration mutation for side effects."
)]

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

use arc_swap::ArcSwapOption;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;

use rayon::prelude::*;
use vtcode_commons::StringId;

mod types;

pub use types::{FileMatch, FileSearchConfig, FileSearchResults, MatchType};

/// Pre-computed file index for instant queries.
///
/// This index is built in the background and cached to avoid
/// repeated directory traversals on every search.
pub struct FileIndex {
    files: Vec<StringId>,
    directories: Vec<StringId>,
    /// Immutable path text indexed by the corresponding [`StringId`].
    /// Searches only read this table, so scoring never contends on the
    /// incremental-update interner or allocates a candidate string.
    path_texts_by_id: Arc<Vec<Arc<str>>>,
    /// Used only while applying incremental index updates.
    interner: vtcode_commons::StringInterner,
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
        let worker_results = Arc::new(Mutex::new(Vec::new()));

        walker.run(|| {
            let search_dir = search_directory.to_path_buf();
            let mut state = IndexWorkerGuard {
                results: Arc::clone(&worker_results),
                files: Vec::new(),
                directories: Vec::new(),
            };

            Box::new(move |result| {
                let entry = match result {
                    Ok(e) => e,
                    Err(_) => return ignore::WalkState::Continue,
                };

                // Make path relative to search directory
                if let Some(rel_path) = entry.path().strip_prefix(&search_dir).ok().and_then(|p| p.to_str())
                    && !rel_path.is_empty()
                {
                    if entry.file_type().is_some_and(|file_type| file_type.is_dir()) {
                        state.directories.push(rel_path.to_string());
                    } else {
                        state.files.push(rel_path.to_string());
                    }
                }

                ignore::WalkState::Continue
            })
        });

        let worker_results = Arc::try_unwrap(worker_results)
            .map_err(|results| {
                anyhow::anyhow!("file index worker results still referenced: {}", Arc::strong_count(&results))
            })?
            .into_inner();
        let (files, directories) =
            worker_results
                .into_iter()
                .fold((Vec::new(), Vec::new()), |(mut files, mut directories), result| {
                    files.extend(result.files);
                    directories.extend(result.directories);
                    (files, directories)
                });

        // Pre-size the interner arena and lookup tables from the collected
        // paths so the build below does not repeatedly realloc and rehash.
        let total_path_bytes: usize =
            files.iter().map(String::len).sum::<usize>() + directories.iter().map(String::len).sum::<usize>();
        let mut interner =
            vtcode_commons::StringInterner::with_capacity(total_path_bytes, files.len() + directories.len());
        let mut path_texts_by_id = Vec::with_capacity(files.len() + directories.len());
        let interned_files: Vec<StringId> = files
            .iter()
            .map(|path| intern_path(path, &mut interner, &mut path_texts_by_id))
            .collect();
        let interned_dirs: Vec<StringId> = directories
            .iter()
            .map(|path| intern_path(path, &mut interner, &mut path_texts_by_id))
            .collect();

        Ok(Self {
            files: interned_files,
            directories: interned_dirs,
            path_texts_by_id: Arc::new(path_texts_by_id),
            interner,
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
            heaps.push(score_paths_top_k(
                &self.files,
                self.path_texts_by_id.as_slice(),
                limit,
                pattern_text,
                MatchType::File,
            ));
        }

        if match_type_filter.is_none_or(|t| t == MatchType::Directory) {
            heaps.push(score_paths_top_k(
                &self.directories,
                self.path_texts_by_id.as_slice(),
                limit,
                pattern_text,
                MatchType::Directory,
            ));
        }

        merge_top_k(heaps, limit)
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse(item)| item)
            .collect()
    }
}

fn intern_path(
    path: &str,
    interner: &mut vtcode_commons::StringInterner,
    path_texts_by_id: &mut Vec<Arc<str>>,
) -> StringId {
    let path_id = interner.intern(path);
    let path_index = path_id.as_u32() as usize;
    if path_index == path_texts_by_id.len() {
        path_texts_by_id.push(Arc::from(path));
    }
    path_id
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
    path_texts_by_id: &[Arc<str>],
    limit: usize,
    pattern_text: &str,
    match_type: MatchType,
) -> BinaryHeap<Reverse<(u32, StringId, MatchType)>> {
    const CHUNK: usize = 1024;

    // Serial fast path for small inputs: avoids the rayon thread-pool spawn
    // overhead and keeps equal-score ordering deterministic.
    if paths.len() <= CHUNK {
        let mut list = BestMatchesList::new(limit, pattern_text);
        for &path_id in paths {
            if let Some(path) = path_texts_by_id.get(path_id.as_u32() as usize) {
                list.record_match(path_id, path, match_type);
            }
        }
        return list.matches;
    }

    let heaps: Vec<_> = paths
        .par_chunks(CHUNK)
        .map_init(
            || BestMatchesList::new(limit, pattern_text),
            |list, chunk| {
                for &path_id in chunk {
                    if let Some(path) = path_texts_by_id.get(path_id.as_u32() as usize) {
                        list.record_match(path_id, path, match_type);
                    }
                }
                std::mem::take(&mut list.matches)
            },
        )
        .collect();

    merge_top_k(heaps, limit)
}

/// Merge worker-local top-K heaps into a single top-K heap.
///
/// Because each input heap already holds only its own highest-scoring `limit`
/// entries, the global top-K is a subset of their union; merging and re-keeping
/// the top-K yields the correct global result.
fn merge_top_k(
    heaps: Vec<BinaryHeap<Reverse<(u32, StringId, MatchType)>>>,
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
    /// Lock-free copy of the latest published index for synchronous callers.
    /// This keeps `refresh_background` from blocking or panicking when called
    /// on a Tokio worker that is concurrently publishing a replacement.
    snapshot: Arc<ArcSwapOption<FileIndex>>,
    /// Serializes full index builds so concurrent cache misses do not launch
    /// duplicate workspace traversals and Rayon jobs.
    build_gate: Arc<tokio::sync::Semaphore>,
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
            snapshot: Arc::new(ArcSwapOption::empty()),
            build_gate: Arc::new(tokio::sync::Semaphore::new(1)),
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

        // Re-check after waiting for another caller's build. This avoids a
        // cache-stampede when several searches arrive on an empty/stale cache.
        let _build_permit = self.build_gate.acquire().await?;
        {
            let guard = self.cache.read().await;
            if let Some(index) = guard.as_ref()
                && index.last_built.elapsed() < std::time::Duration::from_secs(300)
            {
                return Ok(Arc::clone(index));
            }
        }

        // Directory traversal and index construction are synchronous and can
        // touch a large workspace. Keep that work off the Tokio worker so a
        // cache miss cannot delay unrelated async tasks.
        let search_directory = self.search_directory.clone();
        let exclude = self.exclude.clone();
        let respect_gitignore = self.respect_gitignore;
        let threads = self.threads;
        let index = Arc::new(
            tokio::task::spawn_blocking(move || {
                FileIndex::build_from_directory(&search_directory, &exclude, respect_gitignore, threads)
            })
            .await??,
        );

        // Cache and return
        {
            let mut guard = self.cache.write().await;
            *guard = Some(Arc::clone(&index));
            self.snapshot.store(Some(Arc::clone(&index)));
        }
        Ok(index)
    }

    /// Force refresh the index in the background.
    ///
    /// Returns the latest published index immediately while rebuilding happens
    /// asynchronously. If no Tokio runtime is available, no refresh is
    /// scheduled and the latest published index is returned unchanged.
    pub fn refresh_background(&self) -> Option<Arc<FileIndex>> {
        let runtime = match tokio::runtime::Handle::try_current() {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::debug!(%error, "cannot refresh file index without a Tokio runtime");
                return self.snapshot.load_full();
            }
        };

        // Build new index asynchronously
        let search_directory = self.search_directory.clone();
        let exclude = self.exclude.clone();
        let respect_gitignore = self.respect_gitignore;
        let threads = self.threads;
        let cache = self.cache.clone();
        let snapshot = Arc::clone(&self.snapshot);
        let build_gate = Arc::clone(&self.build_gate);

        runtime.spawn(async move {
            let _build_permit = match build_gate.acquire_owned().await {
                Ok(permit) => permit,
                Err(error) => {
                    tracing::error!(%error, "file index build gate closed");
                    return;
                }
            };

            match tokio::task::spawn_blocking(move || {
                FileIndex::build_from_directory(&search_directory, &exclude, respect_gitignore, threads)
            })
            .await
            {
                Ok(Ok(new_index)) => {
                    let new_index = Arc::new(new_index);
                    let mut guard = cache.write().await;
                    *guard = Some(Arc::clone(&new_index));
                    snapshot.store(Some(new_index));
                }
                Ok(Err(error)) => {
                    tracing::error!(%error, "failed to rebuild file index");
                }
                Err(error) => {
                    tracing::error!(%error, "file index rebuild task failed");
                }
            }
        });

        self.snapshot.load_full()
    }

    /// Incrementally update the index when a file change is detected.
    /// This is faster than a full rebuild for single file changes.
    pub fn update_file(&self, path: &str, is_added: bool) {
        let mut guard = self.cache.blocking_write();
        let Some(existing) = guard.take() else { return };

        let mut index = Arc::try_unwrap(existing).unwrap_or_else(|arc| (*arc).clone());
        if is_added {
            let path_id = intern_path(path, &mut index.interner, Arc::make_mut(&mut index.path_texts_by_id));
            let is_directory = self.search_directory.join(path).is_dir();
            if is_directory {
                index.files.retain(|&existing| existing != path_id);
                if !index.directories.contains(&path_id) {
                    index.directories.push(path_id);
                }
            } else {
                index.directories.retain(|&existing| existing != path_id);
                if !index.files.contains(&path_id) {
                    index.files.push(path_id);
                }
            }
        } else {
            let Some(path_id) = index.files.iter().chain(index.directories.iter()).copied().find(|&path_id| {
                index
                    .path_texts_by_id
                    .get(path_id.as_u32() as usize)
                    .is_some_and(|value| value.as_ref() == path)
            }) else {
                let index = Arc::new(index);
                *guard = Some(Arc::clone(&index));
                self.snapshot.store(Some(index));
                return;
            };
            index.files.retain(|&existing| existing != path_id);
            index.directories.retain(|&existing| existing != path_id);
        }
        index.last_built = std::time::Instant::now();
        let index = Arc::new(index);
        *guard = Some(Arc::clone(&index));
        self.snapshot.store(Some(index));
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
            path_texts_by_id: Arc::clone(&self.path_texts_by_id),
            interner: self.interner.clone(),
            last_built: self.last_built,
        }
    }
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
    owned_matches: BinaryHeap<Reverse<OwnedMatchCandidate>>,
}

#[derive(Debug, Eq, PartialEq)]
struct OwnedMatchCandidate {
    score: u32,
    path: String,
    match_type: MatchType,
}

impl Ord for OwnedMatchCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .cmp(&other.score)
            .then_with(|| other.path.cmp(&self.path))
            .then_with(|| other.match_type.cmp(&self.match_type))
    }
}

impl PartialOrd for OwnedMatchCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct SearchAggregation {
    matches: BinaryHeap<Reverse<OwnedMatchCandidate>>,
    total_match_count: usize,
}

struct IndexWorkerResult {
    files: Vec<String>,
    directories: Vec<String>,
}

struct IndexWorkerGuard {
    results: Arc<Mutex<Vec<IndexWorkerResult>>>,
    files: Vec<String>,
    directories: Vec<String>,
}

impl Drop for IndexWorkerGuard {
    fn drop(&mut self) {
        self.results.lock().push(IndexWorkerResult {
            files: std::mem::take(&mut self.files),
            directories: std::mem::take(&mut self.directories),
        });
    }
}

struct SearchWorkerGuard {
    results: Arc<Mutex<SearchAggregation>>,
    best_list: BestMatchesList,
    total_match_count: usize,
    limit: usize,
}

impl Drop for SearchWorkerGuard {
    fn drop(&mut self) {
        let worker_matches = std::mem::take(&mut self.best_list.owned_matches);
        let mut results = self.results.lock();
        results.total_match_count += self.total_match_count;
        for Reverse(candidate) in worker_matches {
            push_owned_match(&mut results.matches, self.limit, candidate);
        }
    }
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
            owned_matches: BinaryHeap::new(),
        }
    }

    /// Score a path using the pre-computed pattern without allocating a
    /// temporary string.
    fn score_path(&mut self, path: &str) -> Option<u32> {
        let haystack = nucleo_matcher::Utf32Str::new(path, &mut self.haystack_buf);
        let needle = match &self.pattern {
            PatternStorage::Ascii(bytes) => nucleo_matcher::Utf32Str::Ascii(bytes),
            PatternStorage::Unicode(chars) => nucleo_matcher::Utf32Str::Unicode(chars),
        };
        self.matcher.fuzzy_match(haystack, needle).map(|score| score as u32)
    }

    /// Record a matching path with an already-known [`StringId`].
    fn record_match(&mut self, path_id: StringId, path: &str, match_type: MatchType) -> bool {
        let Some(score) = self.score_path(path) else {
            return false;
        };
        push_top_match(&mut self.matches, self.limit, score, path_id, match_type);
        true
    }

    fn record_scored_match(&mut self, path_id: StringId, score: u32, match_type: MatchType) {
        push_top_match(&mut self.matches, self.limit, score, path_id, match_type);
    }

    fn record_owned_match(&mut self, path: &str, score: u32, match_type: MatchType) {
        if self.owned_matches.len() == self.limit {
            let Some(Reverse(worst)) = self.owned_matches.peek() else {
                return;
            };
            if !owned_match_is_better(score, path, match_type, worst) {
                return;
            }
        }

        push_owned_match(
            &mut self.owned_matches,
            self.limit,
            OwnedMatchCandidate { score, path: path.to_owned(), match_type },
        );
    }
}

fn push_owned_match(
    matches: &mut BinaryHeap<Reverse<OwnedMatchCandidate>>,
    limit: usize,
    candidate: OwnedMatchCandidate,
) {
    if matches.len() == limit {
        let Some(Reverse(worst)) = matches.peek() else {
            return;
        };
        if !owned_match_is_better(candidate.score, candidate.path.as_str(), candidate.match_type, worst) {
            return;
        }
        matches.pop();
    }

    matches.push(Reverse(candidate));
}

fn owned_match_is_better(score: u32, path: &str, match_type: MatchType, worst: &OwnedMatchCandidate) -> bool {
    score
        .cmp(&worst.score)
        .then_with(|| worst.path.as_str().cmp(path))
        .then_with(|| worst.match_type.cmp(&match_type))
        .is_gt()
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
            let path = index_for_results.path_texts_by_id.get(path_id.as_u32() as usize)?.to_string();
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
    let mut matches = BestMatchesList::new(limit, &config.pattern_text);
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
        let Some(score) = matches.score_path(relative_path) else {
            continue;
        };
        let path_id = interner.lock().intern(relative_path);
        matches.record_scored_match(path_id, score, MatchType::File);
        matching_count += 1;
        if matching_count >= limit {
            break;
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

    let worker_results = Arc::new(Mutex::new(SearchAggregation { matches: BinaryHeap::new(), total_match_count: 0 }));
    walker.run(|| {
        let mut state = SearchWorkerGuard {
            results: Arc::clone(&worker_results),
            best_list: BestMatchesList::new(limit, &config.pattern_text),
            total_match_count: 0,
            limit,
        };

        Box::new(move |result| {
            // Check cancellation flag periodically
            if cancel_flag.load(Ordering::Relaxed) {
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

            let Some(file_type) = entry.file_type() else {
                return ignore::WalkState::Continue;
            };
            let match_type = if file_type.is_dir() {
                MatchType::Directory
            } else {
                MatchType::File
            };

            if files_only && match_type == MatchType::Directory {
                return ignore::WalkState::Continue;
            }

            // Try to add to results - no contention with other workers
            let Some(score) = state.best_list.score_path(path_to_match) else {
                return ignore::WalkState::Continue;
            };
            state.best_list.record_owned_match(path_to_match, score, match_type);
            state.total_match_count += 1;

            ignore::WalkState::Continue
        })
    });

    let results = Arc::try_unwrap(worker_results)
        .map_err(|results| {
            anyhow::anyhow!("file search worker results still referenced: {}", Arc::strong_count(&results))
        })?
        .into_inner();
    let total_match_count = results.total_match_count;
    let mut candidates = results
        .matches
        .into_iter()
        .map(|Reverse(candidate)| candidate)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.match_type.cmp(&right.match_type))
    });
    candidates.truncate(limit);

    // Build final results
    let matches = candidates
        .into_iter()
        .map(|candidate| FileMatch {
            score: candidate.score,
            path: candidate.path,
            match_type: candidate.match_type,
            indices: if compute_indices { Some(Vec::new()) } else { None },
        })
        .collect();

    Ok(FileSearchResults { matches, total_match_count })
}

#[cfg(test)]
mod tests;
