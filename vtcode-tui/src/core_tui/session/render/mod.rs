#![allow(dead_code)]

use anstyle::Color as AnsiColorEnum;
use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::super::style::{ratatui_pty_style_from_inline, ratatui_style_from_inline};
use super::super::types::{InlineMessageKind, InlineTextStyle};
use super::{Session, TranscriptLine, file_palette::FilePalette, message::MessageLine, text_utils};
use crate::config::constants::ui;

mod history_picker;
mod modal_renderer;
mod palettes;
mod spans;

pub use history_picker::{render_history_picker, split_inline_history_picker_area};
pub(crate) use modal_renderer::modal_render_styles;
pub use modal_renderer::render_modal;
pub use modal_renderer::split_inline_modal_area;
pub use palettes::{render_file_palette, split_inline_file_palette_area};
use spans::{accent_style, border_style, default_style, text_fallback};

pub(super) fn render_message_spans(session: &Session, index: usize) -> Vec<Span<'static>> {
    spans::render_message_spans(session, index)
}

pub(super) fn agent_prefix_spans(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    spans::agent_prefix_spans(session, line)
}

pub(super) fn strip_ansi_codes(text: &str) -> std::borrow::Cow<'_, str> {
    spans::strip_ansi_codes(text)
}

pub(super) fn render_tool_segments(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    spans::render_tool_segments(session, line)
}

#[allow(dead_code)]
pub fn render(session: &mut Session, frame: &mut Frame<'_>) {
    session.render(frame);
}

fn modal_list_highlight_style(session: &Session) -> Style {
    session.styles.modal_list_highlight_style()
}

pub fn apply_view_rows(session: &mut Session, rows: u16) {
    session.apply_view_rows(rows);
}

pub fn apply_transcript_rows(session: &mut Session, rows: u16) {
    session.apply_transcript_rows(rows);
}

pub fn apply_transcript_width(session: &mut Session, width: u16) {
    session.apply_transcript_width(width);
}

pub fn recalculate_transcript_rows(session: &mut Session) {
    session.recalculate_transcript_rows();
}
fn wrap_block_lines(
    session: &Session,
    first_prefix: &str,
    continuation_prefix: &str,
    content: Vec<Span<'static>>,
    max_width: usize,
    border_style: Style,
) -> Vec<Line<'static>> {
    wrap_block_lines_with_options(
        session,
        first_prefix,
        continuation_prefix,
        content,
        max_width,
        border_style,
        true,
    )
}

fn wrap_block_lines_no_right_border(
    session: &Session,
    first_prefix: &str,
    continuation_prefix: &str,
    content: Vec<Span<'static>>,
    max_width: usize,
    border_style: Style,
) -> Vec<Line<'static>> {
    wrap_block_lines_with_options(
        session,
        first_prefix,
        continuation_prefix,
        content,
        max_width,
        border_style,
        false,
    )
}

fn wrap_block_lines_with_options(
    session: &Session,
    first_prefix: &str,
    continuation_prefix: &str,
    content: Vec<Span<'static>>,
    max_width: usize,
    border_style: Style,
    show_right_border: bool,
) -> Vec<Line<'static>> {
    if max_width < 2 {
        let fallback = if show_right_border {
            format!("{}││", first_prefix)
        } else {
            format!("{}│", first_prefix)
        };
        return vec![Line::from(fallback).style(border_style)];
    }

    let right_border = if show_right_border {
        ui::INLINE_BLOCK_BODY_RIGHT
    } else {
        ""
    };
    let first_prefix_width = first_prefix.chars().count();
    let continuation_prefix_width = continuation_prefix.chars().count();
    let prefix_width = first_prefix_width.max(continuation_prefix_width);
    let border_width = right_border.chars().count();
    let consumed_width = prefix_width.saturating_add(border_width);
    let content_width = max_width.saturating_sub(consumed_width);

    if max_width == usize::MAX {
        let mut spans = vec![Span::styled(first_prefix.to_owned(), border_style)];
        spans.extend(content);
        if show_right_border {
            spans.push(Span::styled(right_border.to_owned(), border_style));
        }
        return vec![Line::from(spans)];
    }

    let mut wrapped = wrap_line(session, Line::from(content), content_width);
    if wrapped.is_empty() {
        wrapped.push(Line::default());
    }

    // Add borders to each wrapped line
    for (idx, line) in wrapped.iter_mut().enumerate() {
        let line_width = line.spans.iter().map(|s| s.width()).sum::<usize>();
        let padding = if show_right_border {
            content_width.saturating_sub(line_width)
        } else {
            0
        };

        let active_prefix = if idx == 0 {
            first_prefix
        } else {
            continuation_prefix
        };
        let mut new_spans = vec![Span::styled(active_prefix.to_owned(), border_style)];
        new_spans.append(&mut line.spans);
        if padding > 0 {
            new_spans.push(Span::styled(" ".repeat(padding), Style::default()));
        }
        if show_right_border {
            new_spans.push(Span::styled(right_border.to_owned(), border_style));
        }
        line.spans = new_spans;
    }

    wrapped
}

