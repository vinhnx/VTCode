//! Shared fenced-code-block scanning and tool-name hygiene.
//!
//! Content-derived tool-call extraction must not treat markup inside fences
//! (documentation, test fixtures) or mid-prose tag mentions as executable
//! calls. This module is pure infrastructure used by the runloop `text_tools`
//! parsers and the skill sub-LLM textual fallback.

/// Maximum accepted tool-name length for content-derived calls.
pub const MAX_CLEAN_TOOL_NAME_LEN: usize = 64;

/// Classify a line as a fenced-code-block delimiter.
///
/// Returns `Some((fence_char, is_closing))` when the line opens or closes a
/// fence: an opener is 3+ backticks/tildes (info string allowed), a closer is
/// 3+ of the same fence character with nothing but whitespace after. A line
/// with an info string while a fence is open is body text (`None`).
pub fn fence_delimiter_line(line: &str, open_char: Option<char>) -> Option<(char, bool)> {
    let trimmed = line.trim();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let mut count = 1usize;
    let mut closed_run = false;
    for ch in chars {
        if ch == first {
            count += 1;
        } else {
            closed_run = true;
            break;
        }
    }
    if count < 3 {
        return None;
    }
    if closed_run {
        // Info string present: only valid as an opener.
        return Some((first, false));
    }
    let only_whitespace = trimmed.chars().skip(count).all(char::is_whitespace);
    match open_char {
        Some(open) if open == first && only_whitespace => Some((first, true)),
        Some(_) => None,
        None => Some((first, false)),
    }
}

/// Byte ranges of `text` outside fenced code blocks.
///
/// Markup inside a fence is quoted documentation, not an executable call.
pub fn unfenced_byte_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut open_char: Option<char> = None;
    let mut segment_start = 0usize;
    let mut cursor = 0usize;
    for line in text.split_inclusive('\n') {
        let line_start = cursor;
        cursor += line.len();
        let Some((fence_char, is_closing)) = fence_delimiter_line(line, open_char) else {
            continue;
        };
        if is_closing {
            open_char = None;
        } else if open_char.is_none() {
            ranges.push(segment_start..line_start);
            open_char = Some(fence_char);
        }
        segment_start = cursor;
    }
    // Fail-open for truncated streams: an unclosed fence opener must not
    // swallow a real tool call in the tail (existing runloop contract).
    // Closed fences still exclude their bodies via the ranges above.
    ranges.push(segment_start..text.len());
    ranges.retain(|range| range.start < range.end);
    ranges
}

/// First byte offset of `needle` at or after `from`, outside fenced code blocks.
pub fn find_unfenced_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    unfenced_byte_ranges(text).into_iter().find_map(|range| {
        let start = range.start.max(from);
        if start >= range.end {
            return None;
        }
        text.get(start..range.end)
            .and_then(|slice| slice.find(needle))
            .map(|index| start + index)
    })
}

/// Whether `raw` is a clean tool identifier (not prose, not a fixture blob).
pub fn is_clean_tool_name(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_CLEAN_TOOL_NAME_LEN {
        return false;
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Whether `raw` is safe to dispatch as a native tool name (not a prose blob).
///
/// Allows MCP-visible names (`mcp__github__list`) and names with `:`/`-`/`.`
/// while rejecting whitespace/newlines/backticks and absurd lengths.
pub fn is_dispatchable_tool_name(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_CLEAN_TOOL_NAME_LEN {
        return false;
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '`' | '\u{2014}' | '\u{2013}'))
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfenced_ranges_exclude_fence_bodies() {
        let text = "before\n```sh\nINSIDE\n```\nafter\n";
        let ranges = unfenced_byte_ranges(text);
        assert_eq!(ranges.len(), 2);
        let first = ranges.first().expect("first unfenced range");
        let second = ranges.get(1).expect("second unfenced range");
        assert!(text.get(first.clone()).is_some_and(|slice| slice.contains("before")));
        assert!(text.get(second.clone()).is_some_and(|slice| slice.contains("after")));
    }

    #[test]
    fn find_unfenced_skips_matches_inside_fences() {
        let text = "docs\n```\nNEEDLE\n```\nok NEEDLE\n";
        let hit = find_unfenced_from(text, "NEEDLE", 0).expect("unfenced match");
        assert_eq!(text.get(hit..hit + "NEEDLE".len()), Some("NEEDLE"));
    }

    #[test]
    fn clean_and_dispatchable_name_rules() {
        assert!(is_clean_tool_name("exec_command"));
        assert!(!is_clean_tool_name("has space"));
        assert!(is_dispatchable_tool_name("mcp::github::list"));
        assert!(!is_dispatchable_tool_name("has space"));
    }
}
