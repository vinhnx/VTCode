use anstyle::Effects;
use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::super::message::{MessageLine, TranscriptLine};
use crate::tui::core_tui::types::InlineMessageKind;

/// Check if trimmed, ANSI-stripped text starts with a tool summary prefix.
///
/// Shared by both `is_tool_summary_line` and `reflow_tool_lines` to avoid
/// duplicating the prefix list (DRY).
pub(super) fn has_summary_prefix(text: &str) -> bool {
    let stripped = super::super::text_utils::strip_ansi_codes(text);
    stripped.starts_with("• ")
        || stripped.starts_with("  ├ ")
        || stripped.starts_with("  └ ")
        || stripped.starts_with("  │ ")
}

pub(crate) fn parse_tool_call_prefix(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("• ")?;
    let verb_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    if verb_end == 0 {
        return None;
    }
    let verb = &rest[..verb_end];
    let prefix_len = "• ".len() + verb.len();
    Some((verb, &text[..prefix_len]))
}

pub(super) fn is_tool_summary_line(message: &MessageLine) -> bool {
    let text: String = message.segments.iter().map(|segment| segment.text.as_str()).collect();
    has_summary_prefix(&text)
}

pub(super) fn is_bullet_summary_text(text: &str) -> bool {
    let stripped = super::super::text_utils::strip_ansi_codes(text);
    stripped.starts_with("• ")
}

pub(super) fn is_tree_detail_text(text: &str) -> bool {
    let stripped = super::super::text_utils::strip_ansi_codes(text);
    stripped.starts_with("  └ ") || stripped.starts_with("  ├ ") || stripped.starts_with("  │ ")
}

/// Returns `true` when the next message starts a tool-block boundary that owns
/// its own top spacing (Tool/Pty header or Info tool-summary line).
///
/// Trailing spacing from the previous block must be suppressed in this case so
/// the boundary contributes exactly one gap (the tool top, which is clamped to
/// a minimum of 1). Centralizes the check previously duplicated in
/// `reflow_message_lines` and `reflow_tool_lines`.
pub(super) fn next_is_tool_block(next: Option<&MessageLine>) -> bool {
    match next {
        Some(line) if line.kind == InlineMessageKind::Tool || line.kind == InlineMessageKind::Pty => true,
        Some(line) if line.kind == InlineMessageKind::Info => is_tool_summary_line(line),
        _ => false,
    }
}

/// Returns `true` when a reflowed ratatui line is visually blank (no segments
/// or only whitespace, e.g. `""` or `"  "`). Whitespace-only rows defeat a
/// naive `segments.is_empty()` check and must collapse like empty rows.
pub(super) fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.trim().is_empty())
}

/// Returns `true` when a transcript line is visually blank. Mirrors
/// [`line_is_blank`] for the [`TranscriptLine`] wrapper.
pub(super) fn transcript_line_is_blank(line: &TranscriptLine) -> bool {
    line_is_blank(&line.line)
}

/// Push `count` blank lines, discounting one when `lines` already ends with a
/// blank row.
///
/// Content may already end with a blank row (e.g. agent text ending in `\n\n`
/// or markdown paragraph gaps). Without the discount the content blank and the
/// requested inter-block gap stack (2 rows for the default spacing of 1).
/// With it the boundary contributes exactly `count` rows, preserving the
/// `message_block_spacing` 0–2 range while keeping the cozy rhythm.
pub(super) fn push_spacing_blanks(lines: &mut Vec<Line<'static>>, count: usize) {
    let mut remaining = count;
    if lines.last().is_some_and(line_is_blank) {
        remaining = remaining.saturating_sub(1);
    }
    lines.extend(std::iter::repeat_with(Line::default).take(remaining));
}

/// [`push_spacing_blanks`] for reflowed transcript lines.
pub(super) fn push_spacing_transcript_lines(lines: &mut Vec<TranscriptLine>, count: usize) {
    let mut remaining = count;
    if lines.last().is_some_and(transcript_line_is_blank) {
        remaining = remaining.saturating_sub(1);
    }
    lines.extend(std::iter::repeat_with(TranscriptLine::default).take(remaining));
}

/// Remove trailing blank transcript rows so the caller can append exactly one
/// inter-block separator. Prevents content trailing blanks (e.g. markdown
/// paragraph gaps, `\n\n` endings) from stacking with message gaps.
pub(super) fn trim_trailing_blank_transcript_lines(lines: &mut Vec<TranscriptLine>) {
    while lines.last().is_some_and(transcript_line_is_blank) {
        lines.pop();
    }
}

pub(super) fn agent_code_continuation_prefix(message: &MessageLine) -> Option<String> {
    let first_segment = message.segments.iter().find(|segment| !segment.text.is_empty())?;
    if !first_segment.style.effects.contains(Effects::DIMMED) {
        return None;
    }

    numbered_code_gutter_prefix(&first_segment.text)
}

