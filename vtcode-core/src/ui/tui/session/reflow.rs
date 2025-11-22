use anstyle::Effects;
/// Transcript reflow and wrapping operations for Session
///
/// This module handles transcript line wrapping, reflowing, and formatting including:
/// - Message line reflowing based on width
/// - Text wrapping and justification
/// - Tool and PTY output formatting with borders
/// - Diff line padding
/// - Block line wrapping with borders
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::super::style::ratatui_style_from_inline;
use super::super::types::InlineMessageKind;
use super::{Session, render, text_utils};
use crate::config::constants::ui;

impl Session {
    /// Reflow message lines for a given width (test-only method)
    #[cfg(test)]
    pub(super) fn reflow_transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            let mut lines: Vec<Line<'static>> = Vec::new();
            for index in 0..self.lines.len() {
                lines.extend(self.reflow_message_lines(index, 0));
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            return lines;
        }

        let mut wrapped_lines = Vec::new();
        for index in 0..self.lines.len() {
            wrapped_lines.extend(self.reflow_message_lines(index, width));
        }

        if wrapped_lines.is_empty() {
            wrapped_lines.push(Line::default());
        }

        wrapped_lines
    }

    /// Reflow a specific message line based on its type and width
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

        let spans = render::render_message_spans(self, index);
        let base_line = Line::from(spans);
        if width == 0 {
            return vec![base_line];
        }

        let mut wrapped = Vec::new();
        let max_width = width as usize;

        if message.kind == InlineMessageKind::User && max_width > 0 {
            wrapped.push(self.message_divider_line(max_width, message.kind));
        }

        let mut lines = self.wrap_line(base_line, max_width);
        if !lines.is_empty() {
            lines = self.justify_wrapped_lines(lines, max_width, message.kind);
        }
        if lines.is_empty() {
            lines.push(Line::default());
        }
        wrapped.extend(lines);

        if message.kind == InlineMessageKind::User && max_width > 0 {
            wrapped.push(self.message_divider_line(max_width, message.kind));
        }

        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        // Add a line break after Error, Info, and Policy messages
        match message.kind {
            InlineMessageKind::Error | InlineMessageKind::Info | InlineMessageKind::Policy => {
                wrapped.push(Line::default());
            }
            _ => {}
        }

        wrapped
    }

    /// Wrap a line of text to fit within a maximum width
    pub(super) fn wrap_line(&self, line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
        text_utils::wrap_line(line, max_width)
    }

    /// Wrap content with left and right borders
    pub(super) fn wrap_block_lines(
        &self,
        first_prefix: &str,
        _continuation_prefix: &str,
        content: Vec<Span<'static>>,
        max_width: usize,
        border_style: Style,
    ) -> Vec<Line<'static>> {
        if max_width < 2 {
            return vec![Line::from(vec![Span::styled(
                format!("{}││", first_prefix),
                border_style,
            )])];
        }

        let right_border = ui::INLINE_BLOCK_BODY_RIGHT;
        let prefix_width = first_prefix.chars().count();
        let border_width = right_border.chars().count();
        let consumed_width = prefix_width.saturating_add(border_width);
        let content_width = max_width.saturating_sub(consumed_width);

        if max_width == usize::MAX {
            let mut spans = vec![Span::styled(first_prefix.to_string(), border_style)];
            spans.extend(content);
            spans.push(Span::styled(right_border.to_string(), border_style));
            return vec![Line::from(spans)];
        }

        let mut wrapped = self.wrap_line(Line::from(content), content_width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        // Add borders to each wrapped line
        for line in wrapped.iter_mut() {
            let line_width = line.spans.iter().map(|s| s.width()).sum::<usize>();
            let padding = content_width.saturating_sub(line_width);

            let mut new_spans = vec![Span::styled(first_prefix.to_string(), border_style)];
            new_spans.extend(line.spans.drain(..));
            if padding > 0 {
                new_spans.push(Span::styled(" ".repeat(padding), Style::default()));
            }
            new_spans.push(Span::styled(right_border.to_string(), border_style));
            line.spans = new_spans;
        }

        wrapped
    }

    /// Reflow tool output lines with appropriate formatting
    pub(super) fn reflow_tool_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        let max_width = if width == 0 {
            usize::MAX
        } else {
            width as usize
        };

        let mut border_style =
            ratatui_style_from_inline(&self.styles.tool_border_style(), self.theme.foreground);
        border_style = border_style.add_modifier(Modifier::DIM);

        let is_detail = line
            .segments
            .iter()
            .any(|segment| segment.style.effects.contains(Effects::ITALIC));
        let next_is_tool = self
            .lines
            .get(index + 1)
            .map(|next| next.kind == InlineMessageKind::Tool)
            .unwrap_or(false);

        let is_end = !next_is_tool;

        let mut lines = Vec::new();
        if is_detail {
            let body_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            let content = render::render_tool_segments(self, line);
            lines.extend(self.wrap_block_lines(
                &body_prefix,
                &body_prefix,
                content,
                max_width,
                border_style.clone(),
            ));
        } else {
            // For simple tool output, render without borders
            let mut combined_text = String::new();
            for segment in &line.segments {
                let stripped_text = render::strip_ansi_codes(&segment.text);
                combined_text.push_str(&stripped_text);
            }

            // Collapse multiple consecutive newlines
            let processed_text = collapse_excess_newlines(&combined_text);

            let base_line = Line::from(vec![Span::raw(processed_text)]);
            if max_width > 0 {
                lines.extend(self.wrap_line(base_line, max_width));
            } else {
                lines.push(base_line);
            }
        }

        if is_end {
            // Don't add bottom border for simple tool output
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }

    /// Check if a PTY block has actual content
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

        let mut border_inline = super::super::types::InlineTextStyle::default();
        border_inline.color = self.theme.secondary.or(self.theme.foreground);
        let mut border_style = ratatui_style_from_inline(&border_inline, self.theme.foreground);
        border_style = border_style.add_modifier(Modifier::DIM);

        let header_inline = super::super::types::InlineTextStyle::default()
            .with_color(self.theme.primary.or(self.theme.foreground))
            .bold();
        let header_style = ratatui_style_from_inline(&header_inline, self.theme.foreground);

        let mut body_inline = super::super::types::InlineTextStyle::default();
        body_inline.color = self.theme.foreground;
        let mut body_style = ratatui_style_from_inline(&body_inline, self.theme.foreground);
        body_style = body_style.add_modifier(Modifier::BOLD);

        let prev_is_pty = index
            .checked_sub(1)
            .and_then(|prev| self.lines.get(prev))
            .map(|prev| prev.kind == InlineMessageKind::Pty)
            .unwrap_or(false);
        let next_is_pty = self
            .lines
            .get(index + 1)
            .map(|next| next.kind == InlineMessageKind::Pty)
            .unwrap_or(false);

        let is_start = !prev_is_pty;
        let is_end = !next_is_pty;

        let mut lines = Vec::new();

        let mut combined = String::new();
        for segment in &line.segments {
            combined.push_str(segment.text.as_str());
        }
        if is_start && is_end && combined.trim().is_empty() {
            return Vec::new();
        }
        let header_text = combined
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| ui::INLINE_PTY_PLACEHOLDER.to_string());

        if is_start {
            // Add top border
            if max_width > 2 {
                let top_border_content = format!(
                    "{}{}{}",
                    ui::INLINE_BLOCK_TOP_LEFT,
                    ui::INLINE_BLOCK_HORIZONTAL.repeat(max_width.saturating_sub(2)),
                    ui::INLINE_BLOCK_TOP_RIGHT
                );
                lines.push(Line::from(vec![Span::styled(
                    top_border_content,
                    border_style.clone(),
                )]));
            }

            // Add header with status
            let mut header_spans = Vec::new();
            header_spans.push(Span::styled(
                format!("[{}]", ui::INLINE_PTY_HEADER_LABEL),
                header_style.clone(),
            ));
            header_spans.push(Span::raw(" "));
            let running_style = super::super::types::InlineTextStyle::default()
                .with_color(self.theme.secondary.or(self.theme.foreground))
                .italic();
            header_spans.push(Span::styled(
                ui::INLINE_PTY_RUNNING_LABEL.to_string(),
                ratatui_style_from_inline(&running_style, self.theme.foreground),
            ));
            if !header_text.is_empty() {
                header_spans.push(Span::raw(" "));
                header_spans.push(Span::styled(header_text.clone(), body_style.clone()));
            }
            let status_label = if is_end {
                ui::INLINE_PTY_STATUS_DONE
            } else {
                ui::INLINE_PTY_STATUS_LIVE
            };
            header_spans.push(Span::raw(" "));
            header_spans.push(Span::styled(
                format!("[{}]", status_label),
                self.styles
                    .accent_style()
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD),
            ));

            let first_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            let continuation_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            lines.extend(self.wrap_block_lines(
                &first_prefix,
                &continuation_prefix,
                header_spans,
                max_width,
                border_style.clone(),
            ));
        } else {
            // Body content
            let fallback = self
                .text_fallback(InlineMessageKind::Pty)
                .or(self.theme.foreground);
            let mut body_spans = Vec::new();
            for segment in &line.segments {
                let style = ratatui_style_from_inline(&segment.style, fallback);
                body_spans.push(Span::styled(segment.text.clone(), style));
            }
            let body_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            lines.extend(self.wrap_block_lines(
                &body_prefix,
                &body_prefix,
                body_spans,
                max_width,
                border_style.clone(),
            ));
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }

    /// Create a message divider line
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
        Line::from(vec![Span::styled(content, style)])
    }

    /// Get the style for a message divider
    pub(super) fn message_divider_style(&self, kind: InlineMessageKind) -> Style {
        self.styles.message_divider_style(kind)
    }

    /// Justify wrapped lines for agent messages
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
            let mut next_in_fenced_block = in_fenced_block;
            let is_fence_line = {
                let line_text_storage: std::borrow::Cow<'_, str> = if line.spans.len() == 1 {
                    std::borrow::Cow::Borrowed(line.spans[0].content.as_ref())
                } else {
                    std::borrow::Cow::Owned(
                        line.spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>(),
                    )
                };
                let line_text = line_text_storage.as_ref();
                let trimmed_start = line_text.trim_start();
                trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~")
            };
            if is_fence_line {
                next_in_fenced_block = !in_fenced_block;
            }

            // Extend diff line backgrounds to full width
            let processed_line = if self.is_diff_line(&line) {
                self.pad_diff_line(&line, max_width)
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

    /// Check if a message line should be justified
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
        let text = line.spans[0].content.as_ref();
        if text.trim().is_empty() {
            return false;
        }
        if text.starts_with(char::is_whitespace) {
            return false;
        }
        let trimmed = text.trim();
        if trimmed.starts_with(|ch: char| matches!(ch, '-' | '*' | '`' | '>' | '#')) {
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
    pub(super) fn justify_message_line(
        &self,
        line: &Line<'static>,
        max_width: usize,
    ) -> Line<'static> {
        let span = &line.spans[0];
        if let Some(justified) = text_utils::justify_plain_text(span.content.as_ref(), max_width) {
            Line::from(vec![Span::styled(justified, span.style)])
        } else {
            line.clone()
        }
    }

    /// Check if a line is a diff line (has diff markers and background color)
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

/// Collapse multiple consecutive newlines (3 or more) into at most 2 newlines
fn collapse_excess_newlines(text: &str) -> String {
    let mut result = String::new();
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

    result
}
