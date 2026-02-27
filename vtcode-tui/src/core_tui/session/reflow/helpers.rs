use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::config::constants::ui;

use super::super::super::types::InlineMessageKind;
use super::super::message::MessageLine;

#[derive(Clone, Copy)]
pub(super) struct BlockChars {
    pub(super) top_left: &'static str,
    pub(super) top_right: &'static str,
    pub(super) bottom_left: &'static str,
    pub(super) bottom_right: &'static str,
    pub(super) horizontal: &'static str,
    pub(super) vertical: &'static str,
}

pub(super) fn block_chars(border_type: ratatui::widgets::BorderType) -> BlockChars {
    match border_type {
        ratatui::widgets::BorderType::Rounded => BlockChars {
            top_left: ui::INLINE_BLOCK_TOP_LEFT,
            top_right: ui::INLINE_BLOCK_TOP_RIGHT,
            bottom_left: ui::INLINE_BLOCK_BOTTOM_LEFT,
            bottom_right: ui::INLINE_BLOCK_BOTTOM_RIGHT,
            horizontal: ui::INLINE_BLOCK_HORIZONTAL,
            vertical: ui::INLINE_BLOCK_BODY_LEFT,
        },
        _ => BlockChars {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            horizontal: "-",
            vertical: "|",
        },
    }
}

pub(super) fn is_tool_summary_line(message: &MessageLine) -> bool {
    let text: String = message
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    let trimmed = text.trim_start();
    trimmed.starts_with("• ") || trimmed.starts_with("└ ")
}

pub(super) fn split_tool_spans(spans: Vec<Span<'static>>) -> Vec<Vec<Span<'static>>> {
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();

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

pub(super) fn is_info_box_line(message: &MessageLine) -> bool {
    matches!(
        message.kind,
        InlineMessageKind::Error | InlineMessageKind::Warning
    ) || (message.kind == InlineMessageKind::Info && !is_tool_summary_line(message))
}

/// Collapse multiple consecutive newlines (3 or more) into at most 2 newlines.
/// Returns Cow to avoid allocation when no changes are needed.
#[allow(dead_code)]
pub(super) fn collapse_excess_newlines(text: &str) -> std::borrow::Cow<'_, str> {
    // Quick check: if no triple newlines, return borrowed
    if !text.contains("\n\n\n") {
        return std::borrow::Cow::Borrowed(text);
    }

    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut newline_count = 0;

    while let Some(ch) = chars.next() {
        if ch == '\n' {
            newline_count += 1;
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '\n' {
                    chars.next();
                    newline_count += 1;
                } else {
                    break;
                }
            }

            let newlines_to_add = std::cmp::min(newline_count, 2);
            for _ in 0..newlines_to_add {
                result.push('\n');
            }
            newline_count = 0;
        } else {
            result.push(ch);
            if ch != '\n' {
                newline_count = 0;
            }
        }
    }

    std::borrow::Cow::Owned(result)
}

/// Truncate a `Line` to fit within `max_width` display columns.
///
/// Used for table lines where word-wrapping would break the box-drawing
/// alignment. Spans are trimmed at the character boundary that exceeds the
/// width; any remaining spans are dropped.
pub(super) fn truncate_line_to_width(line: Line<'static>, max_width: usize) -> Line<'static> {
    let total: usize = line.spans.iter().map(|s| s.width()).sum();
    if total <= max_width {
        return line;
    }

    let mut remaining = max_width;
    let mut truncated_spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len());
    for span in line.spans {
        let span_width = span.width();
        if span_width <= remaining {
            remaining -= span_width;
            truncated_spans.push(span);
        } else {
            // Truncate within this span at a char boundary
            let mut chars_width = 0usize;
            let mut byte_end = 0usize;
            for ch in span.content.chars() {
                let cw = UnicodeWidthStr::width(ch.encode_utf8(&mut [0u8; 4]) as &str);
                if chars_width + cw > remaining {
                    break;
                }
                chars_width += cw;
                byte_end += ch.len_utf8();
            }
            if byte_end > 0 {
                let fragment: String = span.content[..byte_end].to_string();
                truncated_spans.push(Span::styled(fragment, span.style));
            }
            break;
        }
    }
    Line::from(truncated_spans)
}
