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
use crate::cache::estimate_json_size;
use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tracing::warn;

// Backend items (input/result types, streaming helpers) are re-exported here
// so existing `crate::tools::grep_file::*` paths keep resolving.
pub(crate) use super::grep_backend::*;

/// How long to wait after a keystroke before firing the first search when none
/// is currently running. Keeps early queries more meaningful.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Poll interval when waiting for an active search to complete
const ACTIVE_SEARCH_COMPLETE_POLL_INTERVAL: Duration = Duration::from_millis(20);

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
                active_search.cancellation_token.store(true, Ordering::Relaxed);
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
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            warn!("grep search query ignored: no active Tokio runtime");
            if let Ok(mut st) = self.state.lock() {
                st.is_search_scheduled = false;
                // Clear so the same query is not deduped away once a runtime exists.
                st.latest_query.clear();
            }
            return;
        };
        // Run debounce and search spawn on a blocking thread to avoid
        // blocking the async runtime or reader threads.
        let search_runtime = runtime.clone();
        drop(runtime.spawn_blocking(move || {
            // Always do a minimum debounce, but then poll until the
            // `active_search` is cleared.
            thread::sleep(SEARCH_DEBOUNCE);
            loop {
                let active_is_none = match state.lock() {
                    Ok(st) => st.active_search.is_none(),
                    Err(err) => {
                        warn!("grep search state lock poisoned while waiting for active search: {err}");
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
                        warn!("grep search state lock poisoned while preparing debounced search: {err}");
                        return;
                    }
                };
                let query = st.latest_query.clone();
                st.is_search_scheduled = false;
                st.active_search = Some(ActiveSearch { query: query.clone(), cancellation_token: token });
                query
            };

            // Spawn the search on the async runtime so it can be killed on
            // timeout or cancellation. The debounce loop above already ensured
            // no active search is running.
            drop(search_runtime.spawn(GrepSearchManager::spawn_grep_file(
                query,
                search_dir,
                cancellation_token,
                state,
                Some(cache),
            )));
        }));
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

    async fn execute_with_backends(
        input: &GrepSearchInput,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<(Vec<Value>, bool, usize)> {
        let matches = run_ripgrep_backend_async(input, cancel_flag).await?;
        Ok(Self::finalize_matches(matches, input))
    }

    fn finalize_matches(mut matches: Vec<Value>, input: &GrepSearchInput) -> (Vec<Value>, bool, usize) {
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
            let is_match = entry.get("type").and_then(Value::as_str).is_some_and(|t| t == "match");
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
                let entry_bytes = estimate_json_size(entry) as usize;
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

    async fn spawn_grep_file(
        query: String,
        search_dir: PathBuf,
        cancellation_token: Arc<AtomicBool>,
        search_state: Arc<Mutex<SearchState>>,
        cache: Option<Arc<GrepSearchCache>>,
    ) {
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

        let input = GrepSearchInput::with_defaults(query.clone(), search_dir.to_string_lossy().into_owned());

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

        // Run with a hard deadline so a runaway rg on a huge tree cannot burn
        // CPU forever. The async backend owns the child with `kill_on_drop`, so
        // both the timeout and mid-search cancellation kill ripgrep.
        let timeout = input.timeout.unwrap_or(DEFAULT_SEARCH_TIMEOUT);
        let search_result = match tokio::time::timeout(
            timeout,
            GrepSearchManager::execute_with_backends(&input, Some(&cancellation_token)),
        )
        .await
        {
            Ok(result) => result,
            Err(_elapsed) => Err(anyhow::anyhow!("ripgrep search timed out after {}s", timeout.as_secs())),
        };

        let is_cancelled = cancellation_token.load(Ordering::Relaxed);
        if !is_cancelled
            && let Ok((matches, truncated, total_match_count)) = search_result
            && !matches.is_empty()
        {
            let result = GrepSearchResult {
                query,
                matches,
                truncated,
                total_matches: if truncated { Some(total_match_count) } else { None },
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
                    warn!("grep search state lock poisoned while clearing active search: {err}");
                    return;
                }
            };
            if let Some(active_search) = &st.active_search
                && Arc::ptr_eq(&active_search.cancellation_token, &cancellation_token)
            {
                st.active_search = None;
            }
        }
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
        // Run ripgrep as an async child owned by the future. On timeout the
        // future is dropped and `kill_on_drop` kills the child — the old
        // `spawn_blocking` + `join.abort()` could not preempt the blocking
        // `Command::output()`, so timed-out searches leaked an rg process that
        // kept burning CPU.
        let outcome = tokio::time::timeout(timeout, Self::execute_with_backends(&input_clone, None)).await;
        let (matches, truncated, total_match_count) = match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(worker_err)) => {
                return Err(worker_err.context("ripgrep search worker failed"));
            }
            Err(_elapsed) => {
                return Err(anyhow::anyhow!(
                    "ripgrep search timed out after {}s; ripgrep was killed",
                    timeout.as_secs()
                ));
            }
        };

        let result = GrepSearchResult {
            query,
            matches,
            truncated,
            total_matches: if truncated { Some(total_match_count) } else { None },
        };

        // Cache the result if it's worth caching (non-empty, successful)
        if GrepSearchCache::should_cache(&result) {
            self.cache.put(&input, result.clone());
        }

        Ok(result)
    }

    /// Perform file enumeration using the optimized file search bridge
    ///
    /// This method uses vtcode-indexer::file_search for parallel, fuzzy file discovery.
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
    pub fn list_all_files(&self, max_results: usize, exclude_patterns: Vec<String>) -> Result<Vec<String>> {
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
    use crate::tools::ast_grep_language::AstGrepLanguage;
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn code_search_literal_stream_reaps_at_candidate_cap() {
        let workspace = TempDir::new().expect("workspace");
        std::fs::write(workspace.path().join("matches.txt"), "Widget\nWidget\nWidget\n").expect("fixture");

        let outcome = search_literal_bounded("Widget", workspace.path(), &[], 2)
            .await
            .expect("bounded literal search");

        assert_eq!(outcome.candidates.len(), 2);
        assert!(outcome.truncated);
    }

    #[tokio::test]
    async fn code_search_literal_candidate_cap_selects_stable_path_prefix() {
        let workspace = TempDir::new().expect("workspace");
        for name in ["z.txt", "a.txt", "m.txt"] {
            std::fs::write(workspace.path().join(name), "Widget\nWidget\n").expect("fixture");
        }

        let first = search_literal_bounded("Widget", workspace.path(), &[], 2)
            .await
            .expect("first bounded search");
        let second = search_literal_bounded("Widget", workspace.path(), &[], 2)
            .await
            .expect("second bounded search");

        assert_eq!(first, second);
        assert!(first.truncated);
        assert!(
            first.candidates.iter().all(|candidate| candidate.path.ends_with("a.txt")),
            "sorted cap prefix should come from a.txt: {first:?}"
        );
    }

    #[tokio::test]
    async fn code_search_literal_stream_reaps_at_byte_cap() {
        let workspace = TempDir::new().expect("workspace");
        std::fs::write(
            workspace.path().join("large.txt"),
            format!("Widget{}\n", "x".repeat(CODE_SEARCH_STREAM_BYTE_CAP)),
        )
        .expect("fixture");

        let outcome = search_literal_bounded("Widget", workspace.path(), &[], 20)
            .await
            .expect("byte-bounded literal search");

        assert!(outcome.candidates.is_empty());
        assert!(outcome.truncated);
    }

    #[tokio::test]
    async fn code_search_bounded_record_distinguishes_exact_eof_from_exhaustion() {
        let mut exact_reader = BufReader::new(&b"{}\n"[..]);
        let mut exact_record = Vec::with_capacity(3);
        let mut exact_bytes_read = 0;
        assert_eq!(
            read_bounded_record(&mut exact_reader, &mut exact_record, &mut exact_bytes_read, 3,)
                .await
                .expect("exact record"),
            BoundedRecordRead::Record
        );
        exact_record.clear();
        assert_eq!(
            read_bounded_record(&mut exact_reader, &mut exact_record, &mut exact_bytes_read, 3,)
                .await
                .expect("exact EOF probe"),
            BoundedRecordRead::Eof
        );

        let mut oversized_reader = BufReader::new(&b"xxxx"[..]);
        let mut oversized_record = Vec::with_capacity(3);
        let mut oversized_bytes_read = 0;
        assert_eq!(
            read_bounded_record(&mut oversized_reader, &mut oversized_record, &mut oversized_bytes_read, 3,)
                .await
                .expect("oversized record"),
            BoundedRecordRead::Exhausted
        );
        assert_eq!(oversized_record.len(), 3);
    }

    #[tokio::test]
    async fn code_search_literal_language_prefilters_are_case_insensitive() {
        let workspace = TempDir::new().expect("workspace");
        std::fs::write(workspace.path().join("UPPER.RS"), "Widget\n").expect("Rust fixture");
        std::fs::write(workspace.path().join("DOCKERFILE"), "Widget\n").expect("Dockerfile fixture");

        let rust = search_literal_bounded("Widget", workspace.path(), &[AstGrepLanguage::Rust], 20)
            .await
            .expect("uppercase Rust extension");
        assert!(rust.candidates.iter().any(|candidate| {
            candidate.path.ends_with("UPPER.RS")
                && AstGrepLanguage::from_path(&candidate.path) == Some(AstGrepLanguage::Rust)
        }));

        let dockerfile = search_literal_bounded("Widget", workspace.path(), &[AstGrepLanguage::Dockerfile], 20)
            .await
            .expect("uppercase Dockerfile name");
        assert!(dockerfile.candidates.iter().any(|candidate| {
            candidate.path.ends_with("DOCKERFILE")
                && AstGrepLanguage::from_path(&candidate.path) == Some(AstGrepLanguage::Dockerfile)
        }));
    }

    #[tokio::test]
    async fn code_search_literal_alternation_matches_each_term() {
        // Reproduces the plan-mode failure in turn_724: a `|`-alternation
        // query used to return 0 because `--fixed-strings` treated the whole
        // string as one literal substring.
        let workspace = TempDir::new().expect("workspace");
        std::fs::write(workspace.path().join("a.rs"), "tokio\n").expect("fixture a");
        std::fs::write(workspace.path().join("b.rs"), "async-std\n").expect("fixture b");
        std::fs::write(workspace.path().join("c.rs"), "runtime\n").expect("fixture c");
        std::fs::write(workspace.path().join("d.rs"), "unrelated\n").expect("fixture d");

        let outcome = search_literal_bounded("tokio|async-std|runtime", workspace.path(), &[], 20)
            .await
            .expect("alternation search");

        let matched_paths: Vec<String> = outcome
            .candidates
            .iter()
            .map(|candidate| {
                candidate
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        assert!(matched_paths.contains(&"a.rs".to_string()), "tokio term should match: {matched_paths:?}");
        assert!(matched_paths.contains(&"b.rs".to_string()), "async-std term should match: {matched_paths:?}");
        assert!(matched_paths.contains(&"c.rs".to_string()), "runtime term should match: {matched_paths:?}");
        assert!(
            !matched_paths.contains(&"d.rs".to_string()),
            "unrelated fixture should not match: {matched_paths:?}"
        );
        assert!(!outcome.truncated);
    }

    #[tokio::test]
    async fn code_search_literal_single_term_still_uses_fixed_strings() {
        // A single-term query with regex metacharacters must stay literal so
        // e.g. `fn main()` is not interpreted as a regex group.
        let workspace = TempDir::new().expect("workspace");
        std::fs::write(workspace.path().join("src.rs"), "fn main() {}\n").expect("fixture");

        let outcome = search_literal_bounded("fn main()", workspace.path(), &[], 20)
            .await
            .expect("single-term literal search");

        assert_eq!(outcome.candidates.len(), 1);
        assert!(outcome.candidates[0].path.ends_with("src.rs"));
    }

    #[test]
    fn split_alternation_handles_edge_cases() {
        assert_eq!(split_alternation(""), Vec::<String>::new());
        assert_eq!(split_alternation("   "), Vec::<String>::new());
        assert_eq!(split_alternation("Widget"), vec!["Widget".to_string()]);
        assert_eq!(split_alternation("  Widget  "), vec!["Widget".to_string()]);
        assert_eq!(
            split_alternation("tokio|async-std|runtime"),
            vec!["tokio".to_string(), "async-std".to_string(), "runtime".to_string()]
        );
        // Empty terms are dropped.
        assert_eq!(split_alternation("|foo|"), vec!["foo".to_string()]);
        assert_eq!(split_alternation("a||b"), vec!["a".to_string(), "b".to_string()]);
        // A single non-empty term after splitting stays single (no alternation).
        assert_eq!(split_alternation("||"), Vec::<String>::new());
    }

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
    fn on_user_query_without_runtime_does_not_panic_and_allows_retry() {
        let manager = GrepSearchManager::new(PathBuf::from("."));
        manager.on_user_query("first");
        manager.on_user_query("first");
        let state = manager.state.lock().expect("state lock");
        assert!(state.latest_query.is_empty());
        assert!(!state.is_search_scheduled);
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
