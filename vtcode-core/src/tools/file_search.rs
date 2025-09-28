//! Recursive file search functionality for VTCode
//!
//! This module provides utilities for recursively searching files in a project workspace,
//! with support for glob patterns, exclusions, and content searching.

use anyhow::{Context, Result};
use glob::Pattern as GlobPattern;
use ignore::WalkBuilder;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern as FuzzyPattern};
use nucleo_matcher::{Matcher, Utf32Str};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

/// Configuration for file search operations
#[derive(Debug, Clone)]
pub struct FileSearchConfig {
    /// Maximum number of results to return
    pub max_results: usize,
    /// Whether to follow symbolic links
    pub follow_links: bool,
    /// Whether to include hidden files
    pub include_hidden: bool,
    /// File extensions to include (if empty, include all)
    pub include_extensions: HashSet<String>,
    /// File extensions to exclude
    pub exclude_extensions: HashSet<String>,
    /// File names/patterns to exclude
    pub exclude_patterns: Vec<GlobPattern>,
    /// Maximum file size in bytes (0 = no limit)
    pub max_file_size: u64,
}

impl Default for FileSearchConfig {
    fn default() -> Self {
        Self {
            max_results: 1000,
            follow_links: false,
            include_hidden: false,
            include_extensions: HashSet::new(),
            exclude_extensions: HashSet::new(),
            exclude_patterns: Vec::new(),
            max_file_size: 0,
        }
    }
}

/// Result of a file search operation
#[derive(Debug, Clone)]
pub struct FileSearchResult {
    /// Path to the file
    pub path: PathBuf,
    /// File name
    pub name: String,
    /// File extension
    pub extension: Option<String>,
    /// File size in bytes
    pub size: u64,
    /// Whether the file is a directory
    pub is_dir: bool,
    /// Content matches (if searched for content)
    pub content_matches: Vec<ContentMatch>,
}

/// A match found in file content
#[derive(Debug, Clone)]
pub struct ContentMatch {
    /// Line number (1-based)
    pub line_number: usize,
    /// Content of the line
    pub content: String,
    /// Column where match starts
    pub column: usize,
}

/// File searcher for recursive file operations
pub struct FileSearcher {
    root: PathBuf,
    config: FileSearchConfig,
}

impl FileSearcher {
    /// Create a new file searcher
    pub fn new(root: PathBuf, config: FileSearchConfig) -> Self {
        Self { root, config }
    }

    /// Create a searcher with default configuration
    pub fn with_default_config(root: PathBuf) -> Self {
        Self::new(root, FileSearchConfig::default())
    }

    fn build_walk_builder(&self) -> WalkBuilder {
        let mut builder = WalkBuilder::new(&self.root);
        builder.follow_links(self.config.follow_links);
        builder.hidden(!self.config.include_hidden);
        builder.require_git(false);
        builder.git_ignore(true);
        builder.git_global(true);
        builder.git_exclude(true);
        builder
    }

    fn relative_path_string(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    /// Recursively search for files matching the given pattern
    pub fn search_files(&self, pattern: Option<&str>) -> Result<Vec<FileSearchResult>> {
        let mut entries: Vec<(String, FileSearchResult)> = Vec::new();
        let max_results = self.config.max_results;
        let compiled_pattern = pattern.and_then(compile_fuzzy_pattern);

        for entry_result in self.build_walk_builder().build() {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            if entry.depth() == 0 {
                continue;
            }

            let file_type = match entry.file_type() {
                Some(file_type) => file_type,
                None => continue,
            };

            let metadata = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };

            if self.should_exclude_entry(entry.path(), Some(&file_type), &metadata)? {
                continue;
            }

            let path = entry.path();
            let result = FileSearchResult {
                path: path.to_path_buf(),
                name: path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
                extension: path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_string()),
                size: metadata.len(),
                is_dir: file_type.is_dir(),
                content_matches: Vec::new(),
            };

            let rel_path = self.relative_path_string(path);
            entries.push((rel_path, result));
        }

