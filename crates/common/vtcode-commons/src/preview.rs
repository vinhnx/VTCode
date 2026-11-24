#![expect(
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "Preview counts and UTF-8 offsets are computed from source lengths and boundary helpers."
)]

//! Shared preview formatting helpers.

use crate::display_width::char_display_width_or;
pub use crate::display_width::display_width;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeadTailPreview<'a, T> {
    pub head: &'a [T],
    pub tail: &'a [T],
    pub hidden_count: usize,
    total: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextLineExcerpt<'a> {
    pub head: Vec<&'a str>,
    pub tail: Vec<&'a str>,
    pub hidden_count: usize,
    pub total: usize,
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
        let char_width = char_display_width_or(ch, 0);
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
        let char_width = char_display_width_or(ch, 0);
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

fn split_head_tail_preview<'a, T>(items: &'a [T], head: usize, tail: usize) -> HeadTailPreview<'a, T> {
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

pub fn split_head_tail_preview_with_limit<'a, T>(
    items: &'a [T],
    limit: usize,
    preferred_head: usize,
) -> HeadTailPreview<'a, T> {
    if limit == 0 {
        return HeadTailPreview {
            head: &items[..0],
            tail: &items[..0],
            hidden_count: items.len(),
            total: items.len(),
        };
    }

    if items.len() <= limit {
        return HeadTailPreview {
            head: items,
            tail: &items[items.len()..],
            hidden_count: 0,
            total: items.len(),
        };
    }

    let (head, tail) = summary_window(limit, preferred_head);
    split_head_tail_preview(items, head, tail)
}

pub fn summary_window(limit: usize, preferred_head: usize) -> (usize, usize) {
    if limit <= 2 {
        return (0, limit);
    }

    let head = preferred_head.min((limit - 1) / 2).max(1);
    let tail = limit.saturating_sub(head + 1).max(1);
    (head, tail)
}

/// Pre-allocation cap (in `&str` refs, 16 bytes each) for the excerpt probe and
/// tail ring. Real windows are a handful of lines, so this only binds
/// pathological caller windows; growth beyond it is driven by output the
/// caller actually asked for, never by input size.
const EXCERPT_PREALLOC_CAP: usize = 1024;

/// Single-pass head/tail excerpt over lines: O(window) memory, independent of input size.
///
/// The naive form (`text.lines().collect()` then slice) materializes one `&str`
/// per line only to drop all but the window — a transient allocation proportional
/// to the input on every large tool output. Instead this buffers at most
/// `fit_budget + 1` lines to detect the fit case, then streams the remainder
/// through a tail-sized ring while counting the total.
///
/// Contract:
/// - `total <= fit_budget` → `head` holds every line, `tail` is empty, `hidden_count` is 0.
/// - otherwise → `head` holds the first `head` lines, `tail` the last `tail`
///   lines, `hidden_count = total - head - tail`.
///
/// Callers must pass `head + tail <= fit_budget` so both branches agree; both
/// public wrappers satisfy this (`excerpt_text_lines` uses `head + tail`,
/// `excerpt_text_lines_with_limit` uses `limit >= head + tail` via `summary_window`).
/// The obligation is executable below: a violating caller fails loud instead
/// of silently returning a wrongly-split excerpt.
fn head_tail_excerpt<'a>(text: &'a str, fit_budget: usize, head: usize, tail: usize) -> TextLineExcerpt<'a> {
    debug_assert!(
        head.saturating_add(tail) <= fit_budget,
        "window ({head} + {tail}) must fit the fit budget ({fit_budget})"
    );
    let mut lines = text.lines();
    // Probe: buffer just past the fit budget to decide fit vs. overflow.
    // Capacity is exact for small windows and capped for huge ones; growth
    // beyond the cap only happens when the output itself needs the lines.
    let mut leading: Vec<&'a str> = Vec::with_capacity(fit_budget.saturating_add(1).min(EXCERPT_PREALLOC_CAP));
    for line in lines.by_ref().take(fit_budget.saturating_add(1)) {
        leading.push(line);
    }
    if leading.len() <= fit_budget {
        let total = leading.len();
        return TextLineExcerpt {
            head: leading,
            tail: Vec::new(),
            hidden_count: 0,
            total,
        };
    }

    // Overflow: `total > fit_budget >= head + tail`, so the window slices are
    // exactly `head` and `tail` lines (no `min` needed) and the ring below
    // always fills. (`fit_budget` saturates rather than overflows; saturation
    // implies a budget no line count can reach, so overflow is unreachable then
    // and the arithmetic below only runs on exact sums.)
    let mut head_vec: Vec<&'a str> = Vec::with_capacity(head.min(leading.len()));
    head_vec.extend(leading.iter().take(head).copied());
    // Ring invariant: `write_idx < tail` whenever the ring is full; pushes
    // before it fills append in order, so `rotate_left(write_idx)` restores
    // oldest-first order exactly when the ring wrapped.
    let mut ring: Vec<&'a str> = Vec::with_capacity(tail.min(EXCERPT_PREALLOC_CAP));
    let mut write_idx = 0usize;
    let mut fed = 0usize;
    let mut ring_push = |line: &'a str| {
        if tail == 0 {
            return;
        }
        if ring.len() < tail {
            ring.push(line);
        } else {
            ring[write_idx] = line;
            write_idx = (write_idx + 1) % tail;
        }
        fed += 1;
    };
    for &line in leading.iter().skip(head) {
        ring_push(line);
    }
    let mut total = leading.len();
    for line in lines {
        ring_push(line);
        total += 1;
    }
    if fed > tail {
        ring.rotate_left(write_idx);
    }
    let hidden_count = total.saturating_sub(head_vec.len()).saturating_sub(ring.len());
    TextLineExcerpt { head: head_vec, tail: ring, hidden_count, total }
}

