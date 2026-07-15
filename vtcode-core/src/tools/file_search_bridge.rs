//! Bridge module integrating vtcode-indexer::file_search with grep_file.rs
//!
//! This module provides a clean interface to use the file-search module
//! for file discovery operations, replacing direct ripgrep subprocess
//! calls for file enumeration.
//!
//! It handles:
//! - Converting between vtcode-core and vtcode-indexer::file_search APIs
//! - Integrating file search results with existing grep workflows

use anyhow::Result;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use vtcode_indexer::file_search::{
    FileMatch, FileSearchResults, MatchType, file_name_from_path, run as file_search_run,
    run_bounded_no_follow,
};

pub use vtcode_indexer::file_search::MatchType as FileMatchType;

/// Configuration for file search operations
#[derive(Debug, Clone)]
pub struct FileSearchConfig {
    /// Search pattern (fuzzy)
    pub pattern: String,
    /// Root directory to search
    pub search_dir: PathBuf,
    /// Patterns to exclude (glob-style)
    pub exclude_patterns: Vec<String>,
    /// Maximum number of results
    pub max_results: usize,
    /// Number of worker threads
    pub num_threads: usize,
    /// Whether to respect .gitignore files
    pub respect_gitignore: bool,
    /// Whether to compute character indices for highlighting
    pub compute_indices: bool,
}

impl FileSearchConfig {
    /// Create a new file search configuration
    pub fn new(pattern: String, search_dir: PathBuf) -> Self {
        Self {
            pattern,
            search_dir,
            exclude_patterns: vec![],
            max_results: 100,
            num_threads: num_cpus::get(),
            respect_gitignore: true,
            compute_indices: false,
        }
    }

    /// Add an exclusion pattern
    pub fn exclude(mut self, pattern: impl Into<String>) -> Self {
        self.exclude_patterns.push(pattern.into());
        self
    }

    /// Set maximum number of results
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.max_results = limit;
        self
    }

    /// Set number of threads
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.num_threads = threads.max(1);
        self
    }

    /// Enable/disable .gitignore support
    pub fn respect_gitignore(mut self, respect: bool) -> Self {
        self.respect_gitignore = respect;
        self
    }

    /// Enable character indices for highlighting
    pub fn compute_indices(mut self, compute: bool) -> Self {
        self.compute_indices = compute;
        self
    }
}

