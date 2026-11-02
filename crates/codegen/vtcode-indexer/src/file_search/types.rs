use serde::{Deserialize, Serialize};
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// A single file match result.
///
/// Fields:
/// - `score`: Relevance score from fuzzy matching (higher is better)
/// - `path`: Path relative to the search directory
/// - `match_type`: Whether the match is a file or directory
/// - `indices`: Optional character positions for highlighting matched characters
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum MatchType {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMatch {
    pub score: u32,
    pub path: String,
    pub match_type: MatchType,
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
