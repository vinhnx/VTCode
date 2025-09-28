//! Helper that owns the debounce/cancellation logic for `rp_search` operations.
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

use anyhow::Result;
use serde_json;
use std::num::NonZeroUsize;
use std::path::PathBuf;
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

/// Result of a ripgrep search
#[derive(Debug, Clone)]
pub struct GrepSearchResult {
    pub query: String,
    pub matches: Vec<serde_json::Value>,
}

/// State machine for grep_search orchestration.
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

            GrepSearchManager::spawn_rp_search(query, search_dir, cancellation_token, state);
        });
    }

    /// Retrieve the last successful search result
    pub fn last_result(&self) -> Option<GrepSearchResult> {
        #[expect(clippy::unwrap_used)]
        let st = self.state.lock().unwrap();
        st.last_result.clone()
    }

    fn spawn_rp_search(
        query: String,
        search_dir: PathBuf,
        cancellation_token: Arc<AtomicBool>,
        search_state: Arc<Mutex<SearchState>>,
    ) {
        use std::process::Command;

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

            // Build the ripgrep command
            let mut cmd = Command::new("rg");
            cmd.arg("-j").arg(NUM_SEARCH_THREADS.get().to_string());

            // Add the search pattern
            cmd.arg(&query);

            // Add the search path
            cmd.arg(search_dir.to_string_lossy().as_ref());

            // Output as JSON for easier parsing
            cmd.arg("--json");

            // Set result limits
            cmd.arg("--max-count")
                .arg(MAX_SEARCH_RESULTS.get().to_string());

            // Execute the command
            let output = cmd.output();

            let is_cancelled = cancellation_token.load(Ordering::Relaxed);
            if !is_cancelled
                && let Ok(output) = output
                && output.status.success()
            {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let mut matches = Vec::new();
                for line in output_str.lines() {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                        matches.push(val);
                    }
                }
                let result = GrepSearchResult {
                    query: query.clone(),
                    matches,
                };
                #[expect(clippy::unwrap_used)]
                let mut st = search_state.lock().unwrap();
                st.last_result = Some(result);
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
        // std::path::Path import removed as it's not directly used
        use std::process::Command;

        // Build the ripgrep command
        let mut cmd = Command::new("rg");

        // Add the search pattern
        cmd.arg(&input.pattern);

        // Add the search path
        cmd.arg(&input.path);

        // Add optional flags
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

        // Set result limits
        let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
        cmd.arg("--max-count").arg(max_results.to_string());

        // Output as JSON for easier parsing
        cmd.arg("--json");

        // Execute the command
        let output = cmd.output()?;

        if !output.status.success() {
            // If ripgrep is not found, return an error
            if String::from_utf8_lossy(&output.stderr).contains("not found") {
                return Err(anyhow::anyhow!(
                    "ripgrep (rg) command not found. Please install ripgrep to use search functionality."
                ));
            }

            // For other errors, still return results but with a warning
        }

        // Parse the JSON output
        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut matches = Vec::new();

        for line in output_str.lines() {
            if !line.trim().is_empty()
                && let Ok(json_value) = serde_json::from_str::<serde_json::Value>(line)
            {
                matches.push(json_value);
            }
        }

        Ok(GrepSearchResult {
            query: input.pattern,
            matches,
        })
    }
}
