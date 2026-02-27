use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::super::super::types::InlineMessageKind;
use super::super::{Session, text_utils};
use crate::config::constants::ui;

impl Session {
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
            let line_text: &str = &line_text_storage;
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
        let text: &str = &line.spans[0].content;
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
        if let Some(justified) = text_utils::justify_plain_text(&span.content, max_width) {
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
}