fn numbered_code_gutter_prefix(text: &str) -> Option<String> {
    let mut chars = text.char_indices().peekable();
    let mut prefix_end = 0usize;

    while let Some((idx, ch)) = chars.peek().copied() {
        if ch == ' ' {
            prefix_end = idx + ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }

    let mut saw_digits = false;
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            saw_digits = true;
            prefix_end = idx + ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    if !saw_digits {
        return None;
    }

    if let Some((idx, '-')) = chars.peek().copied() {
        prefix_end = idx + 1;
        chars.next();

        let mut saw_range_digits = false;
        while let Some((idx, ch)) = chars.peek().copied() {
            if ch.is_ascii_digit() {
                saw_range_digits = true;
                prefix_end = idx + ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        if !saw_range_digits {
            return None;
        }
    }

    let mut trailing_spaces = 0usize;
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch == ' ' {
            trailing_spaces += 1;
            prefix_end = idx + ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    if trailing_spaces < 2 {
        return None;
    }

    Some(" ".repeat(UnicodeWidthStr::width(&text[..prefix_end])))
}

/// Background of a diff content row, if it carries a tinted band.
///
/// Requires both a painted background and a diff marker (`+`/`-`/space body
/// marker, `---`/`+++` file header, or `@@` hunk header) so ordinary tool
/// output that happens to be styled never gets diff treatment.
/// ANSI16/no-color foreground-only rows (no bg) return `None` by design.
pub(super) fn diff_row_bg(spans: &[Span<'_>]) -> Option<Color> {
    // Side-by-side stream rows contain an unpainted outer divider between two
    // independently tinted panes. Do not extend either pane's tint through
    // the reflow padding or into the empty sibling pane.
    let has_uncolored_divider = spans
        .iter()
        .any(|span| span.content.trim() == "│" && matches!(span.style.bg, None | Some(Color::Reset)));
    if has_uncolored_divider {
        return None;
    }
    let bg = spans.iter().find_map(|span| span.style.bg.filter(|bg| *bg != Color::Reset))?;
    let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
    let trimmed = text.trim_start();
    if trimmed.starts_with("--- ")
        || trimmed.starts_with("+++ ")
        || trimmed.starts_with("@@")
        || matches!(spans.iter().find_map(|span| span.content.chars().next()), Some('+' | '-' | ' '))
    {
        Some(bg)
    } else {
        None
    }
}

/// Whether spans form a diff row (header or body), including the
/// foreground-only fallback.
///
/// Matches `---`/`+++`/`@@` headers and `+`/`-` bodies by marker plus a diff
/// foreground (red/green/cyan family) so fg-only rows stay exempt from DIM
/// and prose justification. Ordinary bullets (`- foo`) or tool output
/// without a diff fg never match.
pub(super) fn is_diff_row_spans(spans: &[Span<'_>]) -> bool {
    if diff_row_bg(spans).is_some() {
        return true;
    }
    let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
    let trimmed = text.trim_start();
    let is_marker = trimmed.starts_with("@@") || matches!(trimmed.chars().next(), Some('+' | '-'));
    if !is_marker {
        return false;
    }
    let is_header = trimmed.starts_with("--- ") || trimmed.starts_with("+++ ") || trimmed.starts_with("@@");
    spans.iter().any(|span| {
        matches!(
            span.style.fg,
            Some(
                Color::Red
                    | Color::LightRed
                    | Color::Green
                    | Color::LightGreen
                    | Color::Cyan
                    | Color::LightCyan
                    | Color::Rgb(255, 90, 90)
                    | Color::Rgb(255, 180, 180)
                    | Color::Rgb(85, 255, 85)
                    | Color::Rgb(140, 20, 25)
                    | Color::Rgb(0, 92, 43)
            )
        ) || (is_header && matches!(span.style.fg, Some(Color::Indexed(_))))
    })
}

pub(super) fn split_tool_spans(spans: Vec<Span<'static>>) -> Vec<Vec<Span<'static>>> {
    let mut lines: Vec<Vec<Span<'static>>> = Vec::with_capacity(spans.len());
    let mut current: Vec<Span<'static>> = Vec::with_capacity(spans.len());

    for span in spans {
        let style = span.style;
        let text = span.content.into_owned();
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                current.push(Span::styled(part.to_string(), style));
            }
            if parts.peek().is_some() {
                lines.push(std::mem::take(&mut current));
            }
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::core_tui::types::{InlineSegment, InlineTextStyle};
    use std::sync::Arc;

    fn message_line(kind: InlineMessageKind, text: &str) -> MessageLine {
        MessageLine {
            diagnostic: None,
            kind,
            segments: vec![InlineSegment {
                text: text.to_string(),
                style: Arc::new(InlineTextStyle::default()),
            }],
            link_ranges: Vec::new(),
            revision: 0,
        }
    }

    fn text_line(text: &str) -> TranscriptLine {
        TranscriptLine {
            line: Line::from(text.to_string()),
            explicit_links: Vec::new(),
        }
    }

    #[test]
    fn line_is_blank_covers_empty_and_whitespace_rows() {
        assert!(line_is_blank(&Line::default()));
        assert!(line_is_blank(&Line::from("   ".to_string())));
        assert!(!line_is_blank(&Line::from("answer".to_string())));
    }

    #[test]
    fn push_spacing_discounts_an_existing_trailing_blank() {
        let mut lines = vec![text_line("answer"), TranscriptLine::default()];
        push_spacing_transcript_lines(&mut lines, 1);
        assert_eq!(lines.len(), 2, "existing blank + requested 1 must not stack");

        let mut lines = vec![text_line("answer"), TranscriptLine::default()];
        push_spacing_transcript_lines(&mut lines, 2);
        assert_eq!(lines.len(), 3, "existing blank discounts exactly one row");
    }

    #[test]
    fn push_spacing_blanks_covers_plain_lines() {
        let mut lines = vec![Line::from("tool output".to_string())];
        push_spacing_blanks(&mut lines, 1);
        assert_eq!(lines.len(), 2);

        push_spacing_blanks(&mut lines, 1);
        assert_eq!(lines.len(), 2, "existing blank + requested 1 must not stack");

        push_spacing_blanks(&mut lines, 0);
        assert_eq!(lines.len(), 2, "zero requested rows must not add rows");
    }

    #[test]
    fn trim_trailing_blank_transcript_lines_keeps_head_content() {
        let mut lines = vec![
            text_line("answer"),
            TranscriptLine::default(),
            TranscriptLine::default(),
        ];
        trim_trailing_blank_transcript_lines(&mut lines);
        assert_eq!(lines.len(), 1);
    }

    fn fg_span(text: &str, fg: Color) -> Span<'static> {
        Span::styled(text.to_owned(), Style::default().fg(fg))
    }

    fn bg_span(text: &str, bg: Color) -> Span<'static> {
        Span::styled(text.to_owned(), Style::default().bg(bg))
    }

    #[test]
    fn is_diff_row_spans_matches_tint_and_foreground_only_rows() {
        let tint = Color::Rgb(20, 58, 45);
        assert!(is_diff_row_spans(&[bg_span("+ new line", tint)]));
        assert!(is_diff_row_spans(&[fg_span("+ new line", Color::LightGreen)]));
        assert!(is_diff_row_spans(&[fg_span("- dark deletion", Color::Rgb(255, 180, 180))]));
        assert!(is_diff_row_spans(&[fg_span("+ light insertion", Color::Rgb(0, 92, 43))]));
        assert!(is_diff_row_spans(&[fg_span("- light deletion", Color::Rgb(140, 20, 25))]));
        assert!(is_diff_row_spans(&[fg_span("+ dark insertion", Color::Rgb(85, 255, 85))]));
        assert!(is_diff_row_spans(&[fg_span("@@ -100 +100 @@", Color::Cyan)]));
        assert!(is_diff_row_spans(&[fg_span("--- a/README.md", Color::LightRed)]));
        assert!(is_diff_row_spans(&[fg_span("+++ b/main.rs", Color::Indexed(42))]));
        assert!(is_diff_row_spans(&[bg_span("+++ b/README.md", Color::Indexed(42))]));
        assert!(is_diff_row_spans(&[Span::raw("    "), fg_span("+ indented", Color::LightGreen)]));
        assert!(!is_diff_row_spans(&[fg_span("--- separator", Color::Gray)]));
        assert!(!is_diff_row_spans(&[fg_span("- indexed bullet", Color::Indexed(42))]));
        // Asymmetric: same `-` marker, gray bullet fg must not match.
        assert!(!is_diff_row_spans(&[fg_span("- bullet item", Color::Gray)]));
        assert!(!is_diff_row_spans(&[bg_span("bolt normal output", Color::Black)]));
        assert_eq!(diff_row_bg(&[fg_span("+ new line", Color::LightGreen)]), None);
        assert_eq!(diff_row_bg(&[bg_span("+ new line", tint)]), Some(tint));
    }

    #[test]
    fn next_is_tool_block_covers_tool_pty_and_summaries() {
        assert!(next_is_tool_block(Some(&message_line(InlineMessageKind::Tool, "• Ran x"))));
        assert!(next_is_tool_block(Some(&message_line(InlineMessageKind::Pty, "output"))));
        assert!(next_is_tool_block(Some(&message_line(InlineMessageKind::Info, "• Ran x"))));
        assert!(!next_is_tool_block(Some(&message_line(InlineMessageKind::Agent, "answer"))));
        assert!(!next_is_tool_block(Some(&message_line(InlineMessageKind::Info, "plain status"))));
        assert!(!next_is_tool_block(None));
    }
}
