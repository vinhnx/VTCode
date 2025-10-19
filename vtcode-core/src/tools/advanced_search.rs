//! Advanced search tools with enhanced case-insensitive capabilities

use super::traits::Tool;
use crate::tools::grep_search::GrepSearchManager;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Enhanced search tool with advanced case-insensitive features
pub struct AdvancedSearchTool {
    workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub include_hidden: bool,
    pub max_results: usize,
    pub context_lines: usize,
    pub file_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            whole_word: false,
            regex: false,
            include_hidden: false,
            max_results: 100,
            context_lines: 0,
            file_patterns: vec![],
            exclude_patterns: vec![],
        }
    }
}

impl AdvancedSearchTool {
    pub fn new(workspace_root: PathBuf, _grep_search: Arc<GrepSearchManager>) -> Self {
        // grep_file manager is unused; keep constructor signature for compatibility but drop the field
        Self { workspace_root }
    }

    /// Perform advanced search with multiple options
    pub async fn search(&self, query: &str, path: &str, options: SearchOptions) -> Result<Value> {
        let results = if options.regex {
            // Use regex search
            self.regex_search(query, path, &options).await?
        } else {
            // Use pattern-based search
            self.pattern_search(query, path, &options).await?
        };

        // Apply post-processing filters
        let filtered_results = self.apply_filters(results, &options);

        Ok(json!({
            "success": true,
            "query": query,
            "path": path,
            "options": {
                "case_sensitive": options.case_sensitive,
                "whole_word": options.whole_word,
                "regex": options.regex,
                "include_hidden": options.include_hidden,
                "max_results": options.max_results,
                "context_lines": options.context_lines,
                "file_patterns": options.file_patterns,
                "exclude_patterns": options.exclude_patterns
            },
            "results": filtered_results,
            "total_matches": filtered_results.len()
        }))
    }

    /// Perform regex-based search
    async fn regex_search(
        &self,
        pattern: &str,
        path: &str,
        options: &SearchOptions,
    ) -> Result<Vec<Value>> {
        let regex_flags = if options.case_sensitive { "" } else { "(?i)" };
        let regex_pattern = if options.whole_word {
            format!(r"{}\b{}\b", regex_flags, regex::escape(pattern))
        } else {
            format!(r"{}{}", regex_flags, pattern)
        };

        let regex =
            Regex::new(&regex_pattern).map_err(|e| anyhow!("Invalid regex pattern: {}", e))?;

        let mut results = Vec::new();
        let search_path = self.workspace_root.join(path);

        self.search_files_recursive(&search_path, &regex, options, &mut results)
            .await?;

        Ok(results)
    }

    /// Perform pattern-based search
    async fn pattern_search(
        &self,
        pattern: &str,
        path: &str,
        options: &SearchOptions,
    ) -> Result<Vec<Value>> {
        let search_pattern = if options.whole_word {
            format!(r"\b{}\b", regex::escape(pattern))
        } else {
            regex::escape(pattern)
        };

        let regex_flags = if options.case_sensitive { "" } else { "(?i)" };
        let regex_pattern = format!(r"{}{}", regex_flags, search_pattern);

        let regex =
            Regex::new(&regex_pattern).map_err(|e| anyhow!("Invalid search pattern: {}", e))?;

        let mut results = Vec::new();
        let search_path = self.workspace_root.join(path);

        self.search_files_recursive(&search_path, &regex, options, &mut results)
            .await?;

        Ok(results)
    }

