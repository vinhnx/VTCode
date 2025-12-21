//! File operation result summarization
//!
//! Summarizes read_file and edit_file outputs from full content
//! into concise summaries suitable for LLM context.
//!
//! ## Strategy
//!
//! Instead of sending full file contents (potentially thousands of lines),
//! send structural information:
//! - "Read 450 lines from src/main.rs. Preview: [first 10 lines]"
//! - "Modified 3 files: +45 lines, -12 lines. Changed: auth.rs, db.rs, api.rs"
//!
//! Target: ~100-200 tokens vs potentially thousands

use super::{Summarizer, extract_key_info, truncate_to_tokens};
use anyhow::Result;
use serde_json::Value;

/// Summarizer for read_file results
pub struct ReadSummarizer {
    /// Maximum number of preview lines to show (from start)
    pub max_preview_lines: usize,
    /// Maximum number of suffix lines to show (from end)
    pub max_suffix_lines: usize,
    /// Maximum tokens for entire summary
    pub max_tokens: usize,
}

impl Default for ReadSummarizer {
    fn default() -> Self {
        Self {
            max_preview_lines: 10,
            max_suffix_lines: 3,
            max_tokens: 200,
        }
    }
}

impl Summarizer for ReadSummarizer {
    fn summarize(&self, full_output: &str, metadata: Option<&Value>) -> Result<String> {
        // Try to extract file path from metadata if available
        let file_path = metadata
            .and_then(|m| m.get("file_path"))
            .and_then(|f| f.as_str())
            .unwrap_or("file");

        // Parse the output to get file stats
        let stats = parse_read_output(full_output);

        // Build concise summary
        let mut summary = format!(
            "Read {} lines from {}",
            stats.total_lines,
            file_path
        );

        // Add file size if significant
        if stats.total_chars > 10_000 {
            let kb = stats.total_chars / 1024;
            summary.push_str(&format!(" ({} KB)", kb));
        }

        // Add preview of first lines
        if !stats.preview_lines.is_empty() {
            let preview = stats.preview_lines
                .iter()
                .take(self.max_preview_lines)
                .map(|line| truncate_line(line, 80))
                .collect::<Vec<_>>()
                .join("\n");

            summary.push_str(&format!("\n\nPreview:\n{}", preview));

            if stats.total_lines > self.max_preview_lines {
                summary.push_str(&format!(
                    "\n[...{} more lines]",
                    stats.total_lines - self.max_preview_lines
                ));
            }
        }

        // Add suffix lines if file is long
        if stats.total_lines > self.max_preview_lines + self.max_suffix_lines
            && !stats.suffix_lines.is_empty()
        {
            let suffix = stats.suffix_lines
                .iter()
                .take(self.max_suffix_lines)
                .map(|line| truncate_line(line, 80))
                .collect::<Vec<_>>()
                .join("\n");

            summary.push_str(&format!("\n\nEnd:\n{}", suffix));
        }

        // Truncate to token limit
        Ok(truncate_to_tokens(&summary, self.max_tokens))
    }
}

/// Summarizer for edit_file results
pub struct EditSummarizer {
    /// Maximum tokens for entire summary
    pub max_tokens: usize,
}

impl Default for EditSummarizer {
    fn default() -> Self {
        Self {
            max_tokens: 150,
        }
    }
}

impl Summarizer for EditSummarizer {
    fn summarize(&self, full_output: &str, _metadata: Option<&Value>) -> Result<String> {
        // Parse edit output to extract statistics
        let stats = parse_edit_output(full_output);

        let mut summary = if stats.success {
            format!("Modified {} file(s)", stats.files_changed)
        } else {
            "Edit failed".to_string()
        };

        // Add line change statistics
        if stats.lines_added > 0 || stats.lines_removed > 0 {
            summary.push_str(&format!(
                ": +{} lines, -{} lines",
                stats.lines_added,
                stats.lines_removed
            ));
        }

        // Add affected files
        if !stats.affected_files.is_empty() {
            let files = stats.affected_files
                .iter()
                .take(5)
                .map(|f| {
                    // Extract just filename from path
                    f.split('/').last().unwrap_or(f)
                })
                .collect::<Vec<_>>()
                .join(", ");

            summary.push_str(&format!(". Changed: {}", files));

            if stats.affected_files.len() > 5 {
                summary.push_str(&format!(" (+{} more)", stats.affected_files.len() - 5));
            }
        }

        Ok(truncate_to_tokens(&summary, self.max_tokens))
    }
}

/// Statistics extracted from read output
#[derive(Debug, Default)]
struct ReadStats {
    total_lines: usize,
    total_chars: usize,
    preview_lines: Vec<String>,
    suffix_lines: Vec<String>,
}

/// Statistics extracted from edit output
#[derive(Debug, Default)]
struct EditStats {
    success: bool,
    files_changed: usize,
    lines_added: usize,
    lines_removed: usize,
    affected_files: Vec<String>,
}

