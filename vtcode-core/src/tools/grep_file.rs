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
use anyhow::{Context, Result};
use serde_json::{self, Value};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tokio::task::spawn_blocking;
use tracing::warn;

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

use vtcode_commons::exclusions::DEFAULT_IGNORE_GLOBS;

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

/// Result of a ripgrep search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepSearchResult {
    pub query: String,
    pub matches: Vec<Value>,
    pub truncated: bool,
    /// Total number of "match" type entries found before truncation.
    /// When `truncated` is true, this tells the agent how many matches exist
    /// vs how many are returned in `matches`.
    #[serde(default)]
    pub total_matches: Option<usize>,
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
            total_matches: cached.total_matches,
        })
    }

    /// Call whenever the user edits the search query.
    pub fn on_user_query(&self, query: &str) {
        {
            let mut st = match self.state.lock() {
                Ok(state) => state,
                Err(err) => {
                    warn!("grep search state lock poisoned while handling query update: {err}");
                    return;
                }
            };
            if query != st.latest_query {
                st.latest_query.clear();
                st.latest_query.push_str(query);
            } else {
                return;
            }

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
        spawn_blocking(move || {
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

    fn execute_with_backends(input: &GrepSearchInput) -> Result<(Vec<Value>, bool, usize)> {
        Self::run_ripgrep_backend(input)
    }

    fn run_ripgrep_backend(input: &GrepSearchInput) -> Result<(Vec<Value>, bool, usize)> {
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
        let matches: Vec<Value> = output_str
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect();

        Ok(Self::finalize_matches(matches, input))
    }

    fn finalize_matches(
        mut matches: Vec<Value>,
        input: &GrepSearchInput,
    ) -> (Vec<Value>, bool, usize) {
        let mut truncated = false;
        let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());

        if max_results == 0 {
            return (Vec::new(), !matches.is_empty(), 0);
        }

        // Count total "match" type entries before any truncation.
        let total_match_count = matches
            .iter()
            .filter(|e| e.get("type").and_then(Value::as_str) == Some("match"))
            .count();

        // Count only "match" type entries (not "context", "begin", "end") so that
        // context lines don't crowd out actual matches from the result set.
        let mut match_count = 0usize;
        let mut cut_index = matches.len();
        for (i, entry) in matches.iter().enumerate() {
            let is_match = entry
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t == "match");
            if is_match {
                match_count += 1;
                if match_count >= max_results {
                    // Keep everything up to and including this match, plus any
                    // trailing context lines that belong to it.
                    cut_index = i + 1;
                    // Advance past trailing context lines for this match.
                    for rest in matches.iter().skip(i + 1) {
                        let tp = rest.get("type").and_then(Value::as_str);
                        if tp == Some("context") {
                            cut_index += 1;
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
        }
        // Check if there are more match-type entries beyond our cut point.
        if matches[cut_index..]
            .iter()
            .any(|e| e.get("type").and_then(Value::as_str) == Some("match"))
        {
            truncated = true;
        }
        if cut_index < matches.len() {
            matches.truncate(cut_index);
        }

        if let Some(limit) = input.max_result_bytes {
            let mut total = 0usize;
            let mut kept_count = 0;
            for entry in &matches {
                let entry_bytes = entry.to_string().len();
                if total + entry_bytes > limit {
                    truncated = true;
                    break;
                }
                total += entry_bytes;
                kept_count += 1;
            }
            matches.truncate(kept_count);
        }

        (matches, truncated, total_match_count)
    }

    fn spawn_grep_file(
        query: String,
        search_dir: PathBuf,
        cancellation_token: Arc<AtomicBool>,
        search_state: Arc<Mutex<SearchState>>,
        cache: Option<Arc<GrepSearchCache>>,
    ) {
        // Spawn grep worker on a blocking thread — searching and ripgrep are blocking.
        spawn_blocking(move || {
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
                && let Ok((matches, truncated, total_match_count)) = search_result
                && !matches.is_empty()
            {
                let result = GrepSearchResult {
                    query,
                    matches,
                    truncated,
                    total_matches: if truncated {
                        Some(total_match_count)
                    } else {
                        None
                    },
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
        let (matches, truncated, total_match_count) = tokio::time::timeout(
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
            total_matches: if truncated {
                Some(total_match_count)
            } else {
                None
            },
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

        Ok(file_search_bridge::file_matches_only(results.matches)
            .into_iter()
            .map(|m| m.path)
            .collect())
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

        Ok(file_search_bridge::file_matches_only(results.matches)
            .into_iter()
            .map(|m| m.path)
            .collect())
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

        let (kept, truncated, _total) = GrepSearchManager::finalize_matches(matches, &input);
        assert!(!truncated);
        assert_eq!(kept.len(), 2);

        // Test with smaller limit that truncates
        input.max_result_bytes = Some(20);
        let matches = vec![json!({"text": "12345"}), json!({"text": "6789"})];
        let (kept, truncated, _total) = GrepSearchManager::finalize_matches(matches, &input);
        assert!(truncated);
        assert_eq!(kept.len(), 1); // Only first match fits in 20 bytes
    }

    #[test]
    fn finalize_matches_counts_only_match_type_entries() {
        let mut input = GrepSearchInput::with_defaults("pat".into(), ".".into());
        input.max_results = Some(2);

        // Simulate ripgrep JSON output: begin, context, match, context, end
        let matches = vec![
            json!({"type": "begin", "data": {"path": {"text": "Cargo.lock"}}}),
            json!({"type": "context", "data": {"line_number": 538, "lines": {"text": "ctx1"}}}),
            json!({"type": "context", "data": {"line_number": 539, "lines": {"text": "ctx2"}}}),
            json!({"type": "match", "data": {"line_number": 553, "lines": {"text": "match1"}}}),
            json!({"type": "context", "data": {"line_number": 554, "lines": {"text": "ctx3"}}}),
            json!({"type": "context", "data": {"line_number": 555, "lines": {"text": "ctx4"}}}),
            json!({"type": "context", "data": {"line_number": 560, "lines": {"text": "ctx5"}}}),
            json!({"type": "match", "data": {"line_number": 563, "lines": {"text": "match2"}}}),
            json!({"type": "context", "data": {"line_number": 564, "lines": {"text": "ctx6"}}}),
            json!({"type": "end", "data": {"path": {"text": "Cargo.lock"}}}),
        ];

        let (kept, truncated, total) = GrepSearchManager::finalize_matches(matches, &input);
        // Should keep all entries up through the second match's trailing context.
        // match_count reaches 2 at index 7, then trailing context at index 8 -> cut_index = 9.
        assert!(!truncated);
        assert_eq!(kept.len(), 9);
        assert_eq!(kept[3]["type"], "match");
        assert_eq!(kept[7]["type"], "match");
        assert_eq!(total, 2);
    }

    #[test]
    fn finalize_matches_truncates_when_more_match_types_than_limit() {
        let mut input = GrepSearchInput::with_defaults("pat".into(), ".".into());
        input.max_results = Some(1);

        let matches = vec![
            json!({"type": "begin", "data": {"path": {"text": "f.txt"}}}),
            json!({"type": "match", "data": {"line_number": 1, "lines": {"text": "m1"}}}),
            json!({"type": "context", "data": {"line_number": 2, "lines": {"text": "c1"}}}),
            json!({"type": "match", "data": {"line_number": 10, "lines": {"text": "m2"}}}),
            json!({"type": "context", "data": {"line_number": 11, "lines": {"text": "c2"}}}),
        ];

        let (kept, truncated, total) = GrepSearchManager::finalize_matches(matches, &input);
        assert!(truncated);
        // Keeps: begin + match1 + context after match1 = 3 entries
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[1]["type"], "match");
        assert_eq!(kept[2]["type"], "context");
        assert_eq!(total, 2); // 2 match-type entries in the raw input
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