        if let Some(pattern) = compiled_pattern {
            let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
            let mut buffer = Vec::<char>::new();
            let mut scored = Vec::new();

            for (rel_path, result) in entries {
                buffer.clear();
                let haystack = Utf32Str::new(rel_path.as_str(), &mut buffer);
                if let Some(score) = pattern.score(haystack, &mut matcher) {
                    scored.push((score, rel_path, result));
                }
            }

            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            Ok(scored
                .into_iter()
                .take(max_results)
                .map(|(_, _, result)| result)
                .collect())
        } else {
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(entries
                .into_iter()
                .take(max_results)
                .map(|(_, result)| result)
                .collect())
        }
    }

    /// Search for files containing specific content
    pub fn search_files_with_content(
        &self,
        content_pattern: &str,
        file_pattern: Option<&str>,
    ) -> Result<Vec<FileSearchResult>> {
        let mut results = Vec::new();
        let max_results = self.config.max_results;
        for entry_result in self.build_walk_builder().build() {
            if results.len() >= max_results {
                break;
            }

            let entry = match entry_result {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            if entry.depth() == 0 {
                continue;
            }

            let path = entry.path();

            let file_type = match entry.file_type() {
                Some(file_type) if file_type.is_file() => file_type,
                _ => continue,
            };

            let metadata = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };

            if self.should_exclude_entry(path, Some(&file_type), &metadata)? {
                continue;
            }

            if let Some(pattern) = file_pattern
                && !self.path_matches_pattern(path, pattern)?
            {
                continue;
            }

            match self.search_content_in_file(path, content_pattern) {
                Ok(content_matches) => {
                    if content_matches.is_empty() {
                        continue;
                    }

                    let file_result = FileSearchResult {
                        path: path.to_path_buf(),
                        name: path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string(),
                        extension: path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| ext.to_string()),
                        size: metadata.len(),
                        is_dir: false,
                        content_matches,
                    };

                    results.push(file_result);
                }
                Err(_) => continue,
            }
        }

        Ok(results)
    }

    /// Find a specific file by name (recursively)
    pub fn find_file_by_name(&self, file_name: &str) -> Result<Option<PathBuf>> {
        for entry_result in self.build_walk_builder().build() {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            if entry.depth() == 0 {
                continue;
            }

            let path = entry.path();

            let file_type = match entry.file_type() {
                Some(file_type) => file_type,
                None => continue,
            };

            let metadata = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };

            if self.should_exclude_entry(path, Some(&file_type), &metadata)? {
                continue;
            }

            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name == file_name
            {
                return Ok(Some(path.to_path_buf()));
            }
        }

        Ok(None)
    }

    /// Check if a path should be excluded based on configuration
    fn should_exclude_entry(
        &self,
        path: &Path,
        file_type: Option<&std::fs::FileType>,
        metadata: &Metadata,
    ) -> Result<bool> {
        let path_str = path.to_string_lossy();

        let is_effective_file = metadata.is_file() || file_type.is_some_and(|ft| ft.is_file());

        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            let extension_lower = extension.to_lowercase();

            if self.config.exclude_extensions.contains(&extension_lower) {
                return Ok(true);
            }

            if !self.config.include_extensions.is_empty()
                && !self.config.include_extensions.contains(&extension_lower)
            {
                return Ok(true);
            }
        } else if !self.config.include_extensions.is_empty() && is_effective_file {
            return Ok(true);
        }

        for pattern in &self.config.exclude_patterns {
            if pattern.matches(path_str.as_ref()) {
                return Ok(true);
            }
        }

        if is_effective_file
            && self.config.max_file_size > 0
            && metadata.len() > self.config.max_file_size
        {
            return Ok(true);
        }

        Ok(false)
    }

    /// Check if a path matches a pattern
    fn path_matches_pattern(&self, path: &Path, pattern: &str) -> Result<bool> {
        if let Some(compiled) = compile_fuzzy_pattern(pattern) {
            let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
            let mut buffer = Vec::<char>::new();
            let relative = self.relative_path_string(path);
            let haystack = Utf32Str::new(relative.as_str(), &mut buffer);
            Ok(compiled.score(haystack, &mut matcher).is_some())
        } else {
            Ok(true)
        }
    }

    /// Search for content within a file
    fn search_content_in_file(&self, path: &Path, pattern: &str) -> Result<Vec<ContentMatch>> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        let mut matches = Vec::new();
        let pattern_lower = pattern.to_lowercase();

        for (line_num, line) in content.lines().enumerate() {
            let line_lower = line.to_lowercase();
            if line_lower.contains(&pattern_lower) {
                // Find all occurrences in the line
                let mut start = 0;
                while let Some(pos) = line_lower[start..].find(&pattern_lower) {
                    let actual_pos = start + pos;
                    matches.push(ContentMatch {
                        line_number: line_num + 1,
                        content: line.to_string(),
                        column: actual_pos,
                    });
                    start = actual_pos + pattern.len();
                }
            }
        }

        Ok(matches)
    }

    /// Convert search results to JSON format
    pub fn results_to_json(results: Vec<FileSearchResult>) -> Value {
        let json_results: Vec<Value> = results
            .into_iter()
            .map(|result| {
                json!({
                    "path": result.path.to_string_lossy(),
                    "name": result.name,
                    "extension": result.extension,
                    "size": result.size,
                    "is_dir": result.is_dir,
                    "content_matches": result.content_matches.iter().map(|m| json!({
                        "line_number": m.line_number,
                        "content": m.content,
                        "column": m.column,
                    })).collect::<Vec<Value>>()
                })
            })
            .collect();

        json!({
            "success": true,
            "results": json_results,
            "count": json_results.len()
        })
    }
}