    /// Recursively search files
    async fn search_files_recursive(
        &self,
        dir: &Path,
        regex: &Regex,
        options: &SearchOptions,
        results: &mut Vec<Value>,
    ) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Skip hidden files unless explicitly included
            if !options.include_hidden
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with('.'))
                    .unwrap_or(false)
            {
                continue;
            }

            if path.is_dir() {
                // Skip common directories that shouldn't be searched
                if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                    if matches!(
                        dir_name,
                        "node_modules" | ".git" | "target" | "__pycache__" | ".next"
                    ) {
                        continue;
                    }
                }

                // Recurse into subdirectories
                Box::pin(self.search_files_recursive(&path, regex, options, results)).await?;
            } else if path.is_file() {
                // Check file pattern filters
                if !self.matches_file_patterns(&path, options) {
                    continue;
                }

                // Search file content
                match self.search_file_content(&path, regex, options).await {
                    Ok(file_results) => {
                        results.extend(file_results);
                        // file_count += 1; // Not used, so removed

                        // Check if we've hit the max results limit
                        if results.len() >= options.max_results {
                            break;
                        }
                    }
                    Err(_) => continue, // Skip files that can't be read
                }
            }
        }

        Ok(())
    }

    /// Search content of a single file
    async fn search_file_content(
        &self,
        file_path: &Path,
        regex: &Regex,
        options: &SearchOptions,
    ) -> Result<Vec<Value>> {
        let content = tokio::fs::read_to_string(file_path).await?;
        let lines: Vec<&str> = content.lines().collect();
        let mut file_results = Vec::new();

        for (line_num, line) in lines.iter().enumerate() {
            if regex.is_match(line) {
                let result = json!({
                    "file": file_path.strip_prefix(&self.workspace_root)
                        .unwrap_or(file_path)
                        .to_string_lossy(),
                    "line": line_num + 1,
                    "content": line.trim(),
                    "context": if options.context_lines > 0 {
                        self.get_context_lines(&lines, line_num, options.context_lines)
                    } else {
                        Value::Null
                    }
                });

                file_results.push(result);

                if file_results.len() >= options.max_results {
                    break;
                }
            }
        }

        Ok(file_results)
    }

    /// Get context lines around a match
    fn get_context_lines(&self, lines: &[&str], match_line: usize, context_lines: usize) -> Value {
        let start = match_line.saturating_sub(context_lines);
        let end = (match_line + context_lines + 1).min(lines.len());

        let context: Vec<Value> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num = start + i + 1;
                let is_match = line_num == match_line + 1;
                json!({
                    "line": line_num,
                    "content": line.trim(),
                    "is_match": is_match
                })
            })
            .collect();

        Value::Array(context)
    }

    /// Check if file matches the specified patterns
    fn matches_file_patterns(&self, file_path: &Path, options: &SearchOptions) -> bool {
        if options.file_patterns.is_empty() {
            return true;
        }

        let file_name = file_path.to_string_lossy();

        for pattern in &options.file_patterns {
            if self.matches_glob_pattern(&file_name, pattern) {
                return true;
            }
        }

        false
    }

    /// Simple glob pattern matching
    fn matches_glob_pattern(&self, text: &str, pattern: &str) -> bool {
        if pattern.contains('*') {
            let regex_pattern = pattern
                .replace('.', r"\.")
                .replace('*', ".*")
                .replace('?', ".");
            Regex::new(&format!("^{}$", regex_pattern))
                .map(|r| r.is_match(text))
                .unwrap_or(false)
        } else {
            text.contains(pattern)
        }
    }

    /// Apply post-processing filters to results
    fn apply_filters(&self, results: Vec<Value>, options: &SearchOptions) -> Vec<Value> {
        let mut filtered = results;

        // Apply exclude patterns
        if !options.exclude_patterns.is_empty() {
            filtered = filtered
                .into_iter()
                .filter(|result| {
                    if let Some(file) = result.get("file").and_then(|f| f.as_str()) {
                        !options
                            .exclude_patterns
                            .iter()
                            .any(|pattern| self.matches_glob_pattern(file, pattern))
                    } else {
                        true
                    }
                })
                .collect();
        }

        // Limit results
        if filtered.len() > options.max_results {
            filtered.truncate(options.max_results);
        }

        filtered
    }

    /// Perform case-insensitive search with smart defaults
    pub async fn smart_search(&self, query: &str, path: &str) -> Result<Value> {
        let options = SearchOptions {
            case_sensitive: false,
            whole_word: false,
            regex: false,
            include_hidden: false,
            max_results: 50,
            context_lines: 2,
            file_patterns: vec![],
            exclude_patterns: vec![
                "*.log".to_string(),
                "*.min.js".to_string(),
                "*.min.css".to_string(),
                "node_modules/**".to_string(),
                ".git/**".to_string(),
                "target/**".to_string(),
            ],
        };

        self.search(query, path, options).await
    }

    /// Search for multiple terms with case-insensitive matching
    pub async fn multi_term_search(
        &self,
        terms: &[String],
        path: &str,
        require_all: bool,
    ) -> Result<Value> {
        let mut all_results = Vec::new();
        let mut term_matches = HashMap::new();

        // Search for each term
        for term in terms {
            let result = self.smart_search(term, path).await?;
            if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                term_matches.insert(term.clone(), results.clone());
                all_results.extend(results.clone());
            }
        }

        // Filter results based on require_all flag
        let filtered_results = if require_all {
            self.filter_require_all(all_results, &term_matches, terms)
        } else {
            self.deduplicate_results(all_results)
        };

        Ok(json!({
            "success": true,
            "query_terms": terms,
            "require_all": require_all,
            "results": filtered_results,
            "total_matches": filtered_results.len()
        }))
    }

    /// Filter results to only include files that contain all search terms
    fn filter_require_all(
        &self,
        results: Vec<Value>,
        term_matches: &HashMap<String, Vec<Value>>,
        terms: &[String],
    ) -> Vec<Value> {
        let mut file_groups: HashMap<String, Vec<Value>> = HashMap::new();

        // Group results by file
        for result in results {
            if let Some(file) = result.get("file").and_then(|f| f.as_str()) {
                file_groups
                    .entry(file.to_string())
                    .or_insert_with(Vec::new)
                    .push(result);
            }
        }

        // Filter files that contain all terms
        file_groups
            .into_iter()
            .filter(|(_, file_results)| {
                let _file_path = file_results
                    .first()
                    .and_then(|r| r.get("file"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("");

                terms.iter().all(|term| {
                    file_results.iter().any(|result| {
                        result
                            .get("content")
                            .and_then(|c| c.as_str())
                            .map(|content| {
                                if term_matches.contains_key(term) {
                                    content.to_lowercase().contains(&term.to_lowercase())
                                } else {
                                    false
                                }
                            })
                            .unwrap_or(false)
                    })
                })
            })
            .flat_map(|(_, results)| results)
            .collect()
    }

    /// Remove duplicate results
    fn deduplicate_results(&self, results: Vec<Value>) -> Vec<Value> {
        let mut seen = std::collections::HashSet::new();

        results
            .into_iter()
            .filter(|result| {
                let key = format!(
                    "{}:{}",
                    result.get("file").and_then(|f| f.as_str()).unwrap_or(""),
                    result.get("line").and_then(|l| l.as_u64()).unwrap_or(0)
                );

                seen.insert(key)
            })
            .collect()
    }
}

#[async_trait]
impl Tool for AdvancedSearchTool {
    fn name(&self) -> &'static str {
        "advanced_search"
    }

    fn description(&self) -> &'static str {
        "Advanced search tool with case-insensitive matching, regex support, and smart filtering"
    }

    async fn execute(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or_else(|| anyhow!("Missing query parameter"))?;

        let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");

        let options = SearchOptions {
            case_sensitive: args
                .get("case_sensitive")
                .and_then(|c| c.as_bool())
                .unwrap_or(false),
            whole_word: args
                .get("whole_word")
                .and_then(|w| w.as_bool())
                .unwrap_or(false),
            regex: args.get("regex").and_then(|r| r.as_bool()).unwrap_or(false),
            include_hidden: args
                .get("include_hidden")
                .and_then(|h| h.as_bool())
                .unwrap_or(false),
            max_results: args
                .get("max_results")
                .and_then(|m| m.as_u64())
                .unwrap_or(100) as usize,
            context_lines: args
                .get("context_lines")
                .and_then(|c| c.as_u64())
                .unwrap_or(0) as usize,
            file_patterns: args
                .get("file_patterns")
                .and_then(|fp| fp.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            exclude_patterns: args
                .get("exclude_patterns")
                .and_then(|ep| ep.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        };

        self.search(query, path, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_case_insensitive_search() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();

        // Create test file
        let test_file = workspace_root.join("test.txt");
        tokio::fs::write(&test_file, "Hello World\nHELLO world\nhello WORLD")
            .await
            .unwrap();

        let grep_file = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let search_tool = AdvancedSearchTool::new(workspace_root, grep_file);

        let options = SearchOptions {
            case_sensitive: false,
            ..Default::default()
        };

        let result = search_tool.search("hello", ".", options).await.unwrap();
        let results = result.get("results").unwrap().as_array().unwrap();

        assert_eq!(results.len(), 3); // Should match all 3 lines
    }

    #[tokio::test]
    async fn test_whole_word_search() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();

        // Create test file
        let test_file = workspace_root.join("test.txt");
        tokio::fs::write(&test_file, "hello world\nhelloworld\nhello-world")
            .await
            .unwrap();

        let grep_file = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let search_tool = AdvancedSearchTool::new(workspace_root, grep_file);

        let options = SearchOptions {
            case_sensitive: false,
            whole_word: true,
            ..Default::default()
        };

        let result = search_tool.search("hello", ".", options).await.unwrap();
        let results = result.get("results").unwrap().as_array().unwrap();

        assert_eq!(results.len(), 1); // Should only match "hello world"
    }
}