fn pty_block_has_content(session: &Session, index: usize) -> bool {
    if session.lines.is_empty() {
        return false;
    }

    let mut start = index;
    while start > 0 {
        let Some(previous) = session.lines.get(start - 1) else {
            break;
        };
        if previous.kind != InlineMessageKind::Pty {
            break;
        }
        start -= 1;
    }

    let mut end = index;
    while end + 1 < session.lines.len() {
        let Some(next) = session.lines.get(end + 1) else {
            break;
        };
        if next.kind != InlineMessageKind::Pty {
            break;
        }
        end += 1;
    }

    if start > end || end >= session.lines.len() {
        tracing::warn!(
            "invalid range: start={}, end={}, len={}",
            start,
            end,
            session.lines.len()
        );
        return false;
    }

    for line in &session.lines[start..=end] {
        if line
            .segments
            .iter()
            .any(|segment| !segment.text.trim().is_empty())
        {
            return true;
        }
    }

    false
}

fn reflow_pty_lines(session: &Session, index: usize, width: u16) -> Vec<TranscriptLine> {
    let Some(line) = session.lines.get(index) else {
        return vec![TranscriptLine::default()];
    };

    let max_width = if width == 0 {
        usize::MAX
    } else {
        width as usize
    };

    if !pty_block_has_content(session, index) {
        return Vec::new();
    }

    let mut border_style = ratatui_style_from_inline(
        &session.styles.tool_border_style(),
        session.theme.foreground,
    );
    border_style = border_style.add_modifier(Modifier::DIM);

    let prev_is_pty = index
        .checked_sub(1)
        .and_then(|prev| session.lines.get(prev))
        .map(|prev| prev.kind == InlineMessageKind::Pty)
        .unwrap_or(false);

    let is_start = !prev_is_pty;

    let mut lines: Vec<Line<'static>> = Vec::new();

    let mut combined = String::new();
    for segment in &line.segments {
        combined.push_str(segment.text.as_str());
    }
    if is_start && combined.trim().is_empty() {
        return Vec::new();
    }

    // Render body content - strip ANSI codes to ensure plain text output
    let fallback = text_fallback(session, InlineMessageKind::Pty).or(session.theme.foreground);
    let mut body_spans = Vec::new();
    for segment in &line.segments {
        let stripped_text = strip_ansi_codes(&segment.text);
        let style = ratatui_pty_style_from_inline(&segment.style, fallback);
        body_spans.push(Span::styled(stripped_text.into_owned(), style));
    }

    // Check if this is a thinking spinner line (skip border rendering)
    let is_thinking_spinner = combined.contains("Thinking...");

    if is_start {
        // Render body without borders - just indent with spaces for visual separation
        if is_thinking_spinner {
            // Render thinking spinner without borders
            lines.extend(wrap_block_lines_no_right_border(
                session,
                "",
                "",
                body_spans,
                max_width,
                border_style,
            ));
        } else {
            let body_prefix = "  ";
            let continuation_prefix =
                text_utils::pty_wrapped_continuation_prefix(body_prefix, combined.as_str());
            lines.extend(wrap_block_lines_no_right_border(
                session,
                body_prefix,
                continuation_prefix.as_str(),
                body_spans,
                max_width,
                border_style,
            ));
        }
    } else {
        let body_prefix = "  ";
        let continuation_prefix =
            text_utils::pty_wrapped_continuation_prefix(body_prefix, combined.as_str());
        lines.extend(wrap_block_lines_no_right_border(
            session,
            body_prefix,
            continuation_prefix.as_str(),
            body_spans,
            max_width,
            border_style,
        ));
    }

    if lines.is_empty() {
        lines.push(Line::default());
    }

    lines
        .into_iter()
        .map(|line| TranscriptLine {
            line,
            explicit_links: Vec::new(),
        })
        .collect()
}

fn message_divider_line(session: &Session, width: usize, kind: InlineMessageKind) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }

    let content = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(width);
    let style = message_divider_style(session, kind);
    Line::from(content).style(style)
}

fn message_divider_style(session: &Session, kind: InlineMessageKind) -> Style {
    session.styles.message_divider_style(kind)
}

fn wrap_line(_session: &Session, line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    text_utils::wrap_line(line, max_width)
}

