use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;
use vtcode_commons::diff_paths::{is_diff_addition_line, is_diff_deletion_line};

use super::super::super::style::ratatui_style_from_inline;
use super::super::super::types::InlineMessageKind;
use super::super::{Session, render, text_utils};
use super::helpers::{is_tool_summary_line, split_tool_spans};
use crate::config::constants::ui;

impl Session {
    fn wrapped_diff_continuation_prefix(line_text: &str) -> Option<String> {
        let trimmed = line_text.trim_start();
        if is_diff_deletion_line(trimmed) || is_diff_addition_line(trimmed) {
            let marker_pos = line_text.find(['-', '+'])?;
            let marker_end = marker_pos + 1;
            let after = line_text.get(marker_end..)?;
            let extra_space = after.chars().take_while(|c| *c == ' ').count();
            let end = marker_end + extra_space;
            return line_text.get(..end).map(ToOwned::to_owned);
        }

        // Numbered diff line: "<line_no><spaces><+|-><spaces><code>"
        let mut idx = 0usize;
        for ch in line_text.chars() {
            if ch == ' ' {
                idx += ch.len_utf8();
            } else {
                break;
            }
        }

        let rest = line_text.get(idx..)?;
        let digits_len = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits_len == 0 {
            return None;
        }
        let mut offset = idx
            + rest
                .chars()
                .take(digits_len)
                .map(char::len_utf8)
                .sum::<usize>();
        let after_digits = line_text.get(offset..)?;
        let space_after_digits = after_digits.chars().take_while(|c| *c == ' ').count();
        if space_after_digits == 0 {
            return None;
        }
        offset += after_digits
            .chars()
            .take(space_after_digits)
            .map(char::len_utf8)
            .sum::<usize>();

        let marker = line_text.get(offset..)?.chars().next()?;
        if !matches!(marker, '+' | '-') {
            return None;
        }
        offset += marker.len_utf8();

        let after_marker = line_text.get(offset..)?;
        let space_after_marker = after_marker.chars().take_while(|c| *c == ' ').count();
        if space_after_marker == 0 {
            return None;
        }
        offset += after_marker
            .chars()
            .take(space_after_marker)
            .map(char::len_utf8)
            .sum::<usize>();

        let prefix_width = UnicodeWidthStr::width(line_text.get(..offset)?);
        Some(" ".repeat(prefix_width))
    }

    /// Wrap content with left and right borders
    #[allow(dead_code)]
    pub(super) fn wrap_block_lines(
        &self,
        first_prefix: &str,
        continuation_prefix: &str,
        content: Vec<Span<'static>>,
        max_width: usize,
        border_style: Style,
    ) -> Vec<Line<'static>> {
        self.wrap_block_lines_with_options(
            first_prefix,
            continuation_prefix,
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
        continuation_prefix: &str,
        content: Vec<Span<'static>>,
        max_width: usize,
        border_style: Style,
    ) -> Vec<Line<'static>> {
        self.wrap_block_lines_with_options(
            first_prefix,
            continuation_prefix,
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

        let diff_continuation_prefix = content.first().and_then(|span| {
            let text: &str = span.content.as_ref();
            Self::wrapped_diff_continuation_prefix(text)
        });

        let mut wrapped = self.wrap_line(Line::from(content), content_width);
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

            // For diff lines, preserve hanging indent/prefix on continuation lines.
            if idx > 0
                && let Some(ref prefix) = diff_continuation_prefix
            {
                // Add the diff prefix with dimmed style to match diff appearance
                let prefix_style = border_style.add_modifier(Modifier::DIM);
                new_spans.push(Span::styled(prefix.clone(), prefix_style));
            }

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

        let border_style = self.styles.border_style();

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

        let content = render::render_tool_segments(self, line);
        let split_lines = split_tool_spans(content);
        let summary_prefix = "    ".to_string();
        let detail_prefix = summary_prefix.clone();
        let detail_border_style = border_style.add_modifier(Modifier::DIM);

        for line_spans in split_lines {
            let mut line_text = String::new();
            for span in &line_spans {
                line_text.push_str(<std::borrow::Cow<'_, str> as AsRef<str>>::as_ref(
                    &span.content,
                ));
            }
            let trimmed = line_text.trim_start();
            let is_summary = trimmed.starts_with("• ") || trimmed.starts_with("└ ");

            if is_summary {
                // For tool call summaries, preserve inline colors and add padded borders.
                lines.extend(self.wrap_block_lines(
                    &summary_prefix,
                    &summary_prefix,
                    line_spans,
                    max_width,
                    border_style,
                ));
            } else {
                // Dim tool output and avoid right-side padding borders.
                let mut detail_spans = line_spans;
                for span in &mut detail_spans {
                    span.style = span.style.add_modifier(Modifier::DIM);
                }
                lines.extend(self.wrap_block_lines_no_right_border(
                    &detail_prefix,
                    &detail_prefix,
                    detail_spans,
                    max_width,
                    detail_border_style,
                ));
            }
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
    pub(crate) fn reflow_pty_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
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

        let border_style = self.styles.border_style();

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

        // Render body content - strip ANSI codes to ensure plain text output.
        // Use the theme's pty_body color as fallback instead of terminal DIM
        // for consistent, readable contrast across terminals.
        let pty_fallback = self.theme.pty_body.or(self.theme.foreground);
        let mut body_spans = Vec::new();
        for segment in &line.segments {
            let stripped_text = render::strip_ansi_codes(&segment.text);
            let style =
                ratatui_style_from_inline(&segment.style, pty_fallback).add_modifier(Modifier::DIM);
            body_spans.push(Span::styled(stripped_text.into_owned(), style));
        }

        let body_prefix = "  ";
        let continuation_prefix =
            text_utils::pty_wrapped_continuation_prefix(body_prefix, combined.as_str());
        lines.extend(self.wrap_block_lines_no_right_border(
            body_prefix,
            continuation_prefix.as_str(),
            body_spans,
            max_width,
            border_style,
        ));

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }
}
