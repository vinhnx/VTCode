//! Token estimation utilities

/// Estimate token count from string (rough approximation)
///
/// Simple estimation: 1 token ≈ 4 characters.
/// Returns a minimum of 1 for non-empty strings.
#[inline]
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    (text.len() / 4).max(1)
}

/// Truncate string to approximate token limit
///
/// Returns a truncated string that fits within the approximate token limit.
/// Tries to truncate at word boundaries when possible to avoid mid-word cuts.
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 || text.is_empty() {
        return String::new();
    }

    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        return text.to_string();
    }

    // Try to truncate at a word boundary
    let truncated = &text[..max_chars];
    match truncated.rfind(' ') {
        Some(last_space) if last_space > max_chars / 2 => {
            let mut result = truncated[..last_space].to_string();
            result.push_str("...");
            result
        }
        _ => {
            let mut result = truncated.to_string();
            result.push_str("...");
            result
        }
    }
}
