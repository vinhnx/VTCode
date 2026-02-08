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
    // Simple estimation: 1 token ≈ 4 characters.
    // Use ceiling division: (len + 3) / 4
    (text.len() + 3) / 4
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

    // Ensure we don't slice in the middle of a character
    let mut end = max_chars;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &text[..end];

    // Try to truncate at a word boundary
    match truncated.rfind(' ') {
        Some(last_space) if last_space > end / 2 => {
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
