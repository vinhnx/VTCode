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

use anyhow::{Context, Error as AnyhowError, Result};
use glob::Pattern;
use perg::{SearchConfig, search_paths};
use regex::escape;
use serde_json::{self, Value, json};
use std::io::ErrorKind;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

/// Maximum number of search results to return
const MAX_SEARCH_RESULTS: NonZeroUsize = NonZeroUsize::new(100).unwrap();

/// Number of threads to use for searching
const NUM_SEARCH_THREADS: NonZeroUsize = NonZeroUsize::new(2).unwrap();

/// How long to wait after a keystroke before firing the first search when none
/// is currently running. Keeps early queries more meaningful.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Poll interval when waiting for an active search to complete
const ACTIVE_SEARCH_COMPLETE_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Input parameters for ripgrep search
#[derive(Debug, Clone)]
pub struct GrepSearchInput {
    pub pattern: String,
    pub path: String,
    pub case_sensitive: Option<bool>,
    pub literal: Option<bool>,
    pub glob_pattern: Option<String>,
    pub context_lines: Option<usize>,
    pub include_hidden: Option<bool>,
    pub max_results: Option<usize>,
}

fn is_hidden_path(path: &str) -> bool {
    Path::new(path)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|segment| segment.starts_with('.') && segment != "." && segment != "..")
}

/// Result of a ripgrep search
#[derive(Debug, Clone)]
pub struct GrepSearchResult {
    pub query: String,
    pub matches: Vec<serde_json::Value>,
}

/// State machine for grep_file orchestration.
pub struct GrepSearchManager {
    /// Unified state guarded by one mutex.
    state: Arc<Mutex<SearchState>>,

