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
        format!("{size}B")
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

/// Truncate text to `max_len` chars, reserving room for `ellipsis` so the
/// returned string never exceeds `max_len` chars.
///
/// This differs from [`truncate_text`], which appends the ellipsis *after*
/// taking `max_len` chars (yielding up to `max_len + ellipsis.len()` chars).
/// Use this when the total rendered width must stay within a hard budget.
///
/// ```
/// # use vtcode_commons::formatting::truncate_within;
/// assert_eq!(truncate_within("hello world", 8, "..."), "hello...");
/// assert_eq!(truncate_within("hi", 8, "..."), "hi");
/// assert_eq!(truncate_within("hello", 3, "…"), "he…");
/// ```
pub fn truncate_within(text: &str, max_len: usize, ellipsis: &str) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let keep = max_len.saturating_sub(ellipsis.chars().count());
    let mut truncated = text.chars().take(keep).collect::<String>();
    truncated.push_str(ellipsis);
    truncated
}

/// Truncate `value` to `max_chars` chars by keeping a head and a tail joined by
/// `marker`, preserving context from both ends of the text.
///
/// Returns `(text, was_truncated)`. When the budget is too small to fit the
/// marker plus meaningful context, falls back to a head-only prefix with a
/// ` [truncated]` suffix, respecting the `max_chars` budget.
///
/// ```
/// # use vtcode_commons::formatting::head_tail_truncate;
/// let (out, truncated) = head_tail_truncate("short", 64, " ... ");
/// assert_eq!(out, "short");
/// assert!(!truncated);
/// ```
pub fn head_tail_truncate(value: &str, max_chars: usize, marker: &str) -> (String, bool) {
    const SUFFIX: &str = " [truncated]";

    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return (value.to_string(), false);
    }

    let marker_chars = marker.chars().count();
    if max_chars <= marker_chars + 16 {
        let suffix_len = SUFFIX.chars().count();
        let truncated = if max_chars > suffix_len {
            let available = max_chars - suffix_len;
            let mut result = value.chars().take(available).collect::<String>();
            result.push_str(SUFFIX);
            result
        } else {
            value.chars().take(max_chars).collect::<String>()
        };
        return (truncated, true);
    }

    let available = max_chars.saturating_sub(marker_chars);
    let head_chars = (available * 2) / 3;
    let tail_chars = available.saturating_sub(head_chars);
    let head = value.chars().take(head_chars).collect::<String>();
    let tail = value
        .chars()
        .skip(total_chars.saturating_sub(tail_chars))
        .collect::<String>();
    let mut truncated = String::with_capacity(max_chars + 20);
    truncated.push_str(&head);
    truncated.push_str(marker);
    truncated.push_str(&tail);
    (truncated, true)
}

/// Word-wrap `text` into lines, allowing `first_width` chars on the first line
/// and `continuation_width` chars on subsequent lines. Wrapping prefers
/// whitespace boundaries and is UTF-8 safe (widths count chars, not bytes).
///
/// Returns an empty vec for blank input. Words longer than the width are split
/// at the width boundary rather than overflowing.
///
/// ```
/// # use vtcode_commons::formatting::wrap_text_words;
/// let lines = wrap_text_words("the quick brown fox", 9, 9);
/// assert_eq!(lines, vec!["the quick", "brown fox"]);
/// assert!(wrap_text_words("   ", 5, 5).is_empty());
/// ```
pub fn wrap_text_words(text: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut remaining = trimmed;
    let mut width = first_width.max(1);

    while remaining.chars().count() > width {
        let split = split_at_word_boundary(remaining, width);
        let (head, tail) = remaining.split_at(split);
        let head = head.trim();
        if head.is_empty() {
            break;
        }
        result.push(head.to_string());
        remaining = tail.trim_start();
        if remaining.is_empty() {
            break;
        }
        width = continuation_width.max(1);
    }

    if !remaining.is_empty() {
        result.push(remaining.to_string());
    }
    result
}

fn split_at_word_boundary(input: &str, width: usize) -> usize {
    let mut last_space: Option<usize> = None;
    for (seen, (idx, ch)) in input.char_indices().enumerate() {
        if seen > width {
            break;
        }
        if ch.is_whitespace() {
            last_space = Some(idx);
        }
    }
    match last_space {
        Some(pos) => pos,
        None => byte_index_for_char_count(input, width),
    }
}

