use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

use super::FileIndex;

/// Result returned when a search pattern matches a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
    pub matches: Vec<String>,
}

/// Performs pattern searches across an index cache.
pub struct SearchEngine<'a> {
    index_cache: &'a HashMap<String, FileIndex>,
}

impl<'a> SearchEngine<'a> {
    /// Create a new engine bound to the provided index cache.
    pub fn new(index_cache: &'a HashMap<String, FileIndex>) -> Self {
        Self { index_cache }
    }

    /// Search indexed files and return every match for the provided regex pattern.
    pub fn search(&self, pattern: &str, path_filter: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for file_path in self.filtered_paths(path_filter) {
            if let Ok(content) = fs::read_to_string(file_path) {
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        let matches = regex
                            .find_iter(line)
                            .map(|m| m.as_str().to_string())
                            .collect();

                        results.push(SearchResult {
                            file_path: file_path.clone(),
                            line_number: line_num + 1,
                            line_content: line.to_string(),
                            matches,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    /// Grep-like helper that returns full lines for every match.
    pub fn grep(&self, pattern: &str, file_pattern: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for file_path in self.filtered_paths(file_pattern) {
            if let Ok(content) = fs::read_to_string(file_path) {
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        results.push(SearchResult {
                            file_path: file_path.clone(),
                            line_number: line_num + 1,
                            line_content: line.to_string(),
                            matches: vec![line.to_string()],
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    /// Return file paths whose names match the provided regex pattern.
    pub fn find_files(&self, pattern: &str) -> Result<Vec<String>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for file_path in self.index_cache.keys() {
            if regex.is_match(file_path) {
                results.push(file_path.clone());
            }
        }

        Ok(results)
    }

    fn filtered_paths<'b>(&'b self, filter: Option<&str>) -> impl Iterator<Item = &'b String> {
        self.index_cache.keys().filter(move |path| match filter {
            Some(filter) => path.contains(filter),
            None => true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index_cache(root: &str) -> HashMap<String, FileIndex> {
        let mut cache = HashMap::new();
        cache.insert(
            format!("{root}/main.rs"),
            FileIndex {
                path: format!("{root}/main.rs"),
                hash: "1".into(),
                modified: 0,
                size: 0,
                language: "rs".into(),
                tags: vec![],
            },
        );
        cache
    }

    #[test]
    fn search_returns_matches_and_highlights() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file_path = temp.path().join("main.rs");
        let cache = sample_index_cache(temp.path().to_str().unwrap());
        let engine = SearchEngine::new(&cache);

        let content = "fn main() { println!(\"hello\"); }";
        std::fs::write(&file_path, content).unwrap();

        let results = engine.search("main", None).expect("search succeeds");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matches, vec!["main".into()]);
    }

    #[test]
    fn find_files_filters_by_regex() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache = sample_index_cache(temp.path().to_str().unwrap());
        let engine = SearchEngine::new(&cache);
        let pattern = format!(
            "{}{}",
            regex::escape(temp.path().to_str().unwrap()),
            "/main\\.rs$"
        );
        let files = engine.find_files(&pattern).expect("find succeeds");
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
    }
}
