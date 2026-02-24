//! Helper that owns the debounce/cancellation logic for `grep_file` operations.
//!
//! This module manages the orchestration of ripgrep searches, implementing
//! debounce and cancellation logic to ensure responsive and efficient searches.
//!
//! It works as follows:
//! 1. First query starts a debounce timer.
//! 2. While the timer is pending, the latest query from the user is stored.
//! 3. When the timer fires, it is cleared, and a search is done for the most
//!    recent query.
//! 4. If there is an in-flight search that is not a prefix of the latest thing
//!    the user typed, it is cancelled.

use super::file_search_bridge::{self, FileSearchConfig};
use super::grep_cache::GrepSearchCache;
use anyhow::{Context, Error as AnyhowError, Result};
use glob::Pattern;
use regex::escape;
use serde_json::{self, Value, json};
use std::io::ErrorKind;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tokio::task::spawn_blocking;
use tracing::warn;

#[cfg(not(docsrs))]
use perg::{SearchConfig, search_paths};

/// Maximum number of search results to return - AGENTS.md requires max 5 results
const MAX_SEARCH_RESULTS: NonZeroUsize = NonZeroUsize::new(5).unwrap();

/// Optimal number of threads for searching, calculated based on CPU count
static OPTIMAL_SEARCH_THREADS: OnceLock<NonZeroUsize> = OnceLock::new();

/// Calculate optimal number of search threads based on available CPU cores
/// Uses 75% of cores, clamped between 2 and 8 threads
fn optimal_search_threads() -> NonZeroUsize {
    *OPTIMAL_SEARCH_THREADS.get_or_init(|| {
        let cpu_count = num_cpus::get();
        // Use 75% of cores for better parallelism, min 2, max 8
        let threads = (cpu_count * 3 / 4).clamp(2, 8);
        NonZeroUsize::new(threads).unwrap_or(NonZeroUsize::new(2).unwrap())
    })
}

/// Maximum bytes to keep in a single grep response before truncation.
const DEFAULT_MAX_RESULT_BYTES: usize = 32 * 1024;

/// Default timeout for blocking grep invocations.
const DEFAULT_SEARCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Default ignore globs to avoid noisy vendor/build directories.
const DEFAULT_IGNORE_GLOBS: &[&str] = &[
    "**/.git/**",
    "**/node_modules/**",
    "**/target/**",
    "**/.cursor/**",
];

/// How long to wait after a keystroke before firing the first search when none
/// is currently running. Keeps early queries more meaningful.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Poll interval when waiting for an active search to complete
const ACTIVE_SEARCH_COMPLETE_POLL_INTERVAL: Duration = Duration::from_millis(20);

use serde::{Deserialize, Serialize};

/// Input parameters for ripgrep search
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GrepSearchInput {
    pub pattern: String,
    pub path: String,
    pub case_sensitive: Option<bool>,
    pub literal: Option<bool>,
    pub glob_pattern: Option<String>,
    pub context_lines: Option<usize>,
    pub include_hidden: Option<bool>,
    pub max_results: Option<usize>,
    pub respect_ignore_files: Option<bool>, // Whether to respect .gitignore, .ignore files
    pub max_file_size: Option<usize>,       // Maximum file size to search (in bytes)
    pub search_hidden: Option<bool>,        // Whether to search hidden files/directories
    pub search_binary: Option<bool>,        // Whether to search binary files
    pub files_with_matches: Option<bool>,   // Only print filenames with matches
    pub type_pattern: Option<String>, // Search files of a specific type (e.g., "rust", "python")
    pub invert_match: Option<bool>,   // Invert the matching
    pub word_boundaries: Option<bool>, // Match only word boundaries (regexp \b)
    pub line_number: Option<bool>,    // Show line numbers
    pub column: Option<bool>,         // Show column numbers
    pub only_matching: Option<bool>,  // Show only matching parts
    pub trim: Option<bool>,           // Trim whitespace from matches
    pub max_result_bytes: Option<usize>, // Optional truncation threshold (bytes)
    pub timeout: Option<Duration>,    // Optional timeout for blocking grep
    pub extra_ignore_globs: Option<Vec<String>>, // Additional ignore globs
}

