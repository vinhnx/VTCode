//! Shared preview formatting helpers.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadTailPreview<'a, T> {
    pub head: &'a [T],
    pub tail: &'a [T],
    pub hidden_count: usize,
    pub total: usize,
}

pub fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub fn truncate_to_display_width(text: &str, max_width: usize) -> &str {
    if max_width == 0 {
        return "";
    }
    if display_width(text) <= max_width {
        return text;
    }

    let mut consumed_width = 0usize;
    for (idx, ch) in text.char_indices() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if consumed_width + char_width > max_width {
            return &text[..idx];
        }
        consumed_width += char_width;
    }

    text
}

pub fn truncate_with_ellipsis(text: &str, max_width: usize, ellipsis: &str) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(text) <= max_width {
        return text.to_string();
    }

    let ellipsis_width = display_width(ellipsis);
    if ellipsis_width >= max_width {
        return truncate_to_display_width(ellipsis, max_width).to_string();
    }

    let truncated = truncate_to_display_width(text, max_width - ellipsis_width);
    format!("{truncated}{ellipsis}")
}

pub fn pad_to_display_width(text: &str, width: usize, pad_char: char) -> String {
    let current = display_width(text);
    if current >= width {
        return text.to_string();
    }

    let padding = pad_char.to_string().repeat(width - current);
    format!("{text}{padding}")
}

pub fn suffix_for_display_width(value: &str, max_width: usize) -> &str {
    if display_width(value) <= max_width {
        return value;
    }
    if max_width == 0 {
        return "";
    }

    let mut consumed_width = 0usize;
    let mut start_idx = value.len();
    for (idx, ch) in value.char_indices().rev() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if consumed_width + char_width > max_width {
            break;
        }
        consumed_width += char_width;
        start_idx = idx;
    }

    &value[start_idx..]
}

pub fn format_hidden_lines_summary(hidden: usize) -> String {
    if hidden == 1 {
        "… +1 line".to_string()
    } else {
        format!("… +{hidden} lines")
    }
}

pub fn split_head_tail_preview<'a, T>(
    items: &'a [T],
    head: usize,
    tail: usize,
) -> HeadTailPreview<'a, T> {
    let total = items.len();
    if total <= head.saturating_add(tail) {
        return HeadTailPreview {
            head: items,
            tail: &items[total..],
            hidden_count: 0,
            total,
        };
    }

    let head_count = head.min(total);
    let tail_count = tail.min(total.saturating_sub(head_count));
    let hidden_count = total.saturating_sub(head_count + tail_count);

    HeadTailPreview {
        head: &items[..head_count],
        tail: &items[total - tail_count..],
        hidden_count,
        total,
    }
}

pub fn summary_window(limit: usize, preferred_head: usize) -> (usize, usize) {
    if limit <= 2 {
        return (0, limit);
    }

    let head = preferred_head.min((limit - 1) / 2).max(1);
    let tail = limit.saturating_sub(head + 1).max(1);
    (head, tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_to_display_width_respects_wide_chars() {
        let value = "表表表";
        assert_eq!(truncate_to_display_width(value, 5), "表表");
    }

    #[test]
    fn truncate_with_ellipsis_respects_width_budget() {
        assert_eq!(truncate_with_ellipsis("abcdef", 4, "…"), "abc…");
    }

    #[test]
    fn pad_to_display_width_handles_wide_chars() {
        let padded = pad_to_display_width("表", 4, ' ');
        assert_eq!(display_width(padded.as_str()), 4);
    }

    #[test]
    fn suffix_for_display_width_preserves_tail() {
        assert_eq!(suffix_for_display_width("hello/world.rs", 8), "world.rs");
    }

    #[test]
    fn split_head_tail_preview_preserves_hidden_count() {
        let items = [1, 2, 3, 4, 5, 6, 7];
        let preview = split_head_tail_preview(&items, 2, 2);
        assert_eq!(preview.head, &[1, 2]);
        assert_eq!(preview.tail, &[6, 7]);
        assert_eq!(preview.hidden_count, 3);
        assert_eq!(preview.total, 7);
    }

    #[test]
    fn split_head_tail_preview_keeps_short_input_intact() {
        let items = [1, 2, 3];
        let preview = split_head_tail_preview(&items, 2, 2);
        assert_eq!(preview.head, &[1, 2, 3]);
        assert!(preview.tail.is_empty());
        assert_eq!(preview.hidden_count, 0);
    }

    #[test]
    fn summary_window_reserves_gap_row() {
        assert_eq!(summary_window(6, 3), (2, 3));
        assert_eq!(summary_window(2, 3), (0, 2));
    }

    #[test]
    fn hidden_lines_summary_matches_existing_copy() {
        assert_eq!(format_hidden_lines_summary(1), "… +1 line");
        assert_eq!(format_hidden_lines_summary(4), "… +4 lines");
    }
}