    search_dir: PathBuf,
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
        }
    }

    /// Call whenever the user edits the search query.
    pub fn on_user_query(&self, query: String) {
        {
            #[expect(clippy::unwrap_used)]
            let mut st = self.state.lock().unwrap();
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
        thread::spawn(move || {
            // Always do a minimum debounce, but then poll until the
            // `active_search` is cleared.
            thread::sleep(SEARCH_DEBOUNCE);
            loop {
                #[expect(clippy::unwrap_used)]
                if state.lock().unwrap().active_search.is_none() {
                    break;
                }
                thread::sleep(ACTIVE_SEARCH_COMPLETE_POLL_INTERVAL);
            }

            // The debounce timer has expired, so start a search using the
            // latest query.
            let cancellation_token = Arc::new(AtomicBool::new(false));
            let token = cancellation_token.clone();
            let query = {
                #[expect(clippy::unwrap_used)]
                let mut st = state.lock().unwrap();
                let query = st.latest_query.clone();
                st.is_search_scheduled = false;
                st.active_search = Some(ActiveSearch {
                    query: query.clone(),
                    cancellation_token: token,
                });
                query
            };

            GrepSearchManager::spawn_grep_file(query, search_dir, cancellation_token, state);
        });
    }

    /// Retrieve the last successful search result
    pub fn last_result(&self) -> Option<GrepSearchResult> {
        #[expect(clippy::unwrap_used)]
        let st = self.state.lock().unwrap();
        st.last_result.clone()
    }

    fn execute_with_backends(input: &GrepSearchInput) -> Result<Vec<Value>> {
        match Self::run_ripgrep_backend(input) {
            Ok(matches) => Ok(matches),
            Err(err) => {
                if Self::is_ripgrep_missing(&err) {
                    Self::run_perg_backend(input).with_context(|| {
                        format!(
                            "perg fallback failed for pattern '{}' under '{}'",
                            input.pattern, input.path
                        )
                    })
                } else {
                    Err(err)
                }
            }
        }
    }

    fn run_ripgrep_backend(input: &GrepSearchInput) -> Result<Vec<Value>> {
        use std::process::Command;

        let mut cmd = Command::new("rg");
        cmd.arg("-j").arg(NUM_SEARCH_THREADS.get().to_string());
        cmd.arg(&input.pattern);
        cmd.arg(&input.path);

        if let Some(case_sensitive) = input.case_sensitive {
            if case_sensitive {
                cmd.arg("--case-sensitive");
            } else {
                cmd.arg("--ignore-case");
            }
        }

        if let Some(literal) = input.literal
            && literal
        {
            cmd.arg("--fixed-strings");
        }

        if let Some(glob_pattern) = &input.glob_pattern {
            cmd.arg("--glob").arg(glob_pattern);
        }

        if let Some(context_lines) = input.context_lines {
            cmd.arg("--context").arg(context_lines.to_string());
        }

        if let Some(include_hidden) = input.include_hidden
            && include_hidden
        {
            cmd.arg("--hidden");
        }

        let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
        cmd.arg("--max-count").arg(max_results.to_string());
        cmd.arg("--json");

        let output = cmd.output().with_context(|| {
            format!("failed to execute ripgrep for pattern '{}'", input.pattern)
        })?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut matches = Vec::new();
        for line in output_str.lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                matches.push(value);
            }
        }

        Ok(matches)
    }

    fn run_perg_backend(input: &GrepSearchInput) -> Result<Vec<Value>> {
        let mut pattern = input.pattern.clone();
        if input.literal.unwrap_or(false) {
            pattern = escape(&pattern);
        }

        let config = SearchConfig::new(
            pattern,
            !input.case_sensitive.unwrap_or(true),
            true,
            true,
            false,
            false,
            false,
        );

        let mut output_buffer = Vec::new();
        let search_targets = vec![input.path.clone()];
        search_paths(&config, &search_targets, true, true, &mut output_buffer)
            .with_context(|| format!("perg search failed for path '{}'", input.path))?;

        let output = String::from_utf8(output_buffer)
            .with_context(|| "perg search output was not valid UTF-8".to_string())?;

        let glob_filter =
            if let Some(pattern) = &input.glob_pattern {
                Some(Pattern::new(pattern).with_context(|| {
                    format!("invalid glob pattern '{}' for perg fallback", pattern)
                })?)
            } else {
                None
            };

        let include_hidden = input.include_hidden.unwrap_or(false);
        let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
        let mut matches = Vec::new();

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

            let mut remaining = prefix;
            let mut numeric_segments = Vec::new();

            while let Some((rest, segment)) = remaining.rsplit_once(':') {
                if segment.chars().all(|c| c.is_ascii_digit()) {
                    numeric_segments.push(segment);
                    remaining = rest;
                } else {
                    break;
                }
            }

            if remaining.is_empty() {
                continue;
            }

            let file = remaining;

            if let Some(pattern) = &glob_filter {
                if !pattern.matches(file) {
                    continue;
                }
            }

            if !include_hidden && is_hidden_path(file) {
                continue;
            }

            let line_number = numeric_segments
                .get(1)
                .or_else(|| numeric_segments.get(0))
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

        Ok(matches)
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
    ) {
        thread::spawn(move || {
            // Check if cancelled before starting
            if cancellation_token.load(Ordering::Relaxed) {
                // Reset the active search state
                {
                    #[expect(clippy::unwrap_used)]
                    let mut st = search_state.lock().unwrap();
                    if let Some(active_search) = &st.active_search
                        && Arc::ptr_eq(&active_search.cancellation_token, &cancellation_token)
                    {
                        st.active_search = None;
                    }
                }
                return;
            }

            let input = GrepSearchInput {
                pattern: query.clone(),
                path: search_dir.to_string_lossy().into_owned(),
                case_sensitive: Some(true),
                literal: Some(false),
                glob_pattern: None,
                context_lines: None,
                include_hidden: Some(false),
                max_results: Some(MAX_SEARCH_RESULTS.get()),
            };

            let search_result = GrepSearchManager::execute_with_backends(&input);

            let is_cancelled = cancellation_token.load(Ordering::Relaxed);
            if !is_cancelled {
                if let Ok(matches) = search_result {
                    if !matches.is_empty() {
                        let result = GrepSearchResult {
                            query: query.clone(),
                            matches,
                        };
                        #[expect(clippy::unwrap_used)]
                        let mut st = search_state.lock().unwrap();
                        st.last_result = Some(result);
                    }
                }
            }

            // Reset the active search state
            {
                #[expect(clippy::unwrap_used)]
                let mut st = search_state.lock().unwrap();
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
        let matches = GrepSearchManager::execute_with_backends(&input)?;

        Ok(GrepSearchResult {
            query: input.pattern,
            matches,
        })
    }
}