/// Search for files matching a pattern
///
/// # Arguments
///
/// * `config` - File search configuration
/// * `cancel_flag` - Optional cancellation flag for early termination
///
/// # Returns
///
/// FileSearchResults containing matched files
pub fn search_files(
    config: FileSearchConfig,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<FileSearchResults> {
    let cancel = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

    let limit = NonZero::new(config.max_results).unwrap_or(NonZero::<usize>::MIN);
    let threads = NonZero::new(config.num_threads).unwrap_or(NonZero::<usize>::MIN);

    file_search_run(vtcode_indexer::file_search::FileSearchConfig {
        pattern_text: config.pattern,
        limit,
        search_directory: config.search_dir,
        exclude: config.exclude_patterns,
        threads,
        cancel_flag: cancel,
        compute_indices: config.compute_indices,
        respect_gitignore: config.respect_gitignore,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundedPathSearch {
    pub paths: Vec<PathBuf>,
    pub truncated: bool,
}

/// Retrieve bounded fuzzy path candidates without exposing scores or following
/// symbolic links.
pub(crate) fn search_paths_bounded_no_follow(
    pattern: &str,
    search_dir: PathBuf,
    candidate_cap: usize,
) -> Result<BoundedPathSearch> {
    let limit = NonZero::new(candidate_cap).unwrap_or(NonZero::<usize>::MIN);
    let results = run_bounded_no_follow(vtcode_indexer::file_search::FileSearchConfig {
        pattern_text: pattern.to_string(),
        limit,
        search_directory: search_dir,
        exclude: Vec::new(),
        threads: NonZero::<usize>::MIN,
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: true,
    })?;
    let truncated = results.total_match_count >= candidate_cap;
    let paths = results
        .matches
        .into_iter()
        .filter(|candidate| candidate.match_type == MatchType::File)
        .map(|candidate| PathBuf::from(candidate.path))
        .collect();

    Ok(BoundedPathSearch { paths, truncated })
}

/// Get filename from a file match
///
/// Convenience wrapper around `file_name_from_path`
pub fn match_filename(file_match: &FileMatch) -> String {
    file_name_from_path(&file_match.path)
}

/// Keep only file matches, dropping directory entries.
pub fn file_matches_only(matches: Vec<FileMatch>) -> Vec<FileMatch> {
    matches
        .into_iter()
        .filter(|m| matches!(m.match_type, MatchType::File))
        .collect()
}

/// Filter file matches by file extension
///
/// # Arguments
///
/// * `matches` - Vector of file matches
/// * `extensions` - File extensions to keep (e.g., ["rs", "toml"])
pub fn filter_by_extension(matches: Vec<FileMatch>, extensions: &[&str]) -> Vec<FileMatch> {
    matches
        .into_iter()
        .filter(|m| {
            extensions
                .iter()
                .any(|ext| m.path.ends_with(&format!(".{ext}")) || m.path.ends_with(ext))
        })
        .collect()
}

/// Filter file matches by path pattern
///
/// # Arguments
///
/// * `matches` - Vector of file matches
/// * `path_pattern` - Glob pattern to match against paths
pub fn filter_by_pattern(matches: Vec<FileMatch>, path_pattern: &str) -> Vec<FileMatch> {
    if let Ok(pattern) = glob::Pattern::new(path_pattern) {
        matches
            .into_iter()
            .filter(|m| pattern.matches(&m.path))
            .collect()
    } else {
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn bounded_path_search_does_not_follow_directory_symlinks() {
        use std::fs;
        use std::os::unix::fs::symlink;
        use tempfile::TempDir;

        let workspace = TempDir::new().expect("workspace");
        let outside = TempDir::new().expect("outside directory");
        fs::write(workspace.path().join("WidgetLocal.rs"), "").expect("local file");
        fs::write(outside.path().join("WidgetOutside.rs"), "").expect("outside file");
        symlink(outside.path(), workspace.path().join("linked"))
            .expect("directory symlink fixture");

        let outcome = search_paths_bounded_no_follow("Widget", workspace.path().to_path_buf(), 20)
            .expect("bounded path search");

        assert!(outcome.paths.contains(&PathBuf::from("WidgetLocal.rs")));
        assert!(
            outcome
                .paths
                .iter()
                .all(|path| !path.ends_with("WidgetOutside.rs"))
        );
    }

    #[test]
    fn bounded_path_search_stops_at_candidate_cap() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        for index in 0..5 {
            std::fs::write(workspace.path().join(format!("Widget{index}.rs")), "")
                .expect("fixture file");
        }

        let outcome = search_paths_bounded_no_follow("Widget", workspace.path().to_path_buf(), 2)
            .expect("bounded path search");

        assert_eq!(outcome.paths.len(), 2);
        assert!(outcome.truncated);
    }

    #[test]
    fn bounded_path_search_does_not_spend_file_capacity_on_directories() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let nested = workspace.path().join("WidgetParent").join("WidgetChild");
        std::fs::create_dir_all(&nested).expect("matching directories");
        std::fs::write(nested.join("TargetWidget.rs"), "").expect("matching file");

        let outcome = search_paths_bounded_no_follow("Widget", workspace.path().to_path_buf(), 1)
            .expect("bounded path search");

        assert_eq!(outcome.paths.len(), 1);
        assert!(outcome.paths[0].ends_with("TargetWidget.rs"));
    }

    #[test]
    fn test_file_search_config_builder() {
        let config = FileSearchConfig::new("test".to_string(), PathBuf::from("."))
            .exclude("target/**")
            .with_limit(50)
            .with_threads(4);

        assert_eq!(config.pattern, "test");
        assert_eq!(config.max_results, 50);
        assert_eq!(config.num_threads, 4);
        assert_eq!(config.exclude_patterns.len(), 1);
    }

    #[test]
    fn test_match_filename() {
        let file_match = FileMatch {
            score: 100,
            path: "src/utils/helper.rs".to_string(),
            match_type: MatchType::File,
            indices: None,
        };

        assert_eq!(match_filename(&file_match), "helper.rs");
    }

    #[test]
    fn test_filter_by_extension() {
        let matches = vec![
            FileMatch {
                score: 100,
                path: "src/main.rs".to_string(),
                match_type: MatchType::File,
                indices: None,
            },
            FileMatch {
                score: 90,
                path: "src/config.toml".to_string(),
                match_type: MatchType::File,
                indices: None,
            },
            FileMatch {
                score: 80,
                path: "src/data.json".to_string(),
                match_type: MatchType::File,
                indices: None,
            },
        ];

        let filtered = filter_by_extension(matches, &["rs", "toml"]);
        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .all(|m| { m.path.ends_with(".rs") || m.path.ends_with(".toml") })
        );
    }

    #[test]
    fn test_file_matches_only_filters_directories() {
        let matches = vec![
            FileMatch {
                score: 100,
                path: "src".to_string(),
                match_type: MatchType::Directory,
                indices: None,
            },
            FileMatch {
                score: 90,
                path: "src/main.rs".to_string(),
                match_type: MatchType::File,
                indices: None,
            },
        ];

        let filtered = file_matches_only(matches);

        assert_eq!(filtered.len(), 1);
        assert!(matches!(filtered[0].match_type, MatchType::File));
    }
}