impl GrepSearchInput {
    /// Create a new search input with pattern and path, using sensible defaults
    #[inline]
    pub fn new(pattern: String, path: String) -> Self {
        Self {
            pattern,
            path,
            case_sensitive: None,
            literal: None,
            glob_pattern: None,
            context_lines: None,
            include_hidden: None,
            max_results: None,
            respect_ignore_files: None,
            max_file_size: None,
            search_hidden: None,
            search_binary: None,
            files_with_matches: None,
            type_pattern: None,
            invert_match: None,
            word_boundaries: None,
            line_number: None,
            column: None,
            only_matching: None,
            trim: None,
            max_result_bytes: None,
            timeout: None,
            extra_ignore_globs: None,
        }
    }

    /// Create a search input with common defaults for internal grep searches
    #[inline]
    pub fn with_defaults(pattern: String, path: String) -> Self {
        Self {
            pattern,
            path,
            case_sensitive: Some(true),
            literal: Some(false),
            glob_pattern: None,
            context_lines: None,
            include_hidden: Some(false),
            max_results: Some(MAX_SEARCH_RESULTS.get()),
            respect_ignore_files: Some(true),
            max_file_size: None,
            search_hidden: Some(false),
            search_binary: Some(false),
            files_with_matches: Some(false),
            type_pattern: None,
            invert_match: Some(false),
            word_boundaries: Some(false),
            line_number: Some(true),
            column: Some(false),
            only_matching: Some(false),
            trim: Some(false),
            max_result_bytes: Some(DEFAULT_MAX_RESULT_BYTES),
            timeout: Some(DEFAULT_SEARCH_TIMEOUT),
            extra_ignore_globs: None,
        }
    }
}

fn is_hidden_path(path: &str) -> bool {
    Path::new(path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|segment| segment.starts_with('.') && segment != "." && segment != "..")
}

/// Result of a ripgrep search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepSearchResult {
    pub query: String,
    pub matches: Vec<serde_json::Value>,
    pub truncated: bool,
}

/// State machine for grep_file orchestration.
pub struct GrepSearchManager {
    /// Unified state guarded by one mutex.
    state: Arc<Mutex<SearchState>>,

    search_dir: PathBuf,

    /// LRU cache for search results to avoid redundant searches
    cache: Arc<GrepSearchCache>,
}

struct SearchState {
    /// Latest query typed by user (updated every keystroke).
    latest_query: String,

    /// true if a search is currently scheduled.
    is_search_scheduled: bool,

    /// If there is an active search, this will be the query being searched.
    active_search: Option<ActiveSearch>,
    last_result: Option<GrepSearchResult>,
}

struct ActiveSearch {
    query: String,
    cancellation_token: Arc<AtomicBool>,
}

