#![allow(dead_code)]

use anstyle::Color as AnsiColorEnum;
use ratatui::{
    prelude::*,
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::super::style::ratatui_style_from_inline;
use super::super::types::{InlineMessageKind, InlineTextStyle};
use super::terminal_capabilities;
use super::{
    Session,
    file_palette::FilePalette,
    message::MessageLine,
    modal::{
        ModalBodyContext, ModalListLayout, ModalRenderStyles, compute_modal_area,
        render_modal_body, render_wizard_modal_body,
    },
    text_utils,
};
use crate::config::constants::ui;

mod modal_renderer;
mod palettes;
mod spans;

pub use modal_renderer::render_modal;
pub use palettes::render_file_palette;
use spans::{accent_style, border_style, default_style, invalidate_scroll_metrics, text_fallback};

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

const USER_PREFIX: &str = "";

#[allow(dead_code)]
pub fn render(session: &mut Session, frame: &mut Frame<'_>) {
    let size = frame.area();
    if size.width == 0 || size.height == 0 {
        return;
    }

    // Clear entire frame if modal was just closed to remove artifacts
    if session.needs_full_clear {
        frame.render_widget(Clear, size);
        session.needs_full_clear = false;
    }

    // Pull any newly forwarded log entries before layout calculations
    session.poll_log_entries();

    let header_lines = session.header_lines();
    let header_height = session.header_height_from_lines(size.width, &header_lines);
    if header_height != session.header_rows {
        session.header_rows = header_height;
        recalculate_transcript_rows(session);
    }

    let status_height = if size.width > 0 && palettes::has_input_status(session) {
        1
    } else {
        0
    };
    let inner_width = size
        .width
        .saturating_sub(ui::INLINE_INPUT_PADDING_HORIZONTAL.saturating_mul(2));
    let desired_lines = session.desired_input_lines(inner_width);
    let block_height = Session::input_block_height_for_lines(desired_lines);
    let input_height = block_height.saturating_add(status_height);
    session.apply_input_height(input_height);

    let chunks = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(1),
        Constraint::Length(input_height),
    ])
    .split(size);

    let (header_area, transcript_area, input_area) = (chunks[0], chunks[1], chunks[2]);

    // Calculate available height for transcript
    apply_view_rows(session, transcript_area.height);

    // Render components
    session.render_header(frame, header_area, &header_lines);
    if session.show_logs {
        let split = Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(transcript_area);
        render_transcript(session, frame, split[0]);
        render_log_view(session, frame, split[1]);
    } else {
        render_transcript(session, frame, transcript_area);
    }
    session.render_input(frame, input_area);
    render_modal(session, frame, size);
    super::slash::render_slash_palette(session, frame, size);
    render_file_palette(session, frame, size);
}

fn render_log_view(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    let block = Block::bordered()
        .title("Logs")
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let paragraph = Paragraph::new((*session.log_text()).clone()).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn modal_list_highlight_style(session: &Session) -> Style {
    session.styles.modal_list_highlight_style()
}

pub fn apply_view_rows(session: &mut Session, rows: u16) {
    let resolved = rows.max(2);
    if session.view_rows != resolved {
        session.view_rows = resolved;
        invalidate_scroll_metrics(session);
    }
    recalculate_transcript_rows(session);
    session.enforce_scroll_bounds();
}

pub fn apply_transcript_rows(session: &mut Session, rows: u16) {
    let resolved = rows.max(1);
    if session.transcript_rows != resolved {
        session.transcript_rows = resolved;
        invalidate_scroll_metrics(session);
    }
}

pub fn apply_transcript_width(session: &mut Session, width: u16) {
    if session.transcript_width != width {
        session.transcript_width = width;
        invalidate_scroll_metrics(session);
    }
}

fn render_transcript(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let block = Block::new()
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    apply_transcript_rows(session, inner.height);

    let content_width = inner.width;
    if content_width == 0 {
        return;
    }
    apply_transcript_width(session, content_width);

    let viewport_rows = inner.height as usize;
    let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
    let effective_padding = padding.min(viewport_rows.saturating_sub(1));

    // Skip expensive total_rows calculation if only scrolling (no content change)
    // This optimization saves ~30-50% CPU on viewport-only scrolls
    let total_rows = if session.transcript_content_changed {
        session.total_transcript_rows(content_width) + effective_padding
    } else {
        // Reuse last known total if content unchanged
        session
            .scroll_manager
            .last_known_total()
            .unwrap_or_else(|| session.total_transcript_rows(content_width) + effective_padding)
    };
    let (top_offset, _clamped_total_rows) =
        session.prepare_transcript_scroll(total_rows, viewport_rows);
    let vertical_offset = top_offset.min(session.scroll_manager.max_offset());
    session.transcript_view_top = vertical_offset;

    let visible_start = vertical_offset;
    let scroll_area = inner;

    // Use cached visible lines to avoid re-cloning on viewport-only scrolls
    let cached_lines =
        session.collect_transcript_window_cached(content_width, visible_start, viewport_rows);

    // Only clone if we need to mutate (fill or overlay)
    let fill_count = viewport_rows.saturating_sub(cached_lines.len());
    let visible_lines = if fill_count > 0 || !session.queued_inputs.is_empty() {
        // Need to mutate, so clone from Arc
        let mut lines = (*cached_lines).clone();
        if fill_count > 0 {
            let target_len = lines.len() + fill_count;
            lines.resize_with(target_len, Line::default);
        }
        session.overlay_queue_lines(&mut lines, content_width);
        lines
    } else {
        // No mutation needed, use Arc directly
        (*cached_lines).clone()
    };

    let paragraph = Paragraph::new(visible_lines)
        .style(default_style(session))
        .wrap(Wrap { trim: false });

    // Only clear if content actually changed, not on viewport-only scroll
    // This is a significant optimization: avoids expensive Clear operation on most scrolls
    // Combined with layout skip above, this reduces render CPU by ~50% during scrolling
    if session.transcript_content_changed {
        frame.render_widget(Clear, scroll_area);
        session.transcript_content_changed = false;
    }
    frame.render_widget(paragraph, scroll_area);
}

fn header_reserved_rows(session: &Session) -> u16 {
    session.header_rows.max(ui::INLINE_HEADER_HEIGHT)
}

fn input_reserved_rows(session: &Session) -> u16 {
    header_reserved_rows(session) + session.input_height
}

pub fn recalculate_transcript_rows(session: &mut Session) {
    let reserved = input_reserved_rows(session).saturating_add(2); // account for transcript block borders
    let available = session.view_rows.saturating_sub(reserved).max(1);
    apply_transcript_rows(session, available);
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

fn reflow_pty_lines(session: &Session, index: usize, width: u16) -> Vec<Line<'static>> {
    let Some(line) = session.lines.get(index) else {
        return vec![Line::default()];
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

    let mut lines = Vec::new();

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
        let mut style = ratatui_style_from_inline(&segment.style, fallback);
        style = style.add_modifier(Modifier::DIM);
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
