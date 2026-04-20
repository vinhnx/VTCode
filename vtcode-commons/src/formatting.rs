//! Unified formatting utilities for UI and logging

/// Format file size in human-readable form (KB, MB, GB, etc.)
pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.1}GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1}MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1}KB", size as f64 / KB as f64)
    } else {
        format!("{}B", size)
    }
}

/// Indent a block of text with the given prefix
pub fn indent_block(text: &str, indent: &str) -> String {
    if indent.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let mut indented = String::with_capacity(text.len() + indent.len() * text.lines().count());
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            indented.push('\n');
        }
        if !line.is_empty() {
            indented.push_str(indent);
        }
        indented.push_str(line);
    }
    indented
}

/// Truncate text to a maximum length (in chars) with an optional ellipsis.
pub fn truncate_text(text: &str, max_len: usize, ellipsis: &str) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }

    let mut truncated = text.chars().take(max_len).collect::<String>();
    truncated.push_str(ellipsis);
    truncated
}

/// Truncate a string so that the retained prefix is at most `max_bytes` bytes,
/// rounded down to the nearest UTF-8 char boundary.  Returns the truncated
/// prefix with `suffix` appended, or the original string when it already fits.
pub fn truncate_byte_budget(text: &str, max_bytes: usize, suffix: &str) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_byte_budget_ascii() {
        assert_eq!(truncate_byte_budget("hello world", 5, "..."), "hello...");
        assert_eq!(truncate_byte_budget("hi", 10, "..."), "hi");
    }

    #[test]
    fn truncate_byte_budget_cjk_no_panic() {
        // 'こ' = 3 bytes, 'ん' = 3 bytes → "こんにちは" = 15 bytes
        let jp = "こんにちは";
        // Cutting at 5 bytes lands inside 'ん' (bytes 3..6); must round down to 3.
        assert_eq!(truncate_byte_budget(jp, 5, "…"), "こ…");
        // Cutting at 6 lands on boundary
        assert_eq!(truncate_byte_budget(jp, 6, "…"), "こん…");
    }

    #[test]
    fn truncate_byte_budget_mixed_ascii_cjk() {
        let mixed = "AB日本語CD";
        // A=1, B=1, 日=3, 本=3, 語=3, C=1, D=1 → 13 bytes total
        assert_eq!(truncate_byte_budget(mixed, 4, ".."), "AB.."); // mid-日 rounds to 2
        assert_eq!(truncate_byte_budget(mixed, 5, ".."), "AB日.."); // 2+3=5 exact
    }

    #[test]
    fn truncate_byte_budget_emoji() {
        let emoji = "👋🌍"; // 4 bytes each = 8 bytes
        assert_eq!(truncate_byte_budget(emoji, 5, "!"), "👋!");
    }

    #[test]
    fn truncate_byte_budget_zero() {
        assert_eq!(truncate_byte_budget("abc", 0, "..."), "...");
    }

    #[test]
    fn truncate_text_counts_chars_not_bytes() {
        let jp = "あいうえお"; // 5 chars, 15 bytes
        assert_eq!(truncate_text(jp, 3, "…"), "あいう…");
        assert_eq!(truncate_text(jp, 5, "…"), "あいうえお");
    }
}