/// First `head` and last `tail` lines of `text` in a single pass.
///
/// Memory is O(`head + tail`), not O(total lines): large tool outputs stream
/// through a tail-sized ring instead of materializing every line transiently.
pub fn excerpt_text_lines<'a>(text: &'a str, head: usize, tail: usize) -> TextLineExcerpt<'a> {
    head_tail_excerpt(text, head.saturating_add(tail), head, tail)
}

/// [`excerpt_text_lines`] with the window derived from `summary_window`.
///
/// Fit is tested against `limit`, not `head + tail`: `summary_window` reserves
/// one row for the gap marker (`head + tail == limit - 1` when `limit > 2`),
/// so an input of exactly `limit` lines still fits whole.
pub fn excerpt_text_lines_with_limit<'a>(text: &'a str, limit: usize, preferred_head: usize) -> TextLineExcerpt<'a> {
    let (head, tail) = summary_window(limit, preferred_head);
    head_tail_excerpt(text, limit, head, tail)
}

fn format_hidden_bytes_summary(hidden: usize) -> String {
    format!("… [{hidden} bytes omitted] …")
}

pub fn condense_text_bytes(content: &str, head_bytes: usize, tail_bytes: usize) -> String {
    let byte_len = content.len();
    let max_inline = head_bytes + tail_bytes;
    if byte_len <= max_inline {
        return content.to_string();
    }

    let head_end = content.floor_char_boundary(head_bytes);
    let tail_start_raw = byte_len.saturating_sub(tail_bytes);
    let tail_start = content.ceil_char_boundary(tail_start_raw);

    let omitted = byte_len.saturating_sub(head_end).saturating_sub(byte_len - tail_start);

    format!("{}\n\n{}\n\n{}", &content[..head_end], format_hidden_bytes_summary(omitted), &content[tail_start..])
}

/// Bound model-facing error echoes of text the caller already sent
/// (old_str, patch context, snippets). Threshold is deliberately high:
/// typical edit mismatches are mid-string whitespace/indent issues, so most
/// real payloads must stay intact. Only pathological multi-KB pastes are
/// reduced to a head/tail.
pub fn condense_error_echo(text: &str) -> String {
    if text.len() > 2_000 {
        condense_text_bytes(text, 800, 800)
    } else {
        text.to_string()
    }
}

