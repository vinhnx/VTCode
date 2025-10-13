use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
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
        Ok(self.search_iter(pattern, path_filter)?.collect::<Vec<_>>())
    }

    /// Return a streaming iterator over regex matches.
    pub fn search_iter(
        &self,
        pattern: &str,
        path_filter: Option<&str>,
    ) -> Result<SearchResultsIter> {
        let regex = Regex::new(pattern)?;
        let file_paths = self
            .filtered_paths(path_filter)
            .cloned()
            .collect::<Vec<_>>();

        Ok(SearchResultsIter::new(
            regex,
            file_paths,
            SearchMode::Matches,
        ))
    }

    /// Grep-like helper that returns full lines for every match.
    pub fn grep(&self, pattern: &str, file_pattern: Option<&str>) -> Result<Vec<SearchResult>> {
        Ok(self.grep_iter(pattern, file_pattern)?.collect::<Vec<_>>())
    }

    /// Grep-like iterator that yields matches lazily.
    pub fn grep_iter(
        &self,
        pattern: &str,
        file_pattern: Option<&str>,
    ) -> Result<SearchResultsIter> {
        let regex = Regex::new(pattern)?;
        let file_paths = self
            .filtered_paths(file_pattern)
            .cloned()
            .collect::<Vec<_>>();

        Ok(SearchResultsIter::new(regex, file_paths, SearchMode::Lines))
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

/// Streaming iterator over search results that can be sent between threads.
#[derive(Debug)]
pub struct SearchResultsIter {
    regex: Regex,
    mode: SearchMode,
    pending_paths: VecDeque<String>,
    current_file: Option<FileState>,
}

#[derive(Debug)]
struct FileState {
    path: String,
    lines: Vec<String>,
    next_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Matches,
    Lines,
}

impl SearchResultsIter {
    fn new(regex: Regex, file_paths: Vec<String>, mode: SearchMode) -> Self {
        Self {
            regex,
            mode,
            pending_paths: file_paths.into(),
            current_file: None,
        }
    }

    fn prepare_next_file(&mut self) {
        while self.current_file.is_none() {
            let path = match self.pending_paths.pop_front() {
                Some(path) => path,
                None => break,
            };

            match fs::read_to_string(&path) {
                Ok(content) => {
                    let lines = content.lines().map(ToString::to_string).collect();
                    self.current_file = Some(FileState {
                        path,
                        lines,
                        next_line: 0,
                    });
                }
                Err(_) => continue,
            }
        }
    }

    fn next_from_current_file(&mut self) -> Option<SearchResult> {
        let file_state = self.current_file.as_mut()?;

        while file_state.next_line < file_state.lines.len() {
            let line_number = file_state.next_line + 1;
            let line_content = file_state.lines[file_state.next_line].clone();
            file_state.next_line += 1;

            if self.regex.is_match(&line_content) {
                let matches = match self.mode {
                    SearchMode::Matches => self
                        .regex
                        .find_iter(&line_content)
                        .map(|m| m.as_str().to_string())
                        .collect(),
                    SearchMode::Lines => vec![line_content.clone()],
                };

                return Some(SearchResult {
                    file_path: file_state.path.clone(),
                    line_number,
                    line_content,
                    matches,
                });
            }
        }

        self.current_file = None;
        None
    }
}

impl Iterator for SearchResultsIter {
    type Item = SearchResult;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.current_file.is_none() {
                self.prepare_next_file();
            }

            if self.current_file.is_none() {
                return None;
            }

            if let Some(result) = self.next_from_current_file() {
                return Some(result);
            }
        }
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

    #[test]
    fn streaming_iterator_matches_collect_results() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file_path = temp.path().join("lib.rs");
        let cache = sample_index_cache(temp.path().to_str().unwrap());
        let engine = SearchEngine::new(&cache);

        std::fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let mut iter = engine
            .search_iter("println", None)
            .expect("iterator builds");

        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SearchResultsIter>();

        let first = iter.next().expect("first result");
        assert_eq!(first.line_number, 2);
        assert_eq!(first.matches, vec!["println".into()]);
        assert!(iter.next().is_none());
    }

    #[test]
    fn grep_iterator_returns_full_lines() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file_path = temp.path().join("main.rs");
        let cache = sample_index_cache(temp.path().to_str().unwrap());
        let engine = SearchEngine::new(&cache);

        std::fs::write(&file_path, "fn main() {\n    // TODO\n}\n").unwrap();

        let results = engine
            .grep_iter("TODO", None)
            .expect("iterator")
            .collect::<Vec<_>>();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_content.trim(), "// TODO");
        assert_eq!(results[0].matches, vec!["// TODO".into()]);
    }
}
