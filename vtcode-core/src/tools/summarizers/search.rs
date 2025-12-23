//! Search result summarization
//!
//! Summarizes grep_file and list_files outputs from full match listings
//! into concise summaries suitable for LLM context.
//!
//! ## Strategy
//!
//! Instead of sending all 127 matches across 2,500 tokens, send:
//! "Found 127 matches in 15 files. Key files: src/tools/grep.rs (3 matches),
//! src/tools/list.rs (1 match). Pattern in: execute_grep(), grep_impl() functions"
//!
//! Target: ~50 tokens vs 2,500 tokens = 98% savings

use super::{Summarizer, truncate_to_tokens};
use anyhow::Result;
use std::collections::HashMap;

/// Summarizer for grep_file results
pub struct GrepSummarizer {
    /// Maximum number of files to list in summary
    pub max_files: usize,
    /// Maximum number of functions/symbols to mention
    pub max_symbols: usize,
    /// Maximum tokens for entire summary
    pub max_tokens: usize,
}

impl Default for GrepSummarizer {
    fn default() -> Self {
        Self {
            max_files: 5,
            max_symbols: 5,
            max_tokens: 100,
        }
    }
}

impl Summarizer for GrepSummarizer {
    fn summarize(
        &self,
        full_output: &str,
        _metadata: Option<&serde_json::Value>,
    ) -> Result<String> {
        // Parse grep output to extract key information
        let stats = parse_grep_output(full_output);

        // Build concise summary
        let mut summary = format!(
            "Found {} matches in {} files",
            stats.total_matches, stats.unique_files
        );

        // Add top files if available
        if !stats.top_files.is_empty() {
            let file_list: Vec<String> = stats
                .top_files
                .iter()
                .take(self.max_files)
                .map(|(file, count)| format!("{} ({})", file, count))
                .collect();
            summary.push_str(&format!(". Key files: {}", file_list.join(", ")));
        }

        // Add pattern context if available
        if !stats.symbols.is_empty() {
            let symbol_list: Vec<&str> = stats
                .symbols
                .iter()
                .take(self.max_symbols)
                .map(|s| s.as_str())
                .collect();
            summary.push_str(&format!(". Pattern in: {}", symbol_list.join(", ")));
        }

        // Truncate to token limit
        Ok(truncate_to_tokens(&summary, self.max_tokens))
    }
}

/// Summarizer for list_files results
pub struct ListSummarizer {
    pub max_dirs: usize,
    pub max_files: usize,
    pub max_tokens: usize,
}

impl Default for ListSummarizer {
    fn default() -> Self {
        Self {
            max_dirs: 3,
            max_files: 10,
            max_tokens: 80,
        }
    }
}

impl Summarizer for ListSummarizer {
    fn summarize(
        &self,
        full_output: &str,
        _metadata: Option<&serde_json::Value>,
    ) -> Result<String> {
        let stats = parse_list_output(full_output);

        let mut summary = format!(
            "Listed {} items ({} files, {} directories)",
            stats.total_items, stats.file_count, stats.dir_count
        );

        // Add sample files if available
        if !stats.sample_files.is_empty() {
            let files: Vec<&str> = stats
                .sample_files
                .iter()
                .take(self.max_files)
                .map(|s| s.as_str())
                .collect();
            summary.push_str(&format!(". Files: {}", files.join(", ")));
        }

        Ok(truncate_to_tokens(&summary, self.max_tokens))
    }
}

/// Statistics extracted from grep output
#[derive(Debug, Default)]
struct GrepStats {
    total_matches: usize,
    unique_files: usize,
    top_files: Vec<(String, usize)>, // (filename, match_count)
    symbols: Vec<String>,            // function names, identifiers
}

/// Statistics extracted from list output
#[derive(Debug, Default)]
struct ListStats {
    total_items: usize,
    file_count: usize,
    dir_count: usize,
    sample_files: Vec<String>,
}

/// Parse grep output to extract statistics
fn parse_grep_output(output: &str) -> GrepStats {
    let mut stats = GrepStats::default();
    let mut file_matches: HashMap<String, usize> = HashMap::new();
    let mut symbols_set: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in output.lines() {
        stats.total_matches += 1;

        // Extract filename (format: "path/file.rs:42:content")
        if let Some(colon_pos) = line.find(':') {
            let file = &line[..colon_pos];
            if !file.is_empty() {
                *file_matches.entry(file.to_string()).or_insert(0) += 1;

                // Extract simple filename for display
                if let Some(slash_pos) = file.rfind('/') {
                    let filename = &file[slash_pos + 1..];
                    if filename.len() < 30 {
                        // reasonable filename length
                        *file_matches.entry(filename.to_string()).or_insert(0) += 1;
                    }
                }
            }

            // Extract potential symbols (functions, methods)
            // Look for patterns like "fn name(", "impl Name", "pub struct"
            let content = &line[colon_pos..];
            extract_symbols(content, &mut symbols_set);
        }
    }

    stats.unique_files = file_matches.len();

    // Sort files by match count (descending)
    let mut sorted_files: Vec<(String, usize)> = file_matches.into_iter().collect();
    sorted_files.sort_by(|a, b| b.1.cmp(&a.1));
    stats.top_files = sorted_files.into_iter().take(10).collect();

    stats.symbols = symbols_set.into_iter().take(10).collect();

    stats
}