pub fn tail_preview_text(content: &str, tail_bytes: usize, max_lines: usize) -> String {
    if content.is_empty() {
        return String::new();
    }

    let tail_start = content.ceil_char_boundary(content.len().saturating_sub(tail_bytes));
    let tail_slice = &content[tail_start..];

    let mut line_start = 0usize;
    if max_lines > 0 {
        let mut seen = 0usize;
        for (idx, b) in tail_slice.as_bytes().iter().enumerate().rev() {
            if *b == b'\n' {
                seen += 1;
                if seen >= max_lines {
                    line_start = idx.saturating_add(1);
                    break;
                }
            }
        }
    }

    let preview = &tail_slice[line_start..];
    let omitted = tail_start.saturating_add(line_start);
    if omitted == 0 {
        return preview.to_string();
    }

    format!("{}\n{}", format_hidden_bytes_summary(omitted), preview)
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
    fn preview_padding_credits_halfwidth_sound_marks() {
        // The shared helper must agree with the TUI: `ｶﾞ` renders in two
        // cells, so a width-3 budget leaves exactly one space, not two.
        assert_eq!(pad_to_display_width("ｶﾞ", 3, ' '), "ｶﾞ ");
        assert_eq!(truncate_to_display_width("aｶﾞ", 2), "aｶ");
    }

    #[test]
    fn preview_width_math_follows_ambiguous_width_verdict() {
        // nextest isolates this process, so the one-time verdict override
        // cannot affect other tests. Truncation used to strip two ambiguous
        // glyphs here because it bypassed the TUI's CJK width table.
        crate::terminal_capabilities::set_ambiguous_width(crate::ui_protocol::AmbiguousWidthMode::Wide);
        assert_eq!(display_width("│││"), 6);
        assert_eq!(truncate_to_display_width("│││", 2), "│");
        assert_eq!(pad_to_display_width("│", 3, ' '), "│ ");
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
    fn split_head_tail_preview_with_limit_preserves_total_and_gap() {
        let items = [1, 2, 3, 4, 5, 6, 7];
        let preview = split_head_tail_preview_with_limit(&items, 6, 3);
        assert_eq!(preview.head, &[1, 2]);
        assert_eq!(preview.tail, &[5, 6, 7]);
        assert_eq!(preview.hidden_count, 2);
        assert_eq!(preview.total, 7);
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

    #[test]
    fn excerpt_text_lines_builds_head_tail_vectors() {
        let preview = excerpt_text_lines("l1\nl2\nl3\nl4\nl5\nl6", 2, 2);
        assert_eq!(preview.head, vec!["l1", "l2"]);
        assert_eq!(preview.tail, vec!["l5", "l6"]);
        assert_eq!(preview.hidden_count, 2);
        assert_eq!(preview.total, 6);
    }

    #[test]
    fn excerpt_text_lines_splits_asymmetric_windows() {
        let text = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8";
        let preview = excerpt_text_lines(text, 2, 3);
        assert_eq!(preview.head, vec!["l1", "l2"]);
        assert_eq!(preview.tail, vec!["l6", "l7", "l8"]);
        assert_eq!(preview.hidden_count, 3);
        assert_eq!(preview.total, 8);

        // Swapped windows select the opposite ends (asymmetric oracle: a
        // head/tail mix-up or off-by-one cannot pass both directions).
        let swapped = excerpt_text_lines(text, 3, 2);
        assert_eq!(swapped.head, vec!["l1", "l2", "l3"]);
        assert_eq!(swapped.tail, vec!["l7", "l8"]);
        assert_eq!(swapped.hidden_count, 3);
        assert_eq!(swapped.total, 8);
    }

    #[test]
    fn excerpt_text_lines_keeps_exact_fit_whole() {
        // total == head + tail: everything fits, nothing hidden.
        let fit = excerpt_text_lines("l1\nl2\nl3\nl4\nl5", 2, 3);
        assert_eq!(fit.head, vec!["l1", "l2", "l3", "l4", "l5"]);
        assert!(fit.tail.is_empty());
        assert_eq!(fit.hidden_count, 0);
        assert_eq!(fit.total, 5);

        // One line past the window hides exactly one line.
        let overflow = excerpt_text_lines("l1\nl2\nl3\nl4\nl5\nl6", 2, 3);
        assert_eq!(overflow.head, vec!["l1", "l2"]);
        assert_eq!(overflow.tail, vec!["l4", "l5", "l6"]);
        assert_eq!(overflow.hidden_count, 1);
        assert_eq!(overflow.total, 6);
    }

    #[test]
    fn excerpt_text_lines_handles_empty_windows() {
        let text = "l1\nl2\nl3";
        let neither = excerpt_text_lines(text, 0, 0);
        assert!(neither.head.is_empty());
        assert!(neither.tail.is_empty());
        assert_eq!(neither.hidden_count, 3);
        assert_eq!(neither.total, 3);

        let head_only = excerpt_text_lines(text, 2, 0);
        assert_eq!(head_only.head, vec!["l1", "l2"]);
        assert!(head_only.tail.is_empty());
        assert_eq!(head_only.hidden_count, 1);

        let tail_only = excerpt_text_lines(text, 0, 2);
        assert!(tail_only.head.is_empty());
        assert_eq!(tail_only.tail, vec!["l2", "l3"]);
        assert_eq!(tail_only.hidden_count, 1);

        let empty = excerpt_text_lines("", 2, 2);
        assert!(empty.head.is_empty());
        assert!(empty.tail.is_empty());
        assert_eq!(empty.hidden_count, 0);
        assert_eq!(empty.total, 0);
    }

    #[test]
    fn excerpt_text_lines_matches_naive_slicing() {
        // Independent re-derivation: materialize-then-slice is the pre-rewrite
        // contract; the single-pass version must agree on every window shape,
        // including windows larger than the input.
        let text = (1..=50).map(|i| format!("line-{i:02}")).collect::<Vec<_>>().join("\n");
        let all: Vec<&str> = text.lines().collect();
        for (head, tail) in [
            (0, 0),
            (0, 5),
            (5, 0),
            (1, 1),
            (3, 7),
            (7, 3),
            (25, 25),
            (49, 49),
            (50, 50),
            (100, 100),
        ] {
            let preview = excerpt_text_lines(&text, head, tail);
            let total = all.len();
            let (expected_head, expected_tail, expected_hidden) = if total <= head.saturating_add(tail) {
                (all.clone(), Vec::new(), 0)
            } else {
                let head_count = head.min(total);
                let tail_count = tail.min(total - head_count);
                (all[..head_count].to_vec(), all[total - tail_count..].to_vec(), total - head_count - tail_count)
            };
            assert_eq!(preview.head, expected_head, "head for window ({head}, {tail})");
            assert_eq!(preview.tail, expected_tail, "tail for window ({head}, {tail})");
            assert_eq!(preview.hidden_count, expected_hidden, "hidden for window ({head}, {tail})");
            assert_eq!(preview.total, total, "total for window ({head}, {tail})");
        }
    }

    #[test]
    fn excerpt_text_lines_matches_naive_slicing_for_all_tiny_inputs() {
        // Exhaustive over the boundary-dense corner: every total up to 8
        // lines against every window up to 3 + 3, fit and overflow both.
        for total in 0..=8usize {
            let text = (0..total).map(|i| format!("r{i}")).collect::<Vec<_>>().join("\n");
            let all: Vec<&str> = text.lines().collect();
            for head in 0..=3usize {
                for tail in 0..=3usize {
                    let preview = excerpt_text_lines(&text, head, tail);
                    let (expected_head, expected_tail, expected_hidden) = if total <= head.saturating_add(tail) {
                        (all.clone(), Vec::new(), 0)
                    } else {
                        let head_count = head.min(total);
                        let tail_count = tail.min(total - head_count);
                        (
                            all[..head_count].to_vec(),
                            all[total - tail_count..].to_vec(),
                            total - head_count - tail_count,
                        )
                    };
                    assert_eq!(preview.head, expected_head, "head for {total} lines, window ({head}, {tail})");
                    assert_eq!(preview.tail, expected_tail, "tail for {total} lines, window ({head}, {tail})");
                    assert_eq!(
                        preview.hidden_count, expected_hidden,
                        "hidden for {total} lines, window ({head}, {tail})"
                    );
                    assert_eq!(preview.total, total, "total for {total} lines, window ({head}, {tail})");
                }
            }
        }
    }

    #[test]
    fn excerpt_text_lines_with_limit_matches_legacy_split_for_all_tiny_inputs() {
        // Exhaustive over limits 0..=6 and head preferences 0..=6: the oracle
        // is the untouched `split_head_tail_preview_with_limit`, i.e. the
        // exact pre-rewrite contract including the gap-row reservation.
        for total in 0..=8usize {
            let text = (0..total).map(|i| format!("r{i}")).collect::<Vec<_>>().join("\n");
            let all: Vec<&str> = text.lines().collect();
            for limit in 0..=6usize {
                for preferred_head in 0..=6usize {
                    let preview = excerpt_text_lines_with_limit(&text, limit, preferred_head);
                    let legacy = split_head_tail_preview_with_limit(&all, limit, preferred_head);
                    assert_eq!(
                        preview.head,
                        legacy.head.to_vec(),
                        "head for {total} lines, limit {limit}, preferred {preferred_head}"
                    );
                    assert_eq!(
                        preview.tail,
                        legacy.tail.to_vec(),
                        "tail for {total} lines, limit {limit}, preferred {preferred_head}"
                    );
                    assert_eq!(
                        preview.hidden_count, legacy.hidden_count,
                        "hidden for {total} lines, limit {limit}, preferred {preferred_head}"
                    );
                    assert_eq!(
                        preview.total, legacy.total,
                        "total for {total} lines, limit {limit}, preferred {preferred_head}"
                    );
                }
            }
        }
    }

    #[test]
    fn excerpt_text_lines_streams_large_inputs() {
        // 100k lines through a small window: the old materialize-then-slice
        // path held every line transiently (~1.6 MB of refs); the ring keeps
        // only the window while counting the same total.
        let text = (0..100_000).map(|i| format!("row-{i}")).collect::<Vec<_>>().join("\n");
        let preview = excerpt_text_lines(&text, 4, 6);
        assert_eq!(preview.head, vec!["row-0", "row-1", "row-2", "row-3"]);
        assert_eq!(
            preview.tail,
            vec![
                "row-99994",
                "row-99995",
                "row-99996",
                "row-99997",
                "row-99998",
                "row-99999"
            ]
        );
        assert_eq!(preview.hidden_count, 100_000 - 10);
        assert_eq!(preview.total, 100_000);
    }

    #[test]
    fn excerpt_text_lines_with_limit_keeps_limit_sized_input_whole() {
        // summary_window(6, 3) == (2, 3): head + tail == limit - 1 (gap row),
        // so fit must be tested against `limit`, not the window sum. Six
        // lines fit; the seventh overflows into the (2, 3) window.
        let fit = excerpt_text_lines_with_limit("l1\nl2\nl3\nl4\nl5\nl6", 6, 3);
        assert_eq!(fit.head, vec!["l1", "l2", "l3", "l4", "l5", "l6"]);
        assert!(fit.tail.is_empty());
        assert_eq!(fit.hidden_count, 0);
        assert_eq!(fit.total, 6);

        let overflow = excerpt_text_lines_with_limit("l1\nl2\nl3\nl4\nl5\nl6\nl7", 6, 3);
        assert_eq!(overflow.head, vec!["l1", "l2"]);
        assert_eq!(overflow.tail, vec!["l5", "l6", "l7"]);
        assert_eq!(overflow.hidden_count, 2);
        assert_eq!(overflow.total, 7);
    }

    #[test]
    fn excerpt_text_lines_with_limit_handles_degenerate_limits() {
        let zero = excerpt_text_lines_with_limit("l1\nl2", 0, 0);
        assert!(zero.head.is_empty());
        assert!(zero.tail.is_empty());
        assert_eq!(zero.hidden_count, 2);
        assert_eq!(zero.total, 2);

        // limit <= 2 takes the all-tail window.
        let tiny = excerpt_text_lines_with_limit("l1\nl2\nl3", 2, 9);
        assert!(tiny.head.is_empty());
        assert_eq!(tiny.tail, vec!["l2", "l3"]);
        assert_eq!(tiny.hidden_count, 1);

        let empty = excerpt_text_lines_with_limit("", 0, 0);
        assert_eq!(empty.total, 0);
        assert_eq!(empty.hidden_count, 0);
    }

    #[test]
    fn condense_text_bytes_respects_utf8_boundaries() {
        let mut content = "a".repeat(7);
        content.push('é');
        content.push_str("bbbbbbbb");

        let preview = condense_text_bytes(&content, 8, 4);
        assert!(preview.contains("bytes omitted"));
        assert!(preview.is_char_boundary(0));
    }

    #[test]
    fn tail_preview_text_keeps_last_lines_only() {
        let input = (0..20).map(|index| format!("line-{index}")).collect::<Vec<_>>().join("\n");

        let preview = tail_preview_text(&input, 40, 3);
        assert!(preview.contains("bytes omitted"));
        assert!(preview.contains("line-19"));
        assert!(!preview.contains("line-1\n"));
    }
}
