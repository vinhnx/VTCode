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
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

pub use vtcode_commons::paths::file_name_from_path;

/// Best matches list per worker thread (lock-free collection).
///
/// Each worker thread gets its own instance to avoid locking during
/// directory traversal. Results are merged at the end.
struct BestMatchesList {
    matches: BinaryHeap<Reverse<(u32, String)>>,
    limit: usize,
    matcher: nucleo_matcher::Matcher,
    haystack_buf: Vec<char>,
    pattern_buf: Vec<char>,
    pattern_text: String,
}

impl BestMatchesList {
    fn new(limit: usize, pattern_text: &str) -> Self {
        Self {
            matches: BinaryHeap::new(),
            limit,
            matcher: nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT),
            haystack_buf: Vec::with_capacity(256),
            pattern_buf: Vec::with_capacity(pattern_text.len()),
            pattern_text: pattern_text.to_string(),
        }
    }

    /// Record a matching path while preserving the worker-local top-K heap.
    ///
    /// Returns true when the path matches the search pattern, even if it
    /// does not survive the top-K cutoff.
    fn record_match(&mut self, path: &str) -> bool {
        let haystack = nucleo_matcher::Utf32Str::new(path, &mut self.haystack_buf);
        let needle = nucleo_matcher::Utf32Str::new(&self.pattern_text, &mut self.pattern_buf);
        let Some(score) = self.matcher.fuzzy_match(haystack, needle) else {
            return false;
        };

        push_top_match(&mut self.matches, self.limit, score as u32, path.to_string());
        true
    }
}

fn push_top_match(
    matches: &mut BinaryHeap<Reverse<(u32, String)>>,
    limit: usize,
    score: u32,
    path: String,
) -> bool {
    if matches.len() < limit {
        matches.push(Reverse((score, path)));
        return true;
    }

    let Some(min_score) = matches.peek().map(|entry| entry.0.0) else {
        return false;
    };

    if score <= min_score {
        return false;
    }

    matches.pop();
    matches.push(Reverse((score, path)));
    true
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
    // Store pattern text for cloning across threads
    // (Pattern is parsed per-match to work with Utf32Str)

    // Build the directory walker
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
    let best_matchers_per_worker: Vec<Arc<Mutex<BestMatchesList>>> = (0..threads)
        .map(|_| {
            Arc::new(Mutex::new(BestMatchesList::new(limit, &config.pattern_text)))
        })
        .collect();

    let index_counter = AtomicUsize::new(0);
    let total_match_count = Arc::new(AtomicUsize::new(0));

    // Run parallel traversal
    walker.run(|| {
        let worker_id =
            index_counter.fetch_add(1, Ordering::Relaxed) % best_matchers_per_worker.len();
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

            // Skip directories
            if entry.metadata().map_or(true, |m| m.is_dir()) {
                return ignore::WalkState::Continue;
            }

            let path = match entry.path().to_str() {
                Some(p) => p,
                None => return ignore::WalkState::Continue,
            };

            // Try to add to results
            {
                let mut list = best_list.lock();
                if list.record_match(path) {
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
        for Reverse((score, path)) in std::mem::take(&mut list.matches).into_vec() {
            push_top_match(&mut merged_matches, limit, score, path);
        }
    }

    // Build final results
    let matches = merged_matches
        .into_sorted_vec()
        .into_iter()
        .map(|Reverse((score, path))| FileMatch {
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