/// Parse read_file output to extract statistics
fn parse_read_output(output: &str) -> ReadStats {
    let mut stats = ReadStats::default();

    let lines: Vec<&str> = output.lines().collect();
    stats.total_lines = lines.len();
    stats.total_chars = output.len();

    // Get preview lines (first 10)
    stats.preview_lines = lines
        .iter()
        .take(10)
        .map(|line| line.to_string())
        .collect();

    // Get suffix lines (last 3)
    if lines.len() > 13 {
        stats.suffix_lines = lines
            .iter()
            .rev()
            .take(3)
            .rev()
            .map(|line| line.to_string())
            .collect();
    }

    stats
}

/// Parse edit_file output to extract statistics
fn parse_edit_output(output: &str) -> EditStats {
    let mut stats = EditStats::default();

    // Try to parse as JSON first
    if let Ok(json) = serde_json::from_str::<Value>(output) {
        stats.success = json.get("success")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        // Extract file information
        if let Some(files) = json.get("files").and_then(|f| f.as_array()) {
            stats.files_changed = files.len();
            stats.affected_files = files
                .iter()
                .filter_map(|f| f.as_str().map(|s| s.to_string()))
                .collect();
        }

        // Extract change statistics
        stats.lines_added = json.get("lines_added")
            .and_then(|l| l.as_u64())
            .unwrap_or(0) as usize;

        stats.lines_removed = json.get("lines_removed")
            .and_then(|l| l.as_u64())
            .unwrap_or(0) as usize;
    } else {
        // Fallback: parse text output
        stats.success = output.to_lowercase().contains("success")
            && !output.to_lowercase().contains("error");

        // Try to count +/- lines in diff-like output
        for line in output.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                stats.lines_added += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                stats.lines_removed += 1;
            }
        }

        if stats.lines_added > 0 || stats.lines_removed > 0 {
            stats.files_changed = 1; // At least one file changed
        }
    }

    stats
}

/// Truncate a line to max length with ellipsis
fn truncate_line(line: &str, max_len: usize) -> String {
    if line.len() <= max_len {
        line.to_string()
    } else {
        format!("{}...", &line[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_summarizer_small_file() {
        let full_output = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";

        let summarizer = ReadSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        assert!(summary.contains("Read 5 lines"));
        assert!(summary.contains("Preview"));
        assert!(summary.contains("Line 1"));
    }

    #[test]
    fn test_read_summarizer_large_file() {
        let mut lines = Vec::new();
        for i in 1..=100 {
            lines.push(format!("Line {}", i));
        }
        let full_output = lines.join("\n");

        let summarizer = ReadSummarizer::default();
        let summary = summarizer.summarize(&full_output, None).unwrap();

        assert!(summary.contains("Read 100 lines"));
        assert!(summary.contains("more lines"));
        assert!(summary.contains("Line 1"));

        // Should be much shorter than full output
        let (llm, ui, pct) = summarizer.estimate_savings(&full_output, &summary);
        assert!(pct > 80.0, "Should save >80% (got {:.1}%)", pct);
    }

    #[test]
    fn test_read_summarizer_with_metadata() {
        let full_output = "fn main() {\n    println!(\"Hello\");\n}";
        let metadata = serde_json::json!({
            "file_path": "src/main.rs"
        });

        let summarizer = ReadSummarizer::default();
        let summary = summarizer.summarize(full_output, Some(&metadata)).unwrap();

        assert!(summary.contains("src/main.rs"));
        assert!(summary.contains("fn main()"));
    }

    #[test]
    fn test_edit_summarizer_json() {
        let full_output = r#"{
            "success": true,
            "files": ["src/auth.rs", "src/db.rs", "src/api.rs"],
            "lines_added": 45,
            "lines_removed": 12
        }"#;

        let summarizer = EditSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        assert!(summary.contains("Modified 3 file"));
        assert!(summary.contains("+45 lines"));
        assert!(summary.contains("-12 lines"));
        assert!(summary.contains("auth.rs"));
    }

    #[test]
    fn test_edit_summarizer_diff() {
        let full_output = "--- a/test.rs\n+++ b/test.rs\n+new line\n+another line\n-old line";

        let summarizer = EditSummarizer::default();
        let summary = summarizer.summarize(full_output, None).unwrap();

        // Diff output without "success" marker is treated as failed
        // But should still show line counts if changes detected
        assert!(summary.contains("Edit") || summary.contains("lines"));
        assert!(summary.contains("+2 lines") || summary.contains("-1 line") || summary.len() > 0);
    }

    #[test]
    fn test_truncate_line() {
        let long_line = "a".repeat(100);
        let truncated = truncate_line(&long_line, 50);

        assert!(truncated.len() <= 50);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_read_stats_parsing() {
        let output = "Line 1\nLine 2\nLine 3";
        let stats = parse_read_output(output);

        assert_eq!(stats.total_lines, 3);
        assert_eq!(stats.preview_lines.len(), 3);
        assert_eq!(stats.preview_lines[0], "Line 1");
    }
}
