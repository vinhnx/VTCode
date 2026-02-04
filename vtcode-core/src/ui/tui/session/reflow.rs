use anstyle::Effects;
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
use super::super::types::{InlineMessageKind, InlineTextStyle};
use super::{Session, message::MessageLine, render, terminal_capabilities, text_utils};
use crate::config::constants::ui;

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

        if matches!(
            message.kind,
            InlineMessageKind::Error | InlineMessageKind::Warning
        ) || (message.kind == InlineMessageKind::Info && !is_tool_summary_line(message))
        {
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

        let mut lines = self.wrap_line(base_line, max_width);
        if !lines.is_empty() {
            lines = self.justify_wrapped_lines(lines, max_width, message.kind);
        }
        if lines.is_empty() {
            lines.push(Line::default());
        }

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

        let content = render::render_message_spans(self, index);
        if max_width == usize::MAX {
            return vec![Line::from(content)];
        }

        let border_style = {
            let inline = InlineTextStyle {
                color: self.theme.secondary.or(self.theme.foreground),
                ..Default::default()
            };
            ratatui_style_from_inline(&inline, self.theme.foreground).add_modifier(Modifier::DIM)
        };

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
            return vec![Line::from(content)];
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
        let base_line = Line::from(content);
        let mut wrapped = self.wrap_line(base_line, content_width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
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

    /// Wrap content with left and right borders
    #[allow(dead_code)]
    pub(super) fn wrap_block_lines(
        &self,
        first_prefix: &str,
        _continuation_prefix: &str,
        content: Vec<Span<'static>>,
        max_width: usize,
        border_style: Style,
    ) -> Vec<Line<'static>> {
        self.wrap_block_lines_with_options(
            first_prefix,
            _continuation_prefix,
            content,
            max_width,
            border_style,
            true,
        )
    }

    /// Wrap content with left border only (no right border)
    #[allow(dead_code)]
    pub(super) fn wrap_block_lines_no_right_border(
        &self,
        first_prefix: &str,
        _continuation_prefix: &str,
        content: Vec<Span<'static>>,
        max_width: usize,
        border_style: Style,
    ) -> Vec<Line<'static>> {
        self.wrap_block_lines_with_options(
            first_prefix,
            _continuation_prefix,
            content,
            max_width,
            border_style,
            false,
        )
    }

    /// Wrap content with configurable border options
    fn wrap_block_lines_with_options(
        &self,
        first_prefix: &str,
        _continuation_prefix: &str,
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
        let prefix_width = first_prefix.chars().count();
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

        let mut wrapped = self.wrap_line(Line::from(content), content_width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        // Add borders to each wrapped line
        for line in wrapped.iter_mut() {
            let line_width = line.spans.iter().map(|s| s.width()).sum::<usize>();
            let padding = if show_right_border {
                content_width.saturating_sub(line_width)
            } else {
                0
            };

            let mut new_spans = vec![Span::styled(first_prefix.to_owned(), border_style)];
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

    /// Reflow tool output lines with appropriate formatting
    ///
    /// Tool blocks are visually grouped with:
    /// - Consistent indentation (2 spaces)
    /// - Dimmed styling for less visual weight
    /// - Optional spacing after tool block ends
    #[allow(dead_code)]
    pub(super) fn reflow_tool_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        let max_width = if width == 0 {
            usize::MAX
        } else {
            width as usize
        };

        let border_style =
            ratatui_style_from_inline(&self.styles.tool_border_style(), self.theme.foreground);

        let is_detail = line
            .segments
            .iter()
            .any(|segment| segment.style.effects.contains(Effects::ITALIC));

        // Check if this is the start of a tool block
        let prev_is_tool = if index > 0 {
            self.lines
                .get(index - 1)
                .map(|prev| prev.kind == InlineMessageKind::Tool)
                .unwrap_or(false)
        } else {
            false
        };
        let is_start = !prev_is_tool;

        let next_is_tool = self
            .lines
            .get(index + 1)
            .map(|next| next.kind == InlineMessageKind::Tool)
            .unwrap_or(false);
        let is_end = !next_is_tool;

        let mut lines = Vec::new();

        // Add visual separator at start of tool block
        if is_start {
            let spacing = self.appearance.message_block_spacing.min(2) as usize;
            let skip_spacing = index > 0
                && self.lines.get(index - 1).is_some_and(|prev| {
                    prev.kind == InlineMessageKind::Info && is_tool_summary_line(prev)
                });
            if index > 0 && !skip_spacing {
                for _ in 0..spacing {
                    lines.push(Line::default());
                }
            }
        }

        if is_detail {
            // Simple indent prefix without border characters
            let body_prefix = "  ";
            let content = render::render_tool_segments(self, line);
            lines.extend(self.wrap_block_lines(
                body_prefix,
                body_prefix,
                content,
                max_width,
                border_style,
            ));
        } else {
            // For tool call summaries, preserve inline colors and add padded borders.
            let content = render::render_tool_segments(self, line);
            let body_prefix = format!("  {} ", ui::INLINE_BLOCK_BODY_LEFT);
            lines.extend(self.wrap_block_lines(
                &body_prefix,
                &body_prefix,
                content,
                max_width,
                border_style,
            ));
        }

        // Add optional spacing after tool block for clean separation
        if is_end {
            let spacing = self.appearance.message_block_spacing.min(2) as usize;
            for _ in 0..spacing {
                lines.push(Line::default());
            }
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }

    /// Check if a PTY block has actual content
    #[allow(dead_code)]
    pub(super) fn pty_block_has_content(&self, index: usize) -> bool {
        if self.lines.is_empty() {
            return false;
        }

        let mut start = index;
        while start > 0 {
            let Some(previous) = self.lines.get(start - 1) else {
                break;
            };
            if previous.kind != InlineMessageKind::Pty {
                break;
            }
            start -= 1;
        }

        let mut end = index;
        while end + 1 < self.lines.len() {
            let Some(next) = self.lines.get(end + 1) else {
                break;
            };
            if next.kind != InlineMessageKind::Pty {
                break;
            }
            end += 1;
        }

        if start > end || end >= self.lines.len() {
            tracing::warn!(
                "invalid range: start={}, end={}, len={}",
                start,
                end,
                self.lines.len()
            );
            return false;
        }

        for line in &self.lines[start..=end] {
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

    /// Reflow PTY output lines with appropriate borders and formatting
    #[allow(dead_code)]
    pub(super) fn reflow_pty_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        let max_width = if width == 0 {
            usize::MAX
        } else {
            width as usize
        };

        if !self.pty_block_has_content(index) {
            return Vec::new();
        }

        let border_inline = super::super::types::InlineTextStyle {
            color: self.theme.secondary.or(self.theme.foreground),
            ..Default::default()
        };
        let border_style = ratatui_style_from_inline(&border_inline, self.theme.foreground);

        let prev_is_pty = index
            .checked_sub(1)
            .and_then(|prev| self.lines.get(prev))
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
        let fallback = self
            .text_fallback(InlineMessageKind::Pty)
            .or(self.theme.foreground);
        let mut body_spans = Vec::new();
        for segment in &line.segments {
            let stripped_text = render::strip_ansi_codes(&segment.text);
            let style = ratatui_style_from_inline(&segment.style, fallback);
            body_spans.push(Span::styled(stripped_text.into_owned(), style));
        }

        if is_start {
            // Simple indent prefix without border characters
            let first_prefix = "  ";
            let continuation_prefix = "  ";
            lines.extend(self.wrap_block_lines_no_right_border(
                first_prefix,
                continuation_prefix,
                body_spans,
                max_width,
                border_style,
            ));
        } else {
            // Simple indent prefix without border characters
            let body_prefix = "  ";
            lines.extend(self.wrap_block_lines_no_right_border(
                body_prefix,
                body_prefix,
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

    /// Create a message divider line
    #[allow(dead_code)]
    pub(super) fn message_divider_line(
        &self,
        width: usize,
        kind: InlineMessageKind,
    ) -> Line<'static> {
        if width == 0 {
            return Line::default();
        }

        let content = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(width);
        let style = self.message_divider_style(kind);
        Line::from(content).style(style)
    }

    /// Get the style for a message divider
    #[allow(dead_code)]
    pub(super) fn message_divider_style(&self, kind: InlineMessageKind) -> Style {
        self.styles.message_divider_style(kind)
    }

    /// Justify wrapped lines for agent messages
    ///
    /// This method also applies visual styling for:
    /// - Todo/checkbox items (completed items are dimmed)
    /// - List items with consistent formatting
    /// - Diff lines with background colors
    #[allow(dead_code)]
    pub(super) fn justify_wrapped_lines(
        &self,
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

            // Extract line text for analysis
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
            let line_text: &str = &*line_text_storage;
            let trimmed_start = line_text.trim_start();

            let mut next_in_fenced_block = in_fenced_block;
            let is_fence_line =
                trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~");
            if is_fence_line {
                next_in_fenced_block = !in_fenced_block;
            }

            // Check for todo/checkbox items
            let todo_state = text_utils::detect_todo_state(line_text);

            // Extend diff line backgrounds to full width
            let processed_line = if self.is_diff_line(&line) {
                self.pad_diff_line(&line, max_width)
            } else if todo_state == text_utils::TodoState::Completed
                && self.appearance.dim_completed_todos
            {
                // Dim completed todo items for visual hierarchy
                self.apply_completed_todo_style(&line)
            } else if kind == InlineMessageKind::Agent
                && !in_fenced_block
                && !is_fence_line
                && self.should_justify_message_line(&line, max_width, is_last)
            {
                self.justify_message_line(&line, max_width)
            } else {
                line
            };

            justified.push(processed_line);
            in_fenced_block = next_in_fenced_block;
        }

        justified
    }

    /// Apply dimmed styling to completed todo items
    fn apply_completed_todo_style(&self, line: &Line<'static>) -> Line<'static> {
        let dimmed_spans: Vec<Span<'static>> = line
            .spans
            .iter()
            .map(|span| {
                Span::styled(
                    span.content.clone(),
                    span.style
                        .add_modifier(Modifier::DIM)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            })
            .collect();
        Line::from(dimmed_spans)
    }

    /// Check if a message line should be justified
    #[allow(dead_code)]
    pub(super) fn should_justify_message_line(
        &self,
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
        let text: &str = &*line.spans[0].content;
        if text.trim().is_empty() {
            return false;
        }
        if text.starts_with(char::is_whitespace) {
            return false;
        }
        let trimmed = text.trim();
        if trimmed.starts_with(['-', '*', '`', '>', '#']) {
            return false;
        }
        if trimmed.contains("```") {
            return false;
        }
        let width = UnicodeWidthStr::width(trimmed);
        if width >= max_width || width < max_width / 2 {
            return false;
        }

        text_utils::justify_plain_text(text, max_width).is_some()
    }

    /// Justify a message line by distributing spaces
    #[allow(dead_code)]
    pub(super) fn justify_message_line(
        &self,
        line: &Line<'static>,
        max_width: usize,
    ) -> Line<'static> {
        let span = &line.spans[0];
        if let Some(justified) = text_utils::justify_plain_text(&*span.content, max_width) {
            Line::from(justified).style(span.style)
        } else {
            line.clone()
        }
    }

    /// Check if a line is a diff line (has diff markers and background color)
    #[allow(dead_code)]
    pub(super) fn is_diff_line(&self, line: &Line<'static>) -> bool {
        if line.spans.is_empty() {
            return false;
        }

        let has_bg_color = line.spans.iter().any(|span| span.style.bg.is_some());
        if !has_bg_color {
            return false;
        }

        let first_span_char = line.spans[0].content.chars().next();
        matches!(first_span_char, Some('+') | Some('-') | Some(' '))
    }

    /// Pad a diff line to full width
    #[allow(dead_code)]
    pub(super) fn pad_diff_line(&self, line: &Line<'static>, max_width: usize) -> Line<'static> {
        if max_width == 0 || line.spans.is_empty() {
            return line.clone();
        }

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

        let padding_style = Style::default();

        let new_spans: Vec<_> = line
            .spans
            .iter()
            .cloned()
            .chain(std::iter::once(Span::styled(
                " ".repeat(padding_needed),
                padding_style,
            )))
            .collect();

        Line::from(new_spans)
    }
}

#[derive(Clone, Copy)]
struct BlockChars {
    top_left: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
    horizontal: &'static str,
    vertical: &'static str,
}

fn block_chars(border_type: ratatui::widgets::BorderType) -> BlockChars {
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

fn is_tool_summary_line(message: &MessageLine) -> bool {
    let text: String = message
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    let trimmed = text.trim_start();
    trimmed.starts_with("• ") || trimmed.starts_with("└ ")
}

/// Collapse multiple consecutive newlines (3 or more) into at most 2 newlines
/// Returns Cow to avoid allocation when no changes are needed
#[allow(dead_code)]
fn collapse_excess_newlines(text: &str) -> std::borrow::Cow<'_, str> {
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
