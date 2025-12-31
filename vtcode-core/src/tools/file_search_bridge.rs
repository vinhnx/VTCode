//! Bridge module integrating vtcode-file-search with grep_file.rs
//!
//! This module provides a clean interface to use the dedicated file-search
//! crate for file discovery operations, replacing direct ripgrep subprocess
//! calls for file enumeration.
//!
//! It handles:
//! - Converting between vtcode-core and vtcode-file-search APIs
//! - Integrating file search results with existing grep workflows
//! - Maintaining backward compatibility during transition

use anyhow::Result;
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use vtcode_file_search::{
    FileMatch, FileSearchResults, file_name_from_path, run as file_search_run,
};

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

    let limit = NonZero::new(config.max_results).unwrap_or(NonZero::new(100).unwrap());
    let threads = NonZero::new(config.num_threads).unwrap_or(NonZero::new(1).unwrap());

    file_search_run(
        &config.pattern,
        limit,
        &config.search_dir,
        config.exclude_patterns,
        threads,
        cancel,
        config.compute_indices,
        config.respect_gitignore,
    )
}

/// Get filename from a file match
///
/// Convenience wrapper around `file_name_from_path`
pub fn match_filename(file_match: &FileMatch) -> String {
    file_name_from_path(&file_match.path)
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
                .any(|ext| m.path.ends_with(&format!(".{}", ext)) || m.path.ends_with(ext))
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
                indices: None,
            },
            FileMatch {
                score: 90,
                path: "src/config.toml".to_string(),
                indices: None,
            },
            FileMatch {
                score: 80,
                path: "src/data.json".to_string(),
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
}