/// Parse list output to extract statistics
fn parse_list_output(output: &str) -> ListStats {
    let mut stats = ListStats::default();

    for line in output.lines() {
        stats.total_items += 1;

        // Detect directories (usually end with / or marked with [dir])
        if line.ends_with('/') || line.contains("[dir]") || line.contains("DIR") {
            stats.dir_count += 1;
        } else {
            stats.file_count += 1;
            // Extract simple filename
            if let Some(name) = line.split('/').next_back()
                && !name.is_empty() && name.len() < 50 {
                    stats.sample_files.push(name.to_string());
                }
        }
    }

    stats
}

/// Extract potential symbols (function names, types) from code line
fn extract_symbols(line: &str, symbols: &mut std::collections::HashSet<String>) {
    // Look for function definitions: "fn name(" or "async fn name("
    if let Some(fn_pos) = line.find("fn ") {
        let after_fn = &line[fn_pos + 3..];
        if let Some(paren_pos) = after_fn.find('(') {
            let name = after_fn[..paren_pos].trim();
            if !name.is_empty() && name.len() < 30 {
                symbols.insert(format!("{}()", name));
            }
        }
    }

    // Look for struct/impl/trait definitions
    for keyword in &["struct ", "impl ", "trait ", "enum "] {
        if let Some(pos) = line.find(keyword) {
            let after_kw = &line[pos + keyword.len()..];
            if let Some(first_word) = after_kw.split_whitespace().next()
                && first_word.len() < 30 && !first_word.contains('{') {
                    symbols.insert(first_word.to_string());
                }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::estimate_tokens;
    use super::*;

    #[test]
    fn test_grep_summarizer() {
        let full_output = "\
src/tools/grep.rs:45:    pub fn execute_grep(pattern: &str) -> Result<String> {
src/tools/grep.rs:67:        let matches = grep_impl(pattern)?;
src/tools/grep.rs:89:    fn grep_impl(pattern: &str) -> Result<Vec<Match>> {
src/tools/list.rs:23:    // Uses grep internally for filtering
src/main.rs:100:    grep.execute(\"test\")?;
";

        let summarizer = GrepSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        assert!(summary.contains("Found 5 matches"));
        assert!(summary.contains("files"));
        assert!(estimate_tokens(&summary) < 100);

        // Verify savings
        let (llm, ui, pct) = summarizer.estimate_savings(full_output, &summary);
        assert!(
            pct > 20.0,
            "Should save >20% (got {:.1}%, {} → {} tokens)",
            pct,
            ui,
            llm
        );
        assert!(llm < ui);
    }

    #[test]
    fn test_list_summarizer() {
        let full_output = "\
src/main.rs
src/lib.rs
src/tools/
src/tools/grep.rs
src/tools/list.rs
tests/
tests/integration.rs
README.md
";

        let summarizer = ListSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        assert!(summary.contains("Listed 8 items"));
        assert!(summary.contains("files"));
        assert!(summary.contains("directories"));
        assert!(estimate_tokens(&summary) < 100);
    }

    #[test]
    fn test_grep_stats_parsing() {
        let output = "\
src/tools/grep.rs:45:    pub fn execute_grep(pattern: &str) -> Result<String> {
src/tools/grep.rs:67:        let matches = grep_impl(pattern)?;
src/tools/list.rs:23:    // comment
";

        let stats = parse_grep_output(output);

        assert_eq!(stats.total_matches, 3);
        assert!(stats.unique_files > 0);
        assert!(!stats.top_files.is_empty());
    }

    #[test]
    fn test_symbol_extraction() {
        let mut symbols = std::collections::HashSet::new();

        extract_symbols("    pub fn execute_grep(pattern: &str)", &mut symbols);
        assert!(symbols.contains("execute_grep()"));

        extract_symbols("impl GrepTool {", &mut symbols);
        assert!(symbols.contains("GrepTool"));

        extract_symbols("pub struct MyStruct {", &mut symbols);
        assert!(symbols.contains("MyStruct"));
    }

    #[test]
    fn test_list_stats_parsing() {
        let output = "file1.rs\nfile2.rs\nsrc/\ntests/\nREADME.md";
        let stats = parse_list_output(output);

        assert_eq!(stats.total_items, 5);
        assert_eq!(stats.dir_count, 2); // src/ and tests/
        assert_eq!(stats.file_count, 3);
    }

    #[test]
    fn test_large_grep_output() {
        // Simulate large output with many matches
        let mut output = String::new();
        for i in 0..200 {
            output.push_str(&format!("src/file{}.rs:{}:    match line\n", i % 20, i));
        }

        let summarizer = GrepSummarizer::default();
        let summary = summarizer.summarize(&output, None).unwrap();

        // Should be very concise despite 200 matches
        assert!(estimate_tokens(&summary) < 150);
        assert!(summary.contains("Found 200 matches"));

        // Verify massive savings
        let (_llm, _ui, pct) = summarizer.estimate_savings(&output, &summary);
        assert!(pct > 95.0, "Should save >95% on large output");
    }
}