fn compile_fuzzy_pattern(pattern: &str) -> Option<FuzzyPattern> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(FuzzyPattern::new(
            trimmed,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn collect_relative_paths(results: &[FileSearchResult], root: &Path) -> Vec<PathBuf> {
        results
            .iter()
            .filter_map(|result| result.path.strip_prefix(root).ok())
            .map(PathBuf::from)
            .collect()
    }

    #[test]
    fn test_file_searcher_creation() {
        let temp_dir = TempDir::new().unwrap();
        let searcher = FileSearcher::with_default_config(temp_dir.path().to_path_buf());
        assert_eq!(searcher.root, temp_dir.path());
    }

    #[test]
    fn test_find_file_by_name() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "test content").unwrap();

        let searcher = FileSearcher::with_default_config(temp_dir.path().to_path_buf());
        let result = searcher.find_file_by_name("test.txt")?;

        assert!(result.is_some());
        assert_eq!(result.unwrap(), test_file);

        Ok(())
    }

    #[test]
    fn test_search_files_without_pattern_returns_sorted_entries() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        fs::write(temp_dir.path().join("b_file.rs"), "content").unwrap();
        fs::write(temp_dir.path().join("a_file.txt"), "content").unwrap();
        fs::create_dir(temp_dir.path().join("subdir")).unwrap();
        fs::write(temp_dir.path().join("subdir").join("nested.txt"), "content").unwrap();

        let searcher = FileSearcher::with_default_config(temp_dir.path().to_path_buf());
        let results = searcher.search_files(None)?;

        let relative = collect_relative_paths(&results, temp_dir.path());
        let expected = vec![
            PathBuf::from("a_file.txt"),
            PathBuf::from("b_file.rs"),
            PathBuf::from("subdir"),
            PathBuf::from("subdir/nested.txt"),
        ];

        assert_eq!(relative, expected);

        Ok(())
    }

    #[test]
    fn test_search_files_uses_fuzzy_matching() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        fs::create_dir(temp_dir.path().join("src")).unwrap();
        fs::write(temp_dir.path().join("src").join("lib.rs"), "content").unwrap();
        fs::write(temp_dir.path().join("README.md"), "docs").unwrap();

        let searcher = FileSearcher::with_default_config(temp_dir.path().to_path_buf());
        let results = searcher.search_files(Some("srlb"))?;

        let file_paths: Vec<PathBuf> = results
            .into_iter()
            .filter(|result| !result.is_dir)
            .filter_map(|result| {
                result
                    .path
                    .strip_prefix(temp_dir.path())
                    .ok()
                    .map(PathBuf::from)
            })
            .collect();

        assert!(file_paths.contains(&PathBuf::from("src/lib.rs")));
        assert!(!file_paths.contains(&PathBuf::from("README.md")));

        Ok(())
    }

    #[test]
    fn test_search_files_respects_gitignore() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();

        fs::write(temp_dir.path().join(".gitignore"), "ignored/\n").unwrap();
        fs::create_dir(temp_dir.path().join("ignored")).unwrap();
        fs::write(temp_dir.path().join("ignored").join("skip.txt"), "skip").unwrap();
        fs::write(temp_dir.path().join("include.txt"), "include").unwrap();

        let searcher = FileSearcher::with_default_config(temp_dir.path().to_path_buf());
        let results = searcher.search_files(None)?;

        let relative = collect_relative_paths(&results, temp_dir.path());

        assert!(relative.contains(&PathBuf::from("include.txt")));
        assert!(!relative.contains(&PathBuf::from("ignored/skip.txt")));

        Ok(())
    }
}