fn justify_wrapped_lines(
    session: &Session,
    lines: Vec<Line<'static>>,
    max_width: usize,
    kind: InlineMessageKind,
) -> Vec<Line<'static>> {
    if max_width == 0 {
        return lines;
    }

    let total = lines.len();
    let mut justified = Vec::with_capacity(total);
    let mut in_fenced_block = false;
    for (index, line) in lines.into_iter().enumerate() {
        let is_last = index + 1 == total;
        let mut next_in_fenced_block = in_fenced_block;
        let is_fence_line = {
            let line_text_storage: std::borrow::Cow<'_, str> = if line.spans.len() == 1 {
                std::borrow::Cow::Borrowed(&*line.spans[0].content)
            } else {
                std::borrow::Cow::Owned(
                    line.spans
                        .iter()
                        .map(|span| &*span.content)
                        .collect::<String>(),
                )
            };
            let line_text: &str = &line_text_storage;
            let trimmed_start = line_text.trim_start();
            trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~")
        };
        if is_fence_line {
            next_in_fenced_block = !in_fenced_block;
        }

        // Extend diff line backgrounds to full width
        let processed_line = if is_diff_line(session, &line) {
            pad_diff_line(session, &line, max_width)
        } else if kind == InlineMessageKind::Agent
            && !in_fenced_block
            && !is_fence_line
            && should_justify_message_line(session, &line, max_width, is_last)
        {
            justify_message_line(session, &line, max_width)
        } else {
            line
        };

        justified.push(processed_line);
        in_fenced_block = next_in_fenced_block;
    }

    justified
}

fn should_justify_message_line(
    _session: &Session,
    line: &Line<'static>,
    max_width: usize,
    is_last: bool,
) -> bool {
    if is_last || max_width == 0 {
        return false;
    }
    if line.spans.len() != 1 {
        return false;
    }
    let text: &str = &line.spans[0].content;
    if text.trim().is_empty() {
        return false;
    }
    if text.starts_with(char::is_whitespace) {
        return false;
    }
    let trimmed = text.trim();
    if trimmed.starts_with(|ch: char| ['-', '*', '`', '>', '#'].contains(&ch)) {
        return false;
    }
    if trimmed.contains("```") {
        return false;
    }
    let width = UnicodeWidthStr::width(trimmed);
    if width >= max_width || width < max_width / 2 {
        return false;
    }

    justify_plain_text(text, max_width).is_some()
}

fn justify_message_line(
    _session: &Session,
    line: &Line<'static>,
    max_width: usize,
) -> Line<'static> {
    let span = &line.spans[0];
    if let Some(justified) = justify_plain_text(&span.content, max_width) {
        Line::from(justified).style(span.style)
    } else {
        line.clone()
    }
}

fn is_diff_line(_session: &Session, line: &Line<'static>) -> bool {
    // Detect actual diff lines: must start with +, -, or space (diff markers)
    // AND have background color styling applied (from git diff coloring)
    // This avoids false positives from regular text that happens to start with these chars
    if line.spans.is_empty() {
        return false;
    }

    // Check if any span has background color (diff lines from render have colored backgrounds)
    let has_bg_color = line.spans.iter().any(|span| span.style.bg.is_some());
    if !has_bg_color {
        return false;
    }

    // Must start with a diff marker character in the first span
    let first_span_char = line.spans[0].content.chars().next();
    matches!(first_span_char, Some('+') | Some('-') | Some(' '))
}

fn pad_diff_line(_session: &Session, line: &Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 || line.spans.is_empty() {
        return line.clone();
    }

    // Calculate actual display width using Unicode width rules
    let line_width: usize = line
        .spans
        .iter()
        .map(|s| {
            s.content
                .chars()
                .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1))
                .sum::<usize>()
        })
        .sum();

    let padding_needed = max_width.saturating_sub(line_width);

    if padding_needed == 0 {
        return line.clone();
    }

    let padding_style = line
        .spans
        .iter()
        .find_map(|span| span.style.bg)
        .map(|bg| Style::default().bg(bg))
        .unwrap_or_default();

    let mut new_spans = Vec::with_capacity(line.spans.len() + 1);
    new_spans.extend(line.spans.iter().cloned());
    new_spans.push(Span::styled(" ".repeat(padding_needed), padding_style));

    Line::from(new_spans)
}

fn prepare_transcript_scroll(
    session: &mut Session,
    total_rows: usize,
    viewport_rows: usize,
) -> (usize, usize) {
    let viewport = viewport_rows.max(1);
    let clamped_total = total_rows.max(1);
    session.scroll_manager.set_total_rows(clamped_total);
    session.scroll_manager.set_viewport_rows(viewport as u16);
    let max_offset = session.scroll_manager.max_offset();

    if session.scroll_manager.offset() > max_offset {
        session.scroll_manager.set_offset(max_offset);
    }

    let top_offset = max_offset.saturating_sub(session.scroll_manager.offset());
    (top_offset, clamped_total)
}

// Delegate to text_utils module
fn justify_plain_text(text: &str, max_width: usize) -> Option<String> {
    text_utils::justify_plain_text(text, max_width)
}
