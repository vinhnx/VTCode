/// Transcript reflow and wrapping operations for Session
///
/// This module handles transcript line wrapping, reflowing, and formatting including:
/// - Message line reflowing based on width
/// - Text wrapping and justification
/// - Tool and PTY output formatting with borders
/// - Diff line padding
/// - Block line wrapping with borders
use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::super::style::ratatui_style_from_inline;
use super::super::types::InlineMessageKind;
use super::{Session, message::MessageLine, render, terminal_capabilities, text_utils};
use crate::config::constants::ui;

mod blocks;
mod formatting;
mod helpers;

use helpers::{block_chars, is_info_box_line, is_tool_summary_line, truncate_line_to_width};

impl Session {
    /// Reflow message lines for a given width (test-only method)
    #[cfg(test)]
    pub(super) fn reflow_transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            let mut lines: Vec<Line<'static>> = Vec::new();
            for (index, _) in self.lines.iter().enumerate() {
                lines.extend(self.reflow_message_lines(index, 0));
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            return lines;
        }

        let mut wrapped_lines = Vec::new();
        for (index, _) in self.lines.iter().enumerate() {
            wrapped_lines.extend(self.reflow_message_lines(index, width));
        }

        if wrapped_lines.is_empty() {
            wrapped_lines.push(Line::default());
        }

