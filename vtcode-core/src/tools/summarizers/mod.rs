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

pub mod execution;
pub mod file_ops;
pub mod search;

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
    ///
    /// Returns (llm_tokens, ui_tokens, savings_percent)
    fn estimate_savings(&self, full_output: &str, summary: &str) -> (usize, usize, f32) {
        let ui_tokens = estimate_tokens(full_output);
        let llm_tokens = estimate_tokens(summary);
        let savings = ui_tokens.saturating_sub(llm_tokens);
        let savings_pct = if ui_tokens > 0 {
            (savings as f32 / ui_tokens as f32) * 100.0
        } else {
            0.0
        };
        (llm_tokens, ui_tokens, savings_pct)
    }
}

/// Estimate token count from string
///
/// Simple estimation: 1 token ≈ 4 characters
/// Conservative and works well for English text
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() as f32 / 4.0).ceil() as usize
}

/// Truncate string to approximate token limit
///
/// Useful for ensuring summaries stay within budget
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let mut truncated = text[..max_chars].to_string();
        truncated.push_str("...");
        truncated
    }
}

/// Extract key information from text (first N lines, keywords, etc.)
///
/// Useful for command output, file content, etc.
pub fn extract_key_info(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().take(max_lines).collect();
    if text.lines().count() > max_lines {
        format!(
            "{}\n[...{} more lines]",
            lines.join("\n"),
            text.lines().count() - max_lines
        )
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("Hello world"), 3); // 11 chars / 4 ≈ 3
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a".repeat(1000).as_str()), 250); // 1000 / 4 = 250
    }

    #[test]
    fn test_truncate_to_tokens() {
        let text = "a".repeat(1000);
        let truncated = truncate_to_tokens(&text, 50); // 50 tokens = 200 chars
        assert!(truncated.len() <= 203); // 200 + "..."
        assert!(truncated.ends_with("..."));
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
