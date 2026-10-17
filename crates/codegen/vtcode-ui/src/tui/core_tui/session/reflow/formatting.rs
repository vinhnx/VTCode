use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::super::super::types::InlineMessageKind;
use super::super::{Session, text_utils};
use super::helpers::is_diff_row_spans;
use crate::tui::config::constants::ui;

impl Session {
    /// Create a message divider line
    pub(super) fn message_divider_line(&self, width: usize, kind: InlineMessageKind) -> Line<'static> {
        if width == 0 {
            return Line::default();
        }

        let content = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(width);
        let style = self.message_divider_style(kind);
        Line::from(content).style(style)
    }

    /// Get the style for a message divider
    fn message_divider_style(&self, kind: InlineMessageKind) -> Style {
        self.styles.message_divider_style(kind)
    }

    /// Justify wrapped lines for agent messages (in-place).
    ///
    /// This method also applies visual styling for:
    /// - Todo/checkbox items (completed items are dimmed)
    /// - List items with consistent formatting
    /// - Diff rows (background-aware with a foreground-only fallback, never
    ///   justified as prose)
    ///
    /// Rows carrying projected transcript links (`row_has_links`) are never
    /// justified: justification inserts interior spaces, which would shift
    /// the text under the precomputed link ranges and misalign underlines.
    ///
    /// Hotpath: operates in-place to avoid one outer `Vec` alloc per Agent
    /// message (7401 in tui_hotpath). Lines that need no work are left
    /// untouched (no clone, no replace).
    pub(super) fn justify_wrapped_lines_in_place(
        &self,
        lines: &mut [Line<'static>],
        max_width: usize,
        kind: InlineMessageKind,
        row_has_links: &[bool],
    ) {
        if max_width == 0 || lines.is_empty() {
            return;
        }

        let total = lines.len();
        let mut in_fenced_block = false;
        for (index, line) in lines.iter_mut().enumerate() {
            let is_last = index + 1 == total;

            // Extract line text for analysis (borrowed for single-span,
            // owned only for multi-span — same as before).
            let (is_fence_line, next_in_fenced_block, todo_state, is_diff, should_justify) = {
                let line_ref: &Line<'static> = line;
                let line_text_storage: std::borrow::Cow<'_, str> = if line_ref.spans.len() == 1 {
                    std::borrow::Cow::Borrowed(&*line_ref.spans[0].content)
                } else {
                    std::borrow::Cow::Owned(line_ref.spans.iter().map(|span| &*span.content).collect::<String>())
                };
                let line_text: &str = &line_text_storage;
                let trimmed_start = line_text.trim_start();

                let mut next_in_fenced_block = in_fenced_block;
                let is_fence_line = trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~");
                if is_fence_line {
                    next_in_fenced_block = !in_fenced_block;
                }

                let todo_state = text_utils::detect_todo_state(line_text);
                let is_diff = self.is_diff_line(line_ref);
                let should_justify = kind == InlineMessageKind::Agent
                    && !in_fenced_block
                    && !is_fence_line
                    && !row_has_links.get(index).copied().unwrap_or(false)
                    && self.should_justify_message_line(line_ref, max_width, is_last);
                (is_fence_line, next_in_fenced_block, todo_state, is_diff, should_justify)
            };
            let _ = is_fence_line;

            let replacement = if is_diff {
                // `pad_diff_line` clones when no padding is needed (no bg or
                // already full width) — keep the original to avoid a
                // clone-replace cycle. Only replace when padding was added.
                let padded = self.pad_diff_line(line, max_width);
                if padded == *line { None } else { Some(padded) }
            } else if todo_state == text_utils::TodoState::Completed && self.appearance.dim_completed_todos {
                Some(self.apply_completed_todo_style(line))
            } else if should_justify {
                Some(self.justify_message_line(line, max_width))
            } else {
                None
            };

            if let Some(new_line) = replacement {
                *line = new_line;
            }
            in_fenced_block = next_in_fenced_block;
        }
    }

    /// Apply dimmed styling to completed todo items
    fn apply_completed_todo_style(&self, line: &Line<'static>) -> Line<'static> {
        let dimmed_spans: Vec<Span<'static>> = line
            .spans
            .iter()
            .map(|span| {
                Span::styled(
                    span.content.clone(),
                    span.style.add_modifier(Modifier::DIM).add_modifier(Modifier::CROSSED_OUT),
                )
            })
            .collect();
        Line::from(dimmed_spans)
    }

    /// Check if a message line should be justified
    fn should_justify_message_line(&self, line: &Line<'static>, max_width: usize, is_last: bool) -> bool {
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
    fn justify_message_line(&self, line: &Line<'static>, max_width: usize) -> Line<'static> {
        let span = &line.spans[0];
        if let Some(justified) = text_utils::justify_plain_text(&span.content, max_width) {
            Line::from(justified).style(span.style)
        } else {
            line.clone()
        }
    }

    /// Check if a line is a diff row (markers plus tint or diff foreground).
    ///
    /// Shared with `blocks.rs` via `helpers::is_diff_row_spans` so
    /// foreground-only fallback diff rows are never justified as prose.
    fn is_diff_line(&self, line: &Line<'static>) -> bool {
        is_diff_row_spans(&line.spans)
    }

    /// Pad a diff line to full width.
    ///
    /// Foreground-only fallback rows (no bg) need no padding by design.
    fn pad_diff_line(&self, line: &Line<'static>, max_width: usize) -> Line<'static> {
        if max_width == 0 || line.spans.is_empty() {
            return line.clone();
        }
        if line.spans.iter().all(|span| span.style.bg.is_none()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::core_tui::types::InlineTheme;

    fn plain_line(text: &str) -> Line<'static> {
        Line::from(Span::raw(text.to_owned()))
    }

    fn line_width(line: &Line<'static>) -> usize {
        use unicode_width::UnicodeWidthStr;
        line.spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum()
    }

    fn words_of(line: &Line<'static>) -> Vec<String> {
        line.spans
            .iter()
            .flat_map(|s| s.content.split_whitespace().map(str::to_owned))
            .collect()
    }

    #[test]
    fn in_place_leaves_last_line_untouched_but_justifies_eligible_non_last() {
        // Arrange: same eligible text in non-last vs last position (asymmetric:
        // only non-last may justify).
        let eligible = "alpha beta gamma delta";
        let session = Session::new(InlineTheme::default(), None, 20);
        let mut lines = vec![plain_line(eligible), plain_line("short")];

        // Act.
        session.justify_wrapped_lines_in_place(&mut lines, 30, InlineMessageKind::Agent, &[]);

        // Assert (oracle): first row padded to full width with words preserved;
        // last row never justified.
        assert_eq!(line_width(&lines[0]), 30);
        assert_eq!(words_of(&lines[0]), vec!["alpha", "beta", "gamma", "delta"]);
        assert_eq!(lines[1].spans[0].content.as_ref(), "short");
    }

    #[test]
    fn in_place_skips_fenced_content_but_justifies_matching_outside_prose() {
        // Arrange (asymmetric): identical eligible text inside vs outside a
        // fenced block — fence state must survive the in-place indexed loop.
        let eligible = "alpha beta gamma delta";
        let session = Session::new(InlineTheme::default(), None, 20);
        let mut lines = vec![
            plain_line("```"),
            plain_line(eligible),
            plain_line("```"),
            plain_line(eligible),
            plain_line("short last"),
        ];

        // Act.
        session.justify_wrapped_lines_in_place(&mut lines, 30, InlineMessageKind::Agent, &[]);

        // Assert: inside-fence row untouched, outside non-last row justified,
        // words preserved on both (independent re-derivation, no helper reuse).
        assert!(line_width(&lines[1]) < 30, "fenced content must not justify");
        assert_eq!(words_of(&lines[1]), vec!["alpha", "beta", "gamma", "delta"]);
        assert_eq!(line_width(&lines[3]), 30, "matching outside prose must justify");
        assert_eq!(words_of(&lines[3]), vec!["alpha", "beta", "gamma", "delta"]);
        assert_eq!(lines[4].spans[0].content.as_ref(), "short last");
    }

    #[test]
    fn in_place_never_justifies_rows_carrying_links() {
        // Arrange (asymmetric): same eligible text, first without link flag,
        // second with — link rows keep wrapped text for range alignment.
        let eligible = "alpha beta gamma delta";
        let session = Session::new(InlineTheme::default(), None, 20);
        let mut lines = vec![plain_line(eligible), plain_line(eligible), plain_line("short")];

        // Act.
        session.justify_wrapped_lines_in_place(&mut lines, 30, InlineMessageKind::Agent, &[false, true]);

        // Assert.
        assert_eq!(line_width(&lines[0]), 30);
        assert!(line_width(&lines[1]) < 30, "link row must keep wrapped text");
        assert_eq!(words_of(&lines[1]), vec!["alpha", "beta", "gamma", "delta"]);
    }
}
