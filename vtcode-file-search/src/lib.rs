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

use nucleo_matcher::{Matcher, Utf32Str};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::num::NonZero;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// A single file match result.
///
/// Fields:
/// - `score`: Relevance score from fuzzy matching (higher is better)
/// - `path`: File path relative to the search directory
/// - `indices`: Optional character positions for highlighting matched characters
#[derive(Debug, Clone, Serialize)]
pub struct FileMatch {
    pub score: u32,
    pub path: String,
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

/// Extract the filename from a path, with fallback to the full path.
///
/// # Examples
///
/// ```
/// use vtcode_file_search::file_name_from_path;
///
/// assert_eq!(file_name_from_path("src/main.rs"), "main.rs");
/// assert_eq!(file_name_from_path("Cargo.toml"), "Cargo.toml");
/// assert_eq!(file_name_from_path("/absolute/path/file.txt"), "file.txt");
/// ```
pub fn file_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Best matches list per worker thread (lock-free collection).
///
/// Each worker thread gets its own instance to avoid locking during
/// directory traversal. Results are merged at the end.
struct BestMatchesList {
    matches: BinaryHeap<Reverse<(u32, String)>>,
    limit: usize,
    matcher: Matcher,
    haystack_buf: Vec<char>,
}

impl BestMatchesList {
    fn new(limit: usize) -> Self {
        Self {
            matches: BinaryHeap::new(),
            limit,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            haystack_buf: Vec::new(),
        }
    }

    /// Try to add a match, maintaining top-K results.
    ///
    /// Returns Some(score) if the match was added or if it would replace
    /// a lower-scoring match.
    fn try_add(&mut self, path: &str, pattern_text: &str) -> Option<u32> {
        self.haystack_buf.clear();
        let haystack = Utf32Str::new(path, &mut self.haystack_buf);
        let mut pattern_buf = Vec::new();
        let needle = Utf32Str::new(pattern_text, &mut pattern_buf);
        let score = self.matcher.fuzzy_match(haystack, needle)? as u32;

        if self.matches.len() < self.limit {
            self.matches.push(Reverse((score, path.to_string())));
            Some(score)
        } else {
            let min_score = self.matches.peek().unwrap().0 .0;
            if score > min_score {
                self.matches.pop();
                self.matches.push(Reverse((score, path.to_string())));
                Some(score)
            } else {
                None
            }
        }
    }

    /// Convert into sorted matches (highest score first).
    #[allow(dead_code)]
    fn into_matches(self) -> Vec<(u32, String)> {
        self.matches
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse((score, path))| (score, path))
            .collect()
    }

    /// Clone current matches without consuming self.
    fn clone_matches(&self) -> Vec<(u32, String)> {
        self.matches
            .iter()
            .map(|Reverse((score, path))| (*score, path.clone()))
            .collect()
    }
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
    let pattern_text = &config.pattern_text;
    let limit = config.limit;
    let search_directory = &config.search_directory;
    let exclude = &config.exclude;
    let threads = config.threads;
    let cancel_flag = &config.cancel_flag;
    let compute_indices = config.compute_indices;
    let respect_gitignore = config.respect_gitignore;
    // Store pattern text for cloning across threads
    // (Pattern is parsed per-match to work with Utf32Str)

    // Build the directory walker
    let mut walk_builder = ignore::WalkBuilder::new(search_directory);
    walk_builder
        .threads(threads.get())
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

    // Add exclusion patterns
    if !exclude.is_empty() {
        let mut override_builder = ignore::overrides::OverrideBuilder::new(search_directory);
        for exclude_pattern in exclude {
            let pattern = format!("!{}", exclude_pattern);
            override_builder.add(&pattern)?;
        }
        let override_matcher = override_builder.build()?;
        walk_builder.overrides(override_matcher);
    }

    let walker = walk_builder.build_parallel();

    // Create per-worker result collection using Arc + Mutex for thread safety
    let num_workers = threads.get();
    let best_matchers_per_worker: Vec<Arc<std::sync::Mutex<BestMatchesList>>> = (0..num_workers)
        .map(|_| Arc::new(std::sync::Mutex::new(BestMatchesList::new(limit.get()))))
        .collect();

    let index_counter = AtomicUsize::new(0);
    let total_match_count = Arc::new(AtomicUsize::new(0));
    let pattern_text = pattern_text.to_string();

    // Run parallel traversal
    walker.run(|| {
        let worker_id =
            index_counter.fetch_add(1, Ordering::Relaxed) % best_matchers_per_worker.len();
        let best_list = best_matchers_per_worker[worker_id].clone();
        let cancel_flag_clone = cancel_flag.clone();
        let pattern_text_clone = pattern_text.clone();
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

            // Skip directories
            if entry.metadata().map_or(true, |m| m.is_dir()) {
                return ignore::WalkState::Continue;
            }

            let path = match entry.path().to_str() {
                Some(p) => p,
                None => return ignore::WalkState::Continue,
            };

            // Try to add to results
            if let Ok(mut list) = best_list.lock() {
                if list.try_add(path, &pattern_text_clone).is_some() {
                    total_match_count_clone.fetch_add(1, Ordering::Relaxed);
                }
            }

            ignore::WalkState::Continue
        })
    });

    // Merge results from all workers
    let mut all_matches = Vec::new();
    for arc in best_matchers_per_worker {
        if let Ok(list) = arc.lock() {
            let matches = list.clone_matches();
            all_matches.extend(matches);
        }
    }

    // Sort by score (descending) and limit
    all_matches.sort_by(|a, b| b.0.cmp(&a.0));
    all_matches.truncate(limit.get());

    // Build final results
    let matches = all_matches
        .into_iter()
        .map(|(score, path)| FileMatch {
            score,
            path,
            indices: if compute_indices {
                Some(Vec::new())
            } else {
                None
            },
        })
        .collect();

    Ok(FileSearchResults {
        matches,
        total_match_count: total_match_count.as_ref().load(Ordering::Relaxed),
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
}
