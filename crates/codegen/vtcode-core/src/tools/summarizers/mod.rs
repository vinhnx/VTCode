//! Tool result summarization strategies
//!
//! Implements Phase 4 summarization: Converting full tool outputs into
//! concise LLM-friendly summaries while preserving rich UI content.
//!
//! ## Summarization Strategies
//!
//! Different tool types need different summarization approaches:
//! - **Search tools** (grep, list): Count-based summaries
//! - **File operations** (read, edit): Content previews and statistics
//! - **Edit tools** (edit, patch): Diff statistics
//! - **Execution tools** (bash, code): Output summaries

use anyhow::Result;

use crate::utils::tokens::{estimate_tokens, truncate_to_tokens};

pub mod execution;
pub mod file_ops;
pub mod search;

/// Token savings estimate from summarization.
#[derive(Debug, Clone)]
pub struct SavingsEstimate {
    /// Token count of the summarized (LLM) output.
    pub llm_tokens: usize,
    /// Token count of the full (UI) output.
    pub ui_tokens: usize,
    /// Percentage of tokens saved (0.0 - 100.0).
    pub savings_percent: f32,
}

use std::borrow::Cow;

/// Truncate a line to max length with ellipsis (shared by execution + file_ops summarizers).
///
/// Returns `Cow::Borrowed` when no truncation is needed (zero allocation).
pub(super) fn truncate_line<'a>(line: &'a str, max_len: usize) -> Cow<'a, str> {
    if line.len() <= max_len {
        Cow::Borrowed(line)
    } else {
        let target = max_len.saturating_sub(3);
        let end = line.char_indices().map(|(i, _)| i).rfind(|&i| i <= target).unwrap_or(0);
        Cow::Owned(format!("{}...", &line[..end]))
    }
}

/// Trait for tool result summarization strategies
///
/// Each tool type implements its own summarization logic
/// to convert full output into concise LLM context
pub trait Summarizer {
    /// Summarize full output into concise LLM content
    ///
    /// # Arguments
    /// * `full_output` - The complete tool output (for UI)
    /// * `metadata` - Optional metadata about the operation
    ///
    /// # Returns
    /// Concise summary optimized for LLM context (target: <100 tokens)
    fn summarize(&self, full_output: &str, metadata: Option<&serde_json::Value>) -> Result<String>;

    /// Estimate token savings from summarization
    fn estimate_savings(&self, full_output: &str, summary: &str) -> SavingsEstimate {
        let ui_tokens = estimate_tokens(full_output);
        let llm_tokens = estimate_tokens(summary);
        let savings = ui_tokens.saturating_sub(llm_tokens);
        let savings_percent = if ui_tokens > 0 {
            (savings as f32 / ui_tokens as f32) * 100.0
        } else {
            0.0
        };
        SavingsEstimate { llm_tokens, ui_tokens, savings_percent }
    }
}

/// Extract key information from text (first N lines, keywords, etc.)
///
/// Useful for command output, file content, etc.
pub fn extract_key_info(text: &str, max_lines: usize) -> String {
    let mut lines: Vec<&str> = Vec::with_capacity(max_lines.min(32));
    let mut total_lines = 0usize;
    for line in text.lines() {
        total_lines += 1;
        if lines.len() < max_lines {
            lines.push(line);
        }
    }

    if total_lines > max_lines {
        format!("{}\n[...{} more lines]", lines.join("\n"), total_lines - max_lines)
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        // tiktoken cl100k_base BPE tokenizes "Hello world" as 2 tokens
        assert_eq!(estimate_tokens("Hello world"), 2);
        assert_eq!(estimate_tokens(""), 0);
        // Repeated single chars are highly compressible: ~8 chars/token for 'a'
        assert_eq!(estimate_tokens("a".repeat(1000).as_str()), 125);
    }

    #[test]
    fn test_truncate_to_tokens() {
        let text = "a".repeat(1000);
        let truncated = truncate_to_tokens(&text, 50); // 50 tokens ≈ 400 chars at 8 chars/token
        // BPE with repeated chars is more efficient (~8 chars/token)
        assert!(truncated.len() <= 500); // generous bound for BPE variance
        // BPE decode of repeated chars succeeds without "..." fallback
        assert!(truncated.len() < text.len());
    }

    #[test]
    fn test_truncate_short_text() {
        let text = "Short text";
        let truncated = truncate_to_tokens(text, 100);
        assert_eq!(truncated, text);
    }

    #[test]
    fn test_extract_key_info() {
        let text = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
        let extracted = extract_key_info(text, 3);
        assert!(extracted.contains("Line 1"));
        assert!(extracted.contains("Line 3"));
        assert!(extracted.contains("[...2 more lines]"));
    }

    #[test]
    fn test_extract_key_info_exact() {
        let text = "Line 1\nLine 2\nLine 3";
        let extracted = extract_key_info(text, 3);
        assert_eq!(extracted, text);
        assert!(!extracted.contains("more lines"));
    }
}
