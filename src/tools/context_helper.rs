//! Context engineering helper utilities for agents
//!
//! This module provides utilities to help agents understand and manage
//! their token budget and context window effectively.

/// Token budget thresholds (in percentage)
pub mod thresholds {
    pub const NORMAL: f64 = 0.75; // <75%: Normal operation
    pub const WARNING: f64 = 0.85; // 75-85%: Start consolidating
    pub const CRITICAL: f64 = 0.90; // >85%: Checkpoint required
}

/// Maximum tool response tokens
pub const MAX_TOOL_RESPONSE_TOKENS: usize = 25_000;

/// Context status based on token usage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextStatus {
    /// Normal operation (<75% usage)
    Normal,
    /// Start consolidating (75-85% usage)
    Warning,
    /// Checkpoint required (>85% usage)
    Critical,
}

impl ContextStatus {
    /// Determine status from usage ratio
    pub fn from_ratio(ratio: f64) -> Self {
        if ratio >= thresholds::CRITICAL {
            ContextStatus::Critical
        } else if ratio >= thresholds::WARNING {
            ContextStatus::Warning
        } else {
            ContextStatus::Normal
        }
    }

    /// Get recommended action for this status
    pub fn recommendation(&self) -> &'static str {
        match self {
            ContextStatus::Normal => "Continue normal operation",
            ContextStatus::Warning => {
                "Start consolidating: remove verbose outputs, keep findings only"
            }
            ContextStatus::Critical => {
                "CHECKPOINT REQUIRED: Create .progress.md, summarize all work, prepare for reset"
            }
        }
    }
}

/// Helper to format token usage for display
pub fn format_token_usage(used: usize, max: usize) -> String {
    let percentage = (used as f64 / max as f64) * 100.0;
    let status = ContextStatus::from_ratio(used as f64 / max as f64);

    format!(
        "{used}/{max} tokens ({percentage:.1}%) - {:?}: {}",
        status,
        status.recommendation()
    )
}

/// Estimate token count for text (fallback heuristic)
/// Note: VT Code's TokenBudgetManager provides accurate model-specific counting
pub fn estimate_tokens(text: &str) -> usize {
    // Rough estimate: ~4 chars per token for English
    // For code: ~3 chars per token (denser)
    let chars = text.len();
    let is_code = text.contains("fn ") || text.contains("impl ") || text.contains("struct ");

    if is_code { chars / 3 } else { chars / 4 }
}

/// Suggest actions based on context status
pub fn suggest_actions(used_tokens: usize, max_tokens: usize) -> Vec<String> {
    let ratio = used_tokens as f64 / max_tokens as f64;
    let status = ContextStatus::from_ratio(ratio);

    match status {
        ContextStatus::Normal => vec![
            "Continue normal operation".to_string(),
            "No restrictions on tool usage".to_string(),
        ],
        ContextStatus::Warning => vec![
            "Remove verbose tool outputs from history".to_string(),
            "Keep only findings, line numbers, file paths".to_string(),
            "Avoid reading full files - use max_tokens parameter".to_string(),
            "Consider using grep_file instead of read_file for discovery".to_string(),
        ],
        ContextStatus::Critical => vec![
            "STOP current work and create .progress.md checkpoint".to_string(),
            "Summarize all completed work (what, not how)".to_string(),
            "List remaining tasks with estimates".to_string(),
            "Prepare for context reset - save state to disk".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_status() {
        assert_eq!(ContextStatus::from_ratio(0.5), ContextStatus::Normal);
        assert_eq!(ContextStatus::from_ratio(0.8), ContextStatus::Warning);
        assert_eq!(ContextStatus::from_ratio(0.9), ContextStatus::Critical);
    }

    #[test]
    fn test_format_token_usage() {
        let result = format_token_usage(96_000, 128_000);
        assert!(result.contains("75.0%"));
        assert!(result.contains("Warning"));
    }

    #[test]
    fn test_estimate_tokens() {
        let english = "Hello world, this is a test.";
        let code = "fn main() { println!(\"Hello\"); }";

        assert!(estimate_tokens(english) > 0);
        assert!(estimate_tokens(code) > estimate_tokens(english));
    }
}