impl GrepSearchManager {
    pub fn new(search_dir: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(SearchState {
                latest_query: String::new(),
                is_search_scheduled: false,
                active_search: None,
                last_result: None,
            })),
            search_dir,
            cache: Arc::new(GrepSearchCache::new(100)), // Cache up to 100 recent searches
        }
    }

    fn cached_result(cache: &GrepSearchCache, input: &GrepSearchInput) -> Option<GrepSearchResult> {
        cache.get(input).map(|cached| GrepSearchResult {
            query: cached.query.clone(),
            matches: cached.matches.clone(),
            truncated: cached.truncated,
        })
    }

    /// Call whenever the user edits the search query.
    pub fn on_user_query(&self, query: String) {
        {
            let mut st = match self.state.lock() {
                Ok(state) => state,
                Err(err) => {
                    warn!("grep search state lock poisoned while handling query update: {err}");
                    return;
                }
            };
            if query == st.latest_query {
                // No change, nothing to do.
                return;
            }

            // Update latest query.
            st.latest_query.clear();
            st.latest_query.push_str(&query);

            // If there is an in-flight search that is definitely obsolete,
            // cancel it now.
            if let Some(active_search) = &st.active_search
                && !query.starts_with(&active_search.query)
            {
                active_search
                    .cancellation_token
                    .store(true, Ordering::Relaxed);
                st.active_search = None;
            }

            // Schedule a search to run after debounce.
            if !st.is_search_scheduled {
                st.is_search_scheduled = true;
            } else {
                return;
            }
        }

        // If we are here, we set `st.is_search_scheduled = true` before
        // dropping the lock. This means we are the only thread that can spawn a
        // debounce timer.
        let state = self.state.clone();
        let search_dir = self.search_dir.clone();
        let cache = self.cache.clone();
        // Run debounce and search spawn on a blocking thread to avoid
        // blocking the async runtime or reader threads.
        tokio::task::spawn_blocking(move || {
            // Always do a minimum debounce, but then poll until the
            // `active_search` is cleared.
            thread::sleep(SEARCH_DEBOUNCE);
            loop {
                let active_is_none = match state.lock() {
                    Ok(st) => st.active_search.is_none(),
                    Err(err) => {
                        warn!(
                            "grep search state lock poisoned while waiting for active search: {err}"
                        );
                        return;
                    }
                };
                if active_is_none {
                    break;
                }
                thread::sleep(ACTIVE_SEARCH_COMPLETE_POLL_INTERVAL);
            }

            // The debounce timer has expired, so start a search using the
            // latest query.
            let cancellation_token = Arc::new(AtomicBool::new(false));
            let token = cancellation_token.clone();
            let query = {
                let mut st = match state.lock() {
                    Ok(state) => state,
                    Err(err) => {
                        warn!(
                            "grep search state lock poisoned while preparing debounced search: {err}"
                        );
                        return;
                    }
                };
                let query = st.latest_query.clone();
                st.is_search_scheduled = false;
                st.active_search = Some(ActiveSearch {
                    query: query.clone(),
                    cancellation_token: token,
                });
                query
            };

            GrepSearchManager::spawn_grep_file(
                query,
                search_dir,
                cancellation_token,
                state,
                Some(cache),
            );
        });
    }

    /// Retrieve the last successful search result
    pub fn last_result(&self) -> Option<GrepSearchResult> {
        match self.state.lock() {
            Ok(st) => st.last_result.clone(),
            Err(err) => {
                warn!("grep search state lock poisoned while reading last result: {err}");
                None
            }
        }
    }

    fn execute_with_backends(input: &GrepSearchInput) -> Result<(Vec<Value>, bool)> {
        match Self::run_ripgrep_backend(input) {
            Ok(matches) => Ok(matches),
            Err(err) => {
                if Self::is_ripgrep_missing(&err) {
                    #[cfg(not(docsrs))]
                    {
                        Self::run_perg_backend(input).with_context(|| {
                            format!(
                                "perg fallback failed for pattern '{}' under '{}'",
                                input.pattern, input.path
                            )
                        })
                    }
                    #[cfg(docsrs)]
                    {
                        // When building docs.rs, return an empty result since perg functionality is not available
                        Ok(Vec::new())
                    }
                } else {
                    Err(err)
                }
            }
        }
    }

    fn run_ripgrep_backend(input: &GrepSearchInput) -> Result<(Vec<Value>, bool)> {
        use std::process::Command;

        let mut cmd = Command::new("rg");
        cmd.arg("-j")
            .arg(optimal_search_threads().get().to_string());

        // Add support for respecting ignore files (default is to respect them)
        if !input.respect_ignore_files.unwrap_or(true) {
            cmd.arg("--no-ignore");
        }

        // Add support for searching hidden files (default is not to search hidden)
        if input.search_hidden.unwrap_or(false) {
            cmd.arg("--hidden");
        }

        // Add support for searching binary files
        if input.search_binary.unwrap_or(false) {
            cmd.arg("--binary");
        }

        // Add support for files with matches only
        if input.files_with_matches.unwrap_or(false) {
            cmd.arg("--files-with-matches");
        }

        // Add support for file type filtering
        if let Some(type_pattern) = &input.type_pattern {
            cmd.arg("--type").arg(type_pattern);
        }

        // Add support for max file size
        if let Some(max_file_size) = input.max_file_size {
            cmd.arg("--max-filesize").arg(format!("{}B", max_file_size));
        }

        // Case sensitivity
        if let Some(case_sensitive) = input.case_sensitive {
            if case_sensitive {
                cmd.arg("--case-sensitive");
            } else {
                cmd.arg("--ignore-case");
            }
        } else {
            // Default to smart case if not specified
            cmd.arg("--smart-case");
        }

        // Invert match
        if input.invert_match.unwrap_or(false) {
            cmd.arg("--invert-match");
        }

        // Word boundaries
        if input.word_boundaries.unwrap_or(false) {
            cmd.arg("--word-regexp");
        }

        // Line numbers
        if input.line_number.unwrap_or(true) {
            // Default to true to maintain context
            cmd.arg("--line-number");
        } else {
            cmd.arg("--no-line-number");
        }

        // Column numbers
        if input.column.unwrap_or(false) {
            cmd.arg("--column");
        }

        // Only matching parts
        if input.only_matching.unwrap_or(false) {
            cmd.arg("--only-matching");
        }

        // Trim whitespace (handled by not adding the --no-unicode flag, which is default)
        if input.trim.unwrap_or(false) {
            // This is handled in post-processing, not as a flag
        }

        if let Some(literal) = input.literal
            && literal
        {
            cmd.arg("--fixed-strings");
        }

        if let Some(glob_pattern) = &input.glob_pattern {
            cmd.arg("--glob").arg(glob_pattern);
        }

        if input.respect_ignore_files.unwrap_or(true) {
            for pattern in DEFAULT_IGNORE_GLOBS {
                cmd.arg("--glob").arg(format!("!{}", pattern));
            }
            if let Some(extra) = &input.extra_ignore_globs {
                for pattern in extra {
                    cmd.arg("--glob").arg(format!("!{}", pattern));
                }
            }
        }

        if let Some(context_lines) = input.context_lines {
            cmd.arg("--context").arg(context_lines.to_string());
        }

        let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
        cmd.arg("--max-count").arg(max_results.to_string());

        // Use JSON output format for structured results
        cmd.arg("--json");

        cmd.arg(&input.pattern);
        cmd.arg(&input.path);

        let output = cmd.output().with_context(|| {
            format!("failed to execute ripgrep for pattern '{}'", input.pattern)
        })?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        // Pre-allocate with reasonable estimate (typically less than max_results lines in output)
        let line_count = output_str.lines().count();
        let mut matches = Vec::with_capacity(line_count.min(max_results));
        for line in output_str.lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                matches.push(value);
            }
        }

        Ok(Self::finalize_matches(matches, input))
    }

    fn finalize_matches(mut matches: Vec<Value>, input: &GrepSearchInput) -> (Vec<Value>, bool) {
        let mut truncated = false;
        let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
        if matches.len() > max_results {
            matches.truncate(max_results);
            truncated = true;
        }

        if let Some(limit) = input.max_result_bytes {
            let mut total = 0usize;
            let mut kept = Vec::with_capacity(matches.len());
            for entry in matches.into_iter() {
                let bytes = serde_json::to_vec(&entry)
                    .map(|v| v.len())
                    .unwrap_or_default();
                if total + bytes > limit {
                    truncated = true;
                    break;
                }
                total += bytes;
                kept.push(entry);
            }
            matches = kept;
        }

        (matches, truncated)
    }

    #[cfg(not(docsrs))]
    fn run_perg_backend(input: &GrepSearchInput) -> Result<(Vec<Value>, bool)> {
        let mut pattern = input.pattern.clone();
        if input.literal.unwrap_or(false) {
            pattern = escape(&pattern);
        }

        // Map the most relevant grep options into perg's configuration structure.
        let case_sensitive = input.case_sensitive.unwrap_or(false);
        let config = SearchConfig::new(
            pattern,
            !case_sensitive,
            input.line_number.unwrap_or(true),
            true,
            input.invert_match.unwrap_or(false),
            input.files_with_matches.unwrap_or(false),
            false,
            false,
            0,
            0,
            input.context_lines.unwrap_or(0),
            input.max_results,
            input.only_matching.unwrap_or(false),
            false,
            String::from("never"),
        );

        // Pre-allocate output buffer with reasonable estimate for search results
        let mut output_buffer = Vec::with_capacity(8192); // 8KB initial capacity
        let search_targets = vec![input.path.clone()];

        // Execute the search
        search_paths(&config, &search_targets, true, true, &mut output_buffer)
            .with_context(|| format!("perg search failed for path '{}'", input.path))?;

        let output = String::from_utf8(output_buffer)
            .with_context(|| "perg search output was not valid UTF-8".to_string())?;

        let glob_filter = input
            .glob_pattern
            .as_ref()
            .map(|pattern| {
                Pattern::new(pattern).with_context(|| {
                    format!("invalid glob pattern '{}' for perg fallback", pattern)
                })
            })
            .transpose()?;

        let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
        // Pre-allocate with estimate based on output lines
        let line_count = output.lines().count();
        let mut matches = Vec::with_capacity(line_count.min(max_results));

        for line in output.lines() {
            if matches.len() >= max_results {
                break;
            }

            if line.trim().is_empty() {
                continue;
            }

            let (prefix, text) = match line.rsplit_once(':') {
                Some(result) => result,
                None => continue,
            };

            let mut prefix_end = prefix.len();
            // Pre-allocate numeric_segments - typically 2-3 segments (file:line:column)
            let mut numeric_segments = Vec::with_capacity(3);

            while let Some(pos) = prefix[..prefix_end].rfind(':') {
                let segment = &prefix[pos + 1..prefix_end];

                if !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()) {
                    numeric_segments.push(segment);
                    prefix_end = pos;
                } else {
                    break;
                }
            }

            if prefix_end == 0 {
                continue;
            }

            let file = &prefix[..prefix_end];

            // Apply glob filtering
            if let Some(pattern) = &glob_filter
                && !pattern.matches(file)
            {
                continue;
            }

            // Check file size if max_file_size is specified
            if let Some(max_size) = input.max_file_size
                && let Ok(metadata) = std::fs::metadata(file)
                && metadata.len() as usize > max_size
            {
                continue;
            }

            // Check if file is hidden and respect include_hidden setting
            if !input.include_hidden.unwrap_or(false) && is_hidden_path(file) {
                continue;
            }

            let line_number = numeric_segments
                .get(1)
                .or_else(|| numeric_segments.first())
                .and_then(|num| num.parse::<u64>().ok())
                .unwrap_or(0);

            matches.push(json!({
                "type": "match",
                "data": {
                    "path": {"text": file},
                    "line_number": line_number,
                    "lines": {"text": format!("{}\n", text)},
                }
            }));
        }

        Ok(Self::finalize_matches(matches, input))
    }

    #[cfg(docsrs)]
    fn run_perg_backend(_input: &GrepSearchInput) -> Result<(Vec<Value>, bool)> {
        // When building docs.rs, return an empty result since perg functionality is not available
        Ok((Vec::new(), false))
    }

    fn is_ripgrep_missing(err: &AnyhowError) -> bool {
        err.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .map(|io_err| io_err.kind() == ErrorKind::NotFound)
                .unwrap_or(false)
        })
    }

    fn spawn_grep_file(
        query: String,
        search_dir: PathBuf,
        cancellation_token: Arc<AtomicBool>,
        search_state: Arc<Mutex<SearchState>>,
        cache: Option<Arc<GrepSearchCache>>,
    ) {
        // Spawn grep worker on a blocking thread — searching and ripgrep are blocking.
        tokio::task::spawn_blocking(move || {
            // Check if cancelled before starting
            if cancellation_token.load(Ordering::Relaxed) {
                // Reset the active search state
                {
                    let mut st = match search_state.lock() {
                        Ok(state) => state,
                        Err(err) => {
                            warn!("grep search state lock poisoned while cancelling search: {err}");
                            return;
                        }
                    };
                    if let Some(active_search) = &st.active_search
                        && Arc::ptr_eq(&active_search.cancellation_token, &cancellation_token)
                    {
                        st.active_search = None;
                    }
                }
                return;
            }

            let input = GrepSearchInput::with_defaults(
                query.clone(),
                search_dir.to_string_lossy().into_owned(),
            );

            // Check cache first if available
            if let Some(ref cache) = cache
                && let Some(cached_result) = Self::cached_result(cache, &input)
            {
                let mut st = match search_state.lock() {
                    Ok(state) => state,
                    Err(err) => {
                        warn!("grep search state lock poisoned while loading cached result: {err}");
                        return;
                    }
                };
                st.last_result = Some(cached_result);
                return;
            }

            let search_result = GrepSearchManager::execute_with_backends(&input);

            let is_cancelled = cancellation_token.load(Ordering::Relaxed);
            if !is_cancelled
                && let Ok((matches, truncated)) = search_result
                && !matches.is_empty()
            {
                let result = GrepSearchResult {
                    query,
                    matches,
                    truncated,
                };

                // Cache the result if cache is available
                if let Some(ref cache) = cache
                    && GrepSearchCache::should_cache(&result)
                {
                    cache.put(&input, result.clone());
                }

                let mut st = match search_state.lock() {
                    Ok(state) => state,
                    Err(err) => {
                        warn!("grep search state lock poisoned while storing search result: {err}");
                        return;
                    }
                };
                st.last_result = Some(result);
            }

            // Reset the active search state
            {
                let mut st = match search_state.lock() {
                    Ok(state) => state,
                    Err(err) => {
                        warn!(
                            "grep search state lock poisoned while clearing active search: {err}"
                        );
                        return;
                    }
                };
                if let Some(active_search) = &st.active_search
                    && Arc::ptr_eq(&active_search.cancellation_token, &cancellation_token)
                {
                    st.active_search = None;
                }
            }
        });
    }

    /// Perform an actual ripgrep search with the given input parameters
    pub async fn perform_search(&self, input: GrepSearchInput) -> Result<GrepSearchResult> {
        // Check cache first
        if let Some(cached_result) = Self::cached_result(&self.cache, &input) {
            return Ok(cached_result);
        }

        let query = input.pattern.clone();
        let input_clone = input.clone();

        let timeout = input.timeout.unwrap_or(DEFAULT_SEARCH_TIMEOUT);
        let (matches, truncated) = tokio::time::timeout(
            timeout,
            spawn_blocking(move || GrepSearchManager::execute_with_backends(&input_clone)),
        )
        .await
        .context("ripgrep search timed out")?
        .context("ripgrep search worker panicked")??;

        let result = GrepSearchResult {
            query,
            matches,
            truncated,
        };

        // Cache the result if it's worth caching (non-empty, successful)
        if GrepSearchCache::should_cache(&result) {
            self.cache.put(&input, result.clone());
        }

        Ok(result)
    }

    /// Perform file enumeration using the optimized file search bridge
    ///
    /// This method uses the vtcode-file-search crate for parallel, fuzzy file discovery.
    /// It's optimized for:
    /// - Listing files in large directories
    /// - Fuzzy filename matching
    /// - Respecting .gitignore and .ignore files
    /// - Parallel directory traversal
    ///
    /// # Arguments
    ///
    /// * `pattern` - Fuzzy search pattern for filenames (e.g., "main", "test.rs")
    /// * `max_results` - Maximum number of files to return
    /// * `cancel_flag` - Optional cancellation token for early termination
    ///
    /// # Returns
    ///
    /// A vector of file paths matching the pattern, sorted by match quality
    pub fn enumerate_files_with_pattern(
        &self,
        pattern: String,
        max_results: usize,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) -> Result<Vec<String>> {
        let config = FileSearchConfig::new(pattern, self.search_dir.clone())
            .with_limit(max_results)
            .respect_gitignore(true);

        let results = file_search_bridge::search_files(config, cancel_flag)?;

        Ok(results.matches.into_iter().map(|m| m.path).collect())
    }

    /// List all files in the search directory using the file search bridge
    ///
    /// This is useful for operations that need to enumerate all discoverable files
    /// without a specific pattern match.
    ///
    /// # Arguments
    ///
    /// * `max_results` - Maximum number of files to return
    /// * `exclude_patterns` - Patterns to exclude from results (glob-style)
    ///
    /// # Returns
    ///
    /// A vector of file paths
    pub fn list_all_files(
        &self,
        max_results: usize,
        exclude_patterns: Vec<String>,
    ) -> Result<Vec<String>> {
        let mut config = FileSearchConfig::new("".to_string(), self.search_dir.clone())
            .with_limit(max_results)
            .respect_gitignore(true);

        for pattern in exclude_patterns {
            config = config.exclude(pattern);
        }

        let results = file_search_bridge::search_files(config, None)?;

        Ok(results.matches.into_iter().map(|m| m.path).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finalize_matches_respects_max_bytes() {
        let mut input = GrepSearchInput::with_defaults("pat".into(), ".".into());
        input.max_result_bytes = Some(100);
        input.max_results = Some(5);

        let matches = vec![json!({"text": "12345"}), json!({"text": "6789"})];

        let (kept, truncated) = GrepSearchManager::finalize_matches(matches, &input);
        assert!(!truncated);
        assert_eq!(kept.len(), 2);

        // Test with smaller limit that truncates
        input.max_result_bytes = Some(20);
        let matches = vec![json!({"text": "12345"}), json!({"text": "6789"})];
        let (kept, truncated) = GrepSearchManager::finalize_matches(matches, &input);
        assert!(truncated);
        assert_eq!(kept.len(), 1); // Only first match fits in 20 bytes
    }

    #[test]
    fn test_grep_search_manager_creation() {
        let manager = GrepSearchManager::new(PathBuf::from("."));
        assert_eq!(manager.search_dir, PathBuf::from("."));
    }

    #[test]
    fn test_grep_search_input_new() {
        let input = GrepSearchInput::new("pattern".to_string(), "/path/to/search".to_string());
        assert_eq!(input.pattern, "pattern");
        assert_eq!(input.path, "/path/to/search");
        assert!(input.case_sensitive.is_none());
    }

    #[test]
    fn test_grep_search_input_with_defaults() {
        let input = GrepSearchInput::with_defaults("pattern".to_string(), "/path".to_string());
        assert_eq!(input.pattern, "pattern");
        assert_eq!(input.path, "/path");
        assert_eq!(input.case_sensitive, Some(true));
        assert_eq!(input.include_hidden, Some(false));
        assert_eq!(input.max_results, Some(MAX_SEARCH_RESULTS.get()));
    }
}
