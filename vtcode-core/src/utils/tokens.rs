//! Token estimation utilities
//!
//! Provides simple token counting heuristics for text content.

/// Estimate token count from string
///
/// Simple estimation: 1 token ≈ 4 characters
/// This is conservative and works well for English text
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() as f32 / 4.0).ceil() as usize
}

/// Truncate string to approximate token limit
///
/// Useful for ensuring content stays within budget
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("test"), 1);
        assert_eq!(estimate_tokens("hello world"), 3);
        assert_eq!(estimate_tokens("a".repeat(100).as_str()), 25);
    }

    #[test]
    fn test_truncate_to_tokens() {
        let text = "a".repeat(100);
        let truncated = truncate_to_tokens(&text, 10);
        assert!(truncated.len() <= 43); // 10 * 4 + 3 for "..."
        assert!(truncated.ends_with("..."));
    }
}