        wrapped_lines
    }

    /// Reflow a specific message line based on its type and width
    ///
    /// This method creates visually grouped message blocks with:
    /// - Role headers for User/Agent messages
    /// - Subtle dividers between conversation turns
    /// - Consistent spacing between message blocks
    /// - Tool output grouped with headers
    #[allow(dead_code)]
    pub(super) fn reflow_message_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(message) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        if message.kind == InlineMessageKind::Tool {
            return self.reflow_tool_lines(index, width);
        }

        if message.kind == InlineMessageKind::Pty {
            return self.reflow_pty_lines(index, width);
        }

        if is_info_box_line(message) {
            if index > 0
                && self
                    .lines
                    .get(index - 1)
                    .is_some_and(|prev| prev.kind == message.kind && is_info_box_line(prev))
            {
                return Vec::new();
            }
            return self.reflow_error_warning_lines(index, width);
        }

        let spans = render::render_message_spans(self, index);
        let base_line = Line::from(spans);
        if width == 0 {
            return vec![base_line];
        }

        let mut wrapped = Vec::new();
        let max_width = width as usize;

        // Check if this is the start of a new conversation turn
        let prev_kind = if index > 0 {
            self.lines.get(index - 1).map(|l| l.kind)
        } else {
            None
        };
        let is_new_turn = prev_kind.is_none()
            || (message.kind == InlineMessageKind::User
                && prev_kind != Some(InlineMessageKind::User))
            || (message.kind == InlineMessageKind::Agent
                && prev_kind != Some(InlineMessageKind::Agent));

        let spacing = self.appearance.message_block_spacing.min(2) as usize;

        // Add a subtle separator before User messages (single divider, not double)
        if message.kind == InlineMessageKind::User && is_new_turn && max_width > 0 {
            if prev_kind.is_some() {
                for _ in 0..spacing {
                    wrapped.push(Line::default());
                }
            }
            let divider = self.message_divider_line(max_width, message.kind);
            wrapped.push(divider);
        }

        let lines = if message.kind == InlineMessageKind::Agent {
            self.reflow_agent_message_lines(message, max_width, !is_new_turn)
        } else {
            let mut lines = self.wrap_line(base_line, max_width);
            if !lines.is_empty() {
                lines = self.justify_wrapped_lines(lines, max_width, message.kind);
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            lines
        };

        wrapped.extend(lines);

        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        // Add spacing after messages for visual grouping (respects message_block_spacing config)
        let next_line = self.lines.get(index + 1);
        let next_kind = next_line.map(|l| l.kind);
        match message.kind {
            InlineMessageKind::Error | InlineMessageKind::Info | InlineMessageKind::Warning => {
                let skip_spacing = is_tool_summary_line(message)
                    && match next_line {
                        Some(next) if next.kind == InlineMessageKind::Info => {
                            is_tool_summary_line(next)
                        }
                        Some(next) if next.kind == InlineMessageKind::Tool => true,
                        _ => false,
                    };
                if !skip_spacing {
                    for _ in 0..spacing {
                        wrapped.push(Line::default());
                    }
                }
            }
            InlineMessageKind::Policy => {
                // No spacing if followed by Agent (reasoning -> content flow)
                if next_kind != Some(InlineMessageKind::Agent) {
                    for _ in 0..spacing {
                        wrapped.push(Line::default());
                    }
                }
            }
            InlineMessageKind::User => {
                // Blank lines after user message for clean separation
                for _ in 0..spacing {
                    wrapped.push(Line::default());
                }
            }
            InlineMessageKind::Agent => {
                // Check if next message is a different type (end of agent turn)
                if next_kind.is_some() && next_kind != Some(InlineMessageKind::Agent) {
                    for _ in 0..spacing {
                        wrapped.push(Line::default());
                    }
                }
            }
            _ => {}
        }

        wrapped
    }

    /// Reflow error, warning, and info messages with a bordered block.
    #[allow(dead_code)]
    pub(super) fn reflow_error_warning_lines(
        &self,
        index: usize,
        width: u16,
    ) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        let max_width = if width == 0 {
            usize::MAX
        } else {
            width as usize
        };

        let mut grouped_lines = Vec::new();
        let mut cursor = index;
        while let Some(current) = self.lines.get(cursor) {
            if current.kind != line.kind || !is_info_box_line(current) {
                break;
            }
            let mut spans = render::render_message_spans(self, cursor);
            for span in &mut spans {
                span.style = span.style.remove_modifier(Modifier::BOLD);
            }
            let line_text: String = spans.iter().map(|span| &*span.content).collect();
            if !line_text.trim().is_empty() {
                grouped_lines.push(Line::from(spans));
            }
            cursor = cursor.saturating_add(1);
        }

        if grouped_lines.is_empty() {
            return Vec::new();
        }

        if max_width == usize::MAX {
            return grouped_lines;
        }

        let border_style = self.styles.dimmed_border_style(true);

        let border_type = terminal_capabilities::get_border_type();
        let border = block_chars(border_type);
        let label = match line.kind {
            InlineMessageKind::Error => "Error",
            InlineMessageKind::Warning => "Warning",
            InlineMessageKind::Info => "Info",
            _ => "",
        };

        let body_prefix = format!("  {} ", border.vertical);
        let prefix_width = body_prefix.chars().count();
        let border_width = border.vertical.chars().count();
        let content_width = max_width.saturating_sub(prefix_width + border_width);

        if content_width == 0 {
            return grouped_lines;
        }

        let inner_width = content_width + 1;
        let top_inner = if label.is_empty() {
            border.horizontal.repeat(inner_width)
        } else {
            let label_segment = format!(" {} ", label);
            let label_width = label_segment.chars().count();
            let base_width = label_width + 2;
            if inner_width <= base_width {
                border.horizontal.repeat(inner_width)
            } else {
                let mut inner = String::new();
                inner.push_str(border.horizontal);
                inner.push_str(&label_segment);
                inner.push_str(border.horizontal);
                let remaining = inner_width.saturating_sub(base_width);
                inner.push_str(&border.horizontal.repeat(remaining));
                inner
            }
        };
        let top = format!("  {}{}{}", border.top_left, top_inner, border.top_right);
        let bottom = format!(
            "  {}{}{}",
            border.bottom_left,
            border.horizontal.repeat(inner_width),
            border.bottom_right
        );

        let mut lines = Vec::new();
        lines.push(Line::styled(top, border_style));
        let mut wrapped = Vec::new();
        for line in grouped_lines {
            let line_wrapped = self.wrap_line(line, content_width);
            for wrapped_line in line_wrapped {
                let text: String = wrapped_line
                    .spans
                    .iter()
                    .map(|span| &*span.content)
                    .collect();
                if !text.trim().is_empty() {
                    wrapped.push(wrapped_line);
                }
            }
        }
        if wrapped.is_empty() {
            return Vec::new();
        }
        for line in &mut wrapped {
            let line_width = line.spans.iter().map(|s| s.width()).sum::<usize>();
            let padding = content_width.saturating_sub(line_width);
            let mut new_spans = vec![Span::styled(body_prefix.to_owned(), border_style)];
            new_spans.append(&mut line.spans);
            if padding > 0 {
                new_spans.push(Span::styled(" ".repeat(padding), Style::default()));
            }
            new_spans.push(Span::styled(border.vertical.to_owned(), border_style));
            line.spans = new_spans;
        }
        lines.extend(wrapped);
        lines.push(Line::styled(bottom, border_style));

        let spacing = self.appearance.message_block_spacing.min(2) as usize;
        for _ in 0..spacing {
            lines.push(Line::default());
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }

    /// Wrap a line of text to fit within a maximum width
    pub(super) fn wrap_line(&self, line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
        text_utils::wrap_line(line, max_width)
    }

    fn reflow_agent_message_lines(
        &self,
        message: &MessageLine,
        max_width: usize,
        suppress_prefix_bullet: bool,
    ) -> Vec<Line<'static>> {
        if max_width == 0 {
            return vec![Line::default()];
        }

        let mut prefix_spans = render::agent_prefix_spans(self, message);
        let left_padding = ui::INLINE_AGENT_MESSAGE_LEFT_PADDING;
        let prefix_width = prefix_spans.iter().map(|span| span.width()).sum::<usize>()
            + UnicodeWidthStr::width(left_padding);

        let content_width = max_width.saturating_sub(prefix_width);
        let fallback = self.text_fallback(message.kind).or(self.theme.foreground);
        let mut content_spans = Vec::new();
        for segment in &message.segments {
            let style = ratatui_style_from_inline(&segment.style, fallback);
            content_spans.push(Span::styled(segment.text.clone(), style));
        }

        let is_table_line = message.segments.iter().any(|seg| {
            let t = &seg.text;
            t.contains('│') || t.contains('├') || t.contains('┤') || t.contains('┼')
        });

        let mut wrapped = if content_width == 0 {
            vec![Line::default()]
        } else if is_table_line {
            // Table lines must not be word-wrapped - wrapping breaks box-drawing
            // alignment. Truncate to the available width instead.
            vec![truncate_line_to_width(
                Line::from(content_spans),
                content_width,
            )]
        } else {
            self.wrap_line(Line::from(content_spans), content_width)
        };

        if !wrapped.is_empty() {
            wrapped = self.justify_wrapped_lines(wrapped, content_width, message.kind);
        }
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        let suppress_prefix_bullet = suppress_prefix_bullet
            || wrapped.first().is_some_and(|line| {
                let line_text = line
                    .spans
                    .iter()
                    .map(|span| AsRef::<str>::as_ref(&span.content))
                    .collect::<String>();
                let stripped = text_utils::strip_ansi_codes(&line_text);
                text_utils::is_list_item(stripped.as_ref())
            });
        if suppress_prefix_bullet
            && !ui::INLINE_AGENT_QUOTE_PREFIX.is_empty()
            && let Some(prefix_span) = prefix_spans
                .first_mut()
                .filter(|span| AsRef::<str>::as_ref(&span.content) == ui::INLINE_AGENT_QUOTE_PREFIX)
        {
            let replacement = " ".repeat(UnicodeWidthStr::width(ui::INLINE_AGENT_QUOTE_PREFIX));
            prefix_span.content = replacement.into();
        }

        let indent = " ".repeat(prefix_width);
        let mut lines = Vec::with_capacity(wrapped.len());
        for (index, mut line) in wrapped.into_iter().enumerate() {
            let mut spans = Vec::new();
            if index == 0 {
                spans.append(&mut prefix_spans);
                if !left_padding.is_empty() {
                    spans.push(Span::raw(left_padding));
                }
            } else if !indent.is_empty() {
                spans.push(Span::raw(indent.clone()));
            }
            spans.append(&mut line.spans);
            if spans.is_empty() {
                spans.push(Span::raw(String::new()));
            }
            lines.push(Line::from(spans));
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }
}