fn byte_index_for_char_count(input: &str, chars: usize) -> usize {
    if chars == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (idx, ch) in input.char_indices() {
        seen += 1;
        if seen == chars {
            return idx + ch.len_utf8();
        }
    }
    input.len()
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

/// Collapse consecutive whitespace into single spaces, trimming leading/trailing.
///
/// ```
/// # use vtcode_commons::formatting::collapse_whitespace;
/// assert_eq!(collapse_whitespace("  hello   world  "), "hello world");
/// assert_eq!(collapse_whitespace(""), "");
/// ```
#[inline]
pub fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !result.is_empty() {
                result.push(' ');
            }
            result.push(ch);
            pending_space = false;
        }
    }
    result
}

/// Clean reasoning text by trimming trailing whitespace on each line and
/// removing blank lines.
///
/// ```
/// # use vtcode_commons::formatting::clean_reasoning_text;
/// assert_eq!(clean_reasoning_text("line1\n\n\nline2\n"), "line1\nline2");
/// assert_eq!(clean_reasoning_text(""), "");
/// ```
pub fn clean_reasoning_text(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
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
    fn wrap_text_words_basic_and_continuation_width() {
        assert_eq!(
            wrap_text_words("the quick brown fox", 9, 9),
            vec!["the quick", "brown fox"]
        );
        // First line wider than continuation lines.
        assert_eq!(
            wrap_text_words("alpha beta gamma delta", 11, 5),
            vec!["alpha beta", "gamma", "delta"]
        );
    }

    #[test]
    fn wrap_text_words_blank_and_unicode() {
        assert!(wrap_text_words("   ", 5, 5).is_empty());
        // Must not panic on multi-byte chars and counts chars, not bytes.
        let wrapped = wrap_text_words("あいう えお かきく", 3, 3);
        assert_eq!(wrapped, vec!["あいう", "えお", "かきく"]);
    }

    #[test]
    fn truncate_within_reserves_ellipsis_budget() {
        // Matches former runner::orchestration::truncate_chars behavior.
        assert_eq!(truncate_within("hello world", 8, "..."), "hello...");
        assert_eq!(truncate_within("hi", 8, "..."), "hi");
        // Single-char ellipsis reserves exactly one char (former snapshots /
        // session_archive behavior).
        assert_eq!(truncate_within("abcdef", 4, "…"), "abc…");
    }

    #[test]
    fn truncate_within_counts_chars() {
        let jp = "あいうえお"; // 5 chars
        assert_eq!(truncate_within(jp, 5, "…"), jp);
        assert_eq!(truncate_within(jp, 3, "…"), "あい…");
    }

    #[test]
    fn head_tail_truncate_keeps_both_ends() {
        let value = "0123456789".repeat(10); // 100 chars
        let (out, truncated) = head_tail_truncate(&value, 40, " ... [truncated] ... ");
        assert!(truncated);
        assert!(out.chars().count() <= 40);
        assert!(out.starts_with("012"));
        assert!(out.contains("[truncated]"));
        assert!(out.ends_with('9'));
    }

    #[test]
    fn head_tail_truncate_passes_through_when_short() {
        let (out, truncated) = head_tail_truncate("short", 64, " ... ");
        assert_eq!(out, "short");
        assert!(!truncated);
    }

    #[test]
    fn head_tail_truncate_small_budget_falls_back_to_prefix() {
        let marker = " ... [truncated] ... ";
        // max_chars <= marker_chars + 16 triggers the prefix fallback.
        // When max_chars (5) <= suffix_len (12), return just the prefix without suffix.
        let (out, truncated) = head_tail_truncate("abcdefghij", 5, marker);
        assert!(truncated);
        assert_eq!(out, "abcde");

        // When max_chars allows room for suffix, include it in the fallback branch.
        // Use max_chars=17 which is <= 21+16=37 (triggers fallback).
        let long_text = "abcdefghijklmnopqrstuvwxyz";
        let (out2, truncated2) = head_tail_truncate(long_text, 17, marker);
        assert!(truncated2);
        assert_eq!(out2, "abcde [truncated]");
        assert_eq!(out2.chars().count(), 17);
    }

    #[test]
    fn truncate_text_counts_chars_not_bytes() {
        let jp = "あいうえお"; // 5 chars, 15 bytes
        assert_eq!(truncate_text(jp, 3, "…"), "あいう…");
        assert_eq!(truncate_text(jp, 5, "…"), "あいうえお");
    }
}
