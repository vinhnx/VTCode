use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;
use vtcode_commons::diff_paths::{is_diff_addition_line, is_diff_deletion_line};

use super::super::super::style::{
    ratatui_color_from_ansi, ratatui_pty_style_from_inline, ratatui_style_from_ansi, ratatui_style_from_inline,
};
use super::super::super::types::{InlineLinkRange, InlineMessageKind, InlineTextStyle};
use super::super::message::RenderedTranscriptLink;
use super::super::styling::tool_inline_style_for;
use super::super::{Session, TranscriptLine, render, text_utils};
use super::helpers::{
    diff_row_bg, has_summary_prefix, is_bullet_summary_text, is_diff_row_spans, is_tool_summary_line,
    is_tree_detail_text, parse_tool_call_prefix, push_spacing_blanks, push_spacing_transcript_lines, split_tool_spans,
};
use crate::tui::config::constants::ui;

/// Whether spans form a diff row (header or body), including the
/// foreground-only fallback.
/// Shared via `helpers::is_diff_row_spans` so reflow and formatting agree.
fn is_diff_row(spans: &[Span<'_>]) -> bool {
    is_diff_row_spans(spans)
}
/// Pad a tinted diff row with bg-colored spaces to the full viewport width.
///
/// Non-diff rows are left at exact content width (no right border/padding).
/// Foreground-only fallback rows (no bg) need no padding by design.
fn pad_diff_row_to_width(line: &mut Line<'static>, max_width: usize) {
    let Some(bg) = diff_row_bg(&line.spans) else {
        return;
    };
    let width: usize = line.spans.iter().map(Span::width).sum();
    let padding = max_width.saturating_sub(width);
    if padding == 0 {
        return;
    }
    line.spans.push(Span::styled(" ".repeat(padding), Style::default().bg(bg)));
}

impl Session {
    fn opaque_tool_header_text_style(&self, style: Style) -> Style {
        let mut style = style.remove_modifier(Modifier::DIM);
        let is_subdued_foreground = [self.theme.pty_body, self.theme.tool_body]
            .into_iter()
            .flatten()
            .map(ratatui_color_from_ansi)
            .any(|color| style.fg == Some(color));
        if is_subdued_foreground && let Some(foreground) = self.theme.foreground.map(ratatui_color_from_ansi) {
            style = style.fg(foreground);
        }
        style
    }

    fn tool_header_action_style(&self, action: &str) -> Style {
        if action == "Ran" {
            let style = InlineTextStyle {
                color: self.theme.primary.or(self.theme.foreground),
                ..InlineTextStyle::default()
            }
            .bold();
            return ratatui_style_from_inline(&style, self.theme.foreground).remove_modifier(Modifier::DIM);
        }

        let tool_style = tool_inline_style_for(action, &self.theme);
        let fallback = self.theme.tool_accent.or(self.theme.primary).or(self.theme.foreground);
        ratatui_style_from_ansi(tool_style.to_ansi_style(fallback)).remove_modifier(Modifier::DIM)
    }

    fn tool_header_body_style(&self) -> Style {
        let style = InlineTextStyle {
            // `tool_body` is an accent color in several themes. It must not be
            // used as the fallback here because a whole status-colored header
            // would still appear colored after the status span is split off.
            color: self.theme.foreground,
            ..InlineTextStyle::default()
        };
        ratatui_style_from_inline(&style, self.theme.foreground).remove_modifier(Modifier::DIM)
    }

    /// Style for the session exit-status row (`✓ exit 0`, `✓ exit 127`, `✓ done`).
    ///
    /// The badge keeps its glyph and text but renders at full intensity in the
    /// success/error accent, so a completed command's status row stays legible
    /// instead of blending into the surrounding subdued tool output.
    fn exit_status_row_style(&self, text: &str) -> Option<Style> {
        let trimmed = text.trim();
        let (rest, explicit_failure) = match trimmed.strip_prefix("✓ ") {
            Some(rest) => (rest, false),
            None => (trimmed.strip_prefix("✗ ")?, true),
        };
        let success = !explicit_failure
            && match rest.strip_prefix("exit ") {
                Some(code) => code.trim().parse::<i32>().is_ok_and(|code| code == 0),
                None => rest == "done",
            };
        let color = if success {
            self.theme.primary.or(self.theme.foreground)
        } else {
            self.theme.error.or(self.theme.foreground)
        }?;
        Some(Style::default().fg(ratatui_color_from_ansi(color)))
    }

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
        let mut offset = idx + rest.chars().take(digits_len).map(char::len_utf8).sum::<usize>();
        let after_digits = line_text.get(offset..)?;
        let space_after_digits = after_digits.chars().take_while(|c| *c == ' ').count();
        if space_after_digits == 0 {
            return None;
        }
        offset += after_digits.chars().take(space_after_digits).map(char::len_utf8).sum::<usize>();

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
        offset += after_marker.chars().take(space_after_marker).map(char::len_utf8).sum::<usize>();

        let prefix_width = UnicodeWidthStr::width(line_text.get(..offset)?);
        Some(" ".repeat(prefix_width))
    }

    /// Wrap content without block borders (no leading/trailing `│`).
    ///
    /// Tool blocks render borderless so task trees and command summaries never
    /// show trailing/leading pipe framing; tree `│` inside task content is
    /// semantic hierarchy and is preserved.
    fn wrap_block_lines_no_right_border(
        &self,
        first_prefix: &str,
        continuation_prefix: &str,
        content: Vec<Span<'static>>,
        max_width: usize,
        border_style: Style,
    ) -> Vec<Line<'static>> {
        if max_width < 2 {
            return vec![Line::from(first_prefix.to_string()).style(border_style)];
        }

        let first_prefix_width = UnicodeWidthStr::width(first_prefix);
        let continuation_prefix_width = UnicodeWidthStr::width(continuation_prefix);
        let prefix_width = first_prefix_width.max(continuation_prefix_width);
        let content_width = max_width.saturating_sub(prefix_width);

        if max_width == usize::MAX {
            let mut spans = vec![Span::styled(first_prefix.to_owned(), border_style)];
            spans.extend(content);
            return vec![Line::from(spans)];
        }

        let diff_continuation_prefix = content
            .first()
            .and_then(|span| {
                let text: &str = span.content.as_ref();
                Self::wrapped_diff_continuation_prefix(text)
            })
            .or_else(|| {
                // Compact tinted rows omit the visible +/- gutter; hang under
                // the marker cell so wrap does not start at the row edge.
                if diff_row_bg(&content).is_some() {
                    Some(" ".to_owned())
                } else {
                    None
                }
            });
        // Wrapping removes the original marker from continuation fragments,
        // so keep the source row's tint before turning the spans into lines.
        let content_diff_bg = diff_row_bg(&content);

        let line_text = content.iter().map(|span| span.content.as_ref()).collect::<String>();
        let tree_continuation_prefix = text_utils::compact_tree_continuation_prefix(&line_text);
        let content_line = Line::from(content);
        // URL-aware wrapping historically reused the first span's style for
        // every fragment. That makes a path-bearing command header inherit
        // the status-colored bullet. Preserve header spans through the normal
        // style-aware wrapper; URL-aware wrapping remains useful for output.
        let mut wrapped = if let Some(prefix) = tree_continuation_prefix {
            text_utils::wrap_line_with_hanging_prefix(content_line, content_width, &prefix)
        } else if line_is_tool_command_header(&content_line) {
            text_utils::wrap_line(content_line, content_width)
        } else {
            self.wrap_line(content_line, content_width)
        };
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        // Add the gutter prefix to each wrapped line
        for (idx, line) in wrapped.iter_mut().enumerate() {
            // Diff rows keep their tinted band edge-to-edge: paint the gutter
            // prefixes with the row bg so wrapped rows don't start with an
            // unpainted strip.
            let row_diff_bg = content_diff_bg.or_else(|| diff_row_bg(&line.spans));
            let mut gutter_style = border_style;
            if let Some(bg) = row_diff_bg {
                gutter_style = gutter_style.bg(bg);
            }
            let active_prefix = if idx == 0 { first_prefix } else { continuation_prefix };
            let mut new_spans = vec![Span::styled(active_prefix.to_owned(), gutter_style)];

            // For diff lines, preserve hanging indent/prefix on continuation lines.
            if idx > 0
                && let Some(ref prefix) = diff_continuation_prefix
            {
                // Keep the continuation marker at the same contrast as the
                // row gutter; terminal-dependent DIM can make it disappear
                // against the stronger intraline background.
                new_spans.push(Span::styled(prefix.clone(), gutter_style));
            }

            new_spans.append(&mut line.spans);
            line.spans = new_spans;
        }

        // Diff rows extend their tinted band to the full viewport width so
        // short rows don't end mid-line. Non-diff rows keep exact content
        // width (no right border/padding by design).
        if max_width != usize::MAX {
            for line in wrapped.iter_mut() {
                pad_diff_row_to_width(line, max_width);
            }
        }

        wrapped
    }

    /// Reflow tool output lines with appropriate formatting
    ///
    /// Tool blocks are visually grouped with:
    /// - Consistent indentation (2 spaces)
    /// - Dimmed styling for less visual weight
    /// - Optional spacing after tool block ends
    pub(super) fn reflow_tool_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        let max_width = if width == 0 { usize::MAX } else { width as usize };

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

        let next_is_tool = self.lines.get(index + 1).is_some_and(|next| {
            if next.kind == InlineMessageKind::Tool {
                return true;
            }
            // A Tool header followed by its Info tree details (e.g. "• Search code"
            // -> "  └ File types:") should be treated as a single visual block
            // without an extra blank line between header and details.
            if next.kind == InlineMessageKind::Info {
                let text: String = next.segments.iter().map(|s| s.text.as_str()).collect();
                if is_tree_detail_text(&text) {
                    return true;
                }
            }
            false
        });
        let is_end = !next_is_tool;

        let mut lines = Vec::new();

        // Visual separator at the start of a tool block. Single ownership: the
        // previous block suppresses its trailing gap before tool blocks (see
        // `next_is_tool_block`), so this top gap is the only one.
        if is_start && self.should_add_tool_block_top_spacing(index) {
            push_spacing_blanks(&mut lines, self.tool_block_spacing());
        }

        let content = render::render_tool_segments(self, line);
        let split_lines = split_tool_spans(content);
        let summary_prefix = ui::INLINE_TOOL_HEADER_GUTTER;
        let summary_continuation = ui::INLINE_TOOL_HEADER_CONTINUATION;
        let detail_prefix = ui::INLINE_TOOL_DETAIL_GUTTER;
        let detail_continuation = ui::INLINE_TOOL_DETAIL_CONTINUATION;
        let detail_border_style = border_style.add_modifier(Modifier::DIM);

        for line_spans in split_lines {
            let line_text: String = line_spans.iter().map(|span| span.content.as_ref()).collect();
            let is_summary = has_summary_prefix(&line_text);
            if is_summary {
                // Style the tool call prefix ("• <Action>") with tool-specific ANSI color + bold.
                let mut styled_spans = Vec::with_capacity(line_spans.len() + 1);
                for (i, span) in line_spans.into_iter().enumerate() {
                    if i == 0 {
                        let text = span.content.clone().into_owned();
                        let style = span.style;
                        if let Some((action, prefix)) = parse_tool_call_prefix(&text) {
                            let mut bullet_style = style.remove_modifier(Modifier::DIM);
                            if bullet_style.fg.is_none() {
                                if let Some(c) = self.theme.foreground.map(ratatui_color_from_ansi) {
                                    bullet_style = bullet_style.fg(c);
                                }
                            }
                            if bullet_style.bg.is_none() {
                                if let Some(c) = self.theme.background.map(ratatui_color_from_ansi) {
                                    bullet_style = bullet_style.bg(c);
                                }
                            }
                            let action_start = prefix.len() - action.len();
                            styled_spans.push(Span::styled(prefix[..action_start].to_owned(), bullet_style));
                            styled_spans.push(Span::styled(action.to_owned(), self.tool_header_action_style(action)));
                            let rest = &text[prefix.len()..];
                            if !rest.is_empty() {
                                styled_spans.push(Span::styled(rest.to_owned(), self.tool_header_body_style()));
                            }
                        } else {
                            styled_spans.push(Span::styled(text, style.remove_modifier(Modifier::DIM)));
                        }
                    } else {
                        styled_spans.push(Span::styled(
                            span.content.into_owned(),
                            self.opaque_tool_header_text_style(span.style),
                        ));
                    }
                }
                lines.extend(self.wrap_block_lines_no_right_border(
                    summary_prefix,
                    summary_continuation,
                    styled_spans,
                    max_width,
                    border_style,
                ));
            } else {
                // Detail rows are nested under their header with an extra indent,
                // and wrapped lines keep the tree marker aligned via hanging
                // indent. No right-side padding borders.
                let mut detail_spans = line_spans;
                // Task-tree rows stay subdued so hierarchy reads as secondary to
                // the tool header and its command output; ordinary exec output
                // renders at normal intensity. Diff rows keep their explicit
                // base/chip styling untouched.
                let is_tree_row = {
                    let trimmed = line_text.trim_start();
                    trimmed.starts_with("└ ") || trimmed.starts_with("├ ") || trimmed.starts_with("│ ")
                };
                if is_tree_row && !is_diff_row(&detail_spans) {
                    for span in &mut detail_spans {
                        span.style = span.style.add_modifier(Modifier::DIM);
                    }
                }
                for span in &mut detail_spans {
                    if let Some(style) = self.exit_status_row_style(span.content.as_ref()) {
                        span.style = style;
                    }
                }
                lines.extend(self.wrap_block_lines_no_right_border(
                    detail_prefix,
                    detail_continuation,
                    detail_spans,
                    max_width,
                    detail_border_style,
                ));
            }
        }

        // Spacing after a tool block for clean separation (single ownership:
        // the next tool-block start suppresses its own top gap inside chains).
        if is_end {
            push_spacing_blanks(&mut lines, self.tool_block_spacing());
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }

    /// Contiguous PTY-block range containing `index`, or `None` when `index`
    /// is not a PTY line.
    fn pty_block_range(&self, index: usize) -> Option<(usize, usize)> {
        if self.lines.get(index)?.kind != InlineMessageKind::Pty {
            return None;
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

        Some((start, end))
    }

    /// Check whether any line in `[start, end]` carries non-blank text.
    fn pty_block_has_content_in(&self, start: usize, end: usize) -> bool {
        let Some(range) = self.lines.get(start..=end) else {
            tracing::warn!("invalid range: start={start}, end={end}, len={}", self.lines.len());
            return false;
        };
        range
            .iter()
            .any(|line| line.segments.iter().any(|segment| !segment.text.trim().is_empty()))
    }

    /// Check if a PTY block has actual content.
    ///
    /// The answer is identical for every line of the same block. A non-blank
    /// line answers for its whole block without scanning, which keeps per-line
    /// reflow of large captures near-linear instead of quadratic.
    fn pty_block_has_content(&self, index: usize) -> bool {
        if self
            .lines
            .get(index)
            .is_some_and(|line| line.segments.iter().any(|segment| !segment.text.trim().is_empty()))
        {
            return true;
        }

        let Some((start, end)) = self.pty_block_range(index) else {
            return false;
        };
        self.pty_block_has_content_in(start, end)
    }

    /// Whether the PTY line at `index` is a `│` continuation of a command
    /// header (`• Ran ...` wrapped across `│` lines).
    ///
    /// Continuations render with the uniform header body style so a wrapped
    /// multiline command matches single-line Tool headers instead of
    /// inheriting shell token colors. The backward chain stops at output rows
    /// so genuine `│`-prefixed program output keeps its PTY styling.
    fn pty_is_header_continuation(&self, index: usize) -> bool {
        let line = match self.lines.get(index) {
            Some(line) if line.kind == InlineMessageKind::Pty => line,
            _ => return false,
        };
        let mut text = String::new();
        for segment in &line.segments {
            text.push_str(&render::strip_ansi_codes(&segment.text));
        }
        // Header wrapping uses the exact `  │ ` prefix. A later program
        // output row may itself begin with `│` after the four-space output
        // gutter; accepting any trimmed pipe would misclassify that output
        // when it follows a wrapped header.
        if !text.starts_with("  │ ") {
            return false;
        }
        let mut cursor = index;
        loop {
            cursor = match cursor.checked_sub(1) {
                Some(previous) => previous,
                None => return false,
            };
            let previous = match self.lines.get(cursor) {
                Some(previous) if previous.kind == InlineMessageKind::Pty => previous,
                _ => return false,
            };
            let mut previous_text = String::new();
            for segment in &previous.segments {
                previous_text.push_str(&render::strip_ansi_codes(&segment.text));
            }
            if parse_tool_call_prefix(&previous_text).is_some() {
                return true;
            }
            if !previous_text.starts_with("  │ ") {
                return false;
            }
        }
    }

    /// Reflow PTY output lines with appropriate borders and formatting
    pub(crate) fn reflow_pty_lines(&self, index: usize, width: u16) -> Vec<TranscriptLine> {
        let Some(line) = self.lines.get(index) else {
            return vec![TranscriptLine::default()];
        };

        let max_width = if width == 0 { usize::MAX } else { width as usize };

        let prev_is_pty = index
            .checked_sub(1)
            .and_then(|prev| self.lines.get(prev))
            .map(|prev| prev.kind == InlineMessageKind::Pty)
            .unwrap_or(false);
        let is_start = !prev_is_pty;

        // A non-blank block start proves the block has content, so the full
        // block scan below can be skipped on the hottest path.
        let line_is_blank = line.segments.iter().all(|segment| segment.text.trim().is_empty());
        if (line_is_blank || !is_start) && !self.pty_block_has_content(index) {
            return Vec::new();
        }

        let border_style = self.styles.border_style();

        let is_end = !self
            .lines
            .get(index + 1)
            .is_some_and(|next| next.kind == InlineMessageKind::Pty);

        let mut lines = Vec::with_capacity(line.segments.len());

        let mut combined = String::with_capacity(line.segments.iter().map(|s| s.text.len()).sum());
        for segment in &line.segments {
            combined.push_str(segment.text.as_str());
        }
        if is_start && combined.trim().is_empty() {
            return Vec::new();
        }

        // Render body content - strip ANSI codes to ensure plain text output.
        // Use the session PTY fallback chain (pty_body -> tool_body -> foreground)
        // and apply a consistent dimmed style for terminal output.
        let pty_fallback = self.text_fallback(InlineMessageKind::Pty).or(self.theme.foreground);

        // Command headers (`• Ran ...`) and their wrapped `│` continuations
        // render like single-line Tool headers: status-colored bullet, bold
        // verb, uniform non-bold body. Shell token colors never leak in, so a
        // wrapped multiline command matches a single-line row instead of
        // going bold on continuations.
        // Every PTY line can begin a new tool command. `is_start` only marks
        // the beginning of the surrounding PTY block, so using it here would
        // incorrectly dim commands that follow an earlier command's output.
        let is_command_header = parse_tool_call_prefix(&combined).is_some();
        let is_header_continuation = !is_command_header && self.pty_is_header_continuation(index);

        let mut body_spans = Vec::with_capacity(line.segments.len() + 1);
        if is_command_header {
            // A streamed header can arrive as one status-colored segment or
            // as shell-highlighted `•`/verb/body segments. Split the full
            // text once so the body is always uniform: keep the status color
            // on the bullet only and never let it leak into the command.
            let mut header_text = String::with_capacity(combined.len());
            for segment in &line.segments {
                header_text.push_str(&render::strip_ansi_codes(&segment.text));
            }
            if let Some((action, prefix)) = parse_tool_call_prefix(&header_text) {
                let fg = self.theme.foreground.map(ratatui_color_from_ansi);
                let bg = self.theme.background.map(ratatui_color_from_ansi);

                let mut bullet_style = line
                    .segments
                    .first()
                    .map(|segment| {
                        ratatui_style_from_inline(&segment.style, pty_fallback).remove_modifier(Modifier::DIM)
                    })
                    .unwrap_or_default();
                if bullet_style.fg.is_none() {
                    if let Some(c) = fg {
                        bullet_style = bullet_style.fg(c);
                    }
                }
                if bullet_style.bg.is_none() {
                    if let Some(c) = bg {
                        bullet_style = bullet_style.bg(c);
                    }
                }
                let bullet = &prefix[..prefix.len() - action.len()];
                body_spans.push(Span::styled(bullet.to_owned(), bullet_style));

                body_spans.push(Span::styled(action.to_owned(), self.tool_header_action_style(action)));

                let rest = &header_text[prefix.len()..];
                if !rest.is_empty() {
                    body_spans.push(Span::styled(rest.to_owned(), self.tool_header_body_style()));
                }
            } else {
                for segment in &line.segments {
                    let stripped_text = render::strip_ansi_codes(&segment.text);
                    body_spans.push(Span::styled(
                        stripped_text.into_owned(),
                        self.opaque_tool_header_text_style(ratatui_style_from_inline(&segment.style, pty_fallback)),
                    ));
                }
            }
        } else {
            let header_body = is_header_continuation.then(|| self.tool_header_body_style());
            for segment in &line.segments {
                let stripped_text = render::strip_ansi_codes(&segment.text);
                let style = match header_body {
                    Some(body) => body,
                    None => ratatui_pty_style_from_inline(&segment.style, pty_fallback),
                };
                body_spans.push(Span::styled(stripped_text.into_owned(), style));
            }
        }

        let body_prefix = ui::INLINE_PTY_BODY_GUTTER;
        let continuation_prefix = text_utils::pty_wrapped_continuation_prefix(body_prefix, combined.as_str());
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

        if is_end {
            push_spacing_blanks(&mut lines, self.tool_block_spacing());
        }

        let mut transcript_lines =
            build_pty_transcript_lines(lines, &line.link_ranges, body_prefix, continuation_prefix.as_str());
        // Single ownership, as for tool blocks above: the previous block
        // suppresses its trailing gap before PTY blocks, so this top gap is
        // the only one.
        if is_start && self.should_add_tool_block_top_spacing(index) {
            let spacing = self.tool_block_spacing();
            let mut spaced = Vec::with_capacity(transcript_lines.len() + spacing);
            push_spacing_transcript_lines(&mut spaced, spacing);
            spaced.append(&mut transcript_lines);
            transcript_lines = spaced;
        }
        transcript_lines
    }
}

fn build_pty_transcript_lines(
    lines: Vec<Line<'static>>,
    link_ranges: &[InlineLinkRange],
    first_prefix: &str,
    continuation_prefix: &str,
) -> Vec<TranscriptLine> {
    let mut combined_offset = 0usize;
    let mut transcript_lines = Vec::with_capacity(lines.len());

    for (index, line) in lines.into_iter().enumerate() {
        let prefix = if index == 0 { first_prefix } else { continuation_prefix };
        let full_text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
        let body_text = full_text.trim_prefix(prefix);
        let body_end = combined_offset + body_text.len();
        let mut explicit_links = Vec::new();

        for link in link_ranges {
            let start = link.start.max(combined_offset);
            let end = link.end.min(body_end);
            if start >= end {
                continue;
            }

            let local_start = start - combined_offset;
            let local_end = end - combined_offset;
            let start_col = UnicodeWidthStr::width(prefix) + UnicodeWidthStr::width(&body_text[..local_start]);
            let width = UnicodeWidthStr::width(&body_text[local_start..local_end]);
            if width == 0 {
                continue;
            }

            explicit_links.push(RenderedTranscriptLink {
                start: prefix.len() + local_start,
                end: prefix.len() + local_end,
                start_col,
                width,
                target: link.target.clone(),
            });
        }

        transcript_lines.push(TranscriptLine { line, explicit_links });
        combined_offset = body_end;
    }

    transcript_lines
}

fn line_is_tool_command_header(line: &Line<'_>) -> bool {
    let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
    let stripped = text_utils::strip_ansi_codes(&text);
    parse_tool_call_prefix(stripped.trim_start()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::prelude::InlineTheme;

    fn span(text: &str, bg: Option<Color>) -> Span<'static> {
        let mut style = Style::default();
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        Span::styled(text.to_owned(), style)
    }

    fn fg_span(text: &str, fg: Color) -> Span<'static> {
        Span::styled(text.to_owned(), Style::default().fg(fg))
    }

    #[test]
    fn diff_row_bg_detects_body_and_headers() {
        let add_bg = Some(Color::Rgb(20, 58, 45));
        assert_eq!(diff_row_bg(&[span("+ 12 │ + new", add_bg)]), add_bg);
        assert_eq!(diff_row_bg(&[span("- 10 │ - old", add_bg)]), add_bg);
        assert_eq!(diff_row_bg(&[span(" 11 │ ctx", add_bg)]), add_bg);
        assert_eq!(diff_row_bg(&[span("--- a/README.md", add_bg)]), add_bg);
        assert_eq!(diff_row_bg(&[span("+++ b/README.md", add_bg)]), add_bg);
        assert_eq!(diff_row_bg(&[span("@@ -100 +100 @@", add_bg)]), add_bg);
    }

    #[test]
    fn diff_row_bg_ignores_unstyled_or_non_diff_text() {
        assert_eq!(diff_row_bg(&[span("hello world", None)]), None);
        assert_eq!(diff_row_bg(&[span("bolt normal output", Some(Color::Black))]), None);
    }

    #[test]
    fn diff_row_bg_ignores_side_by_side_divider_tint() {
        let left_bg = Some(Color::Rgb(70, 38, 42));
        let right_bg = Some(Color::Rgb(20, 58, 45));
        let spans = vec![
            span("- 10 │ old", left_bg),
            Span::raw("│"),
            span("+ 10 │ new", right_bg),
        ];

        assert_eq!(diff_row_bg(&spans), None);
        assert_eq!(
            diff_row_bg(&[
                span("- 10 │ old", left_bg),
                Span::styled("│", Style::default().bg(Color::Reset)),
            ]),
            None
        );
    }

    #[test]
    fn is_diff_row_detects_foreground_only_headers_and_bodies() {
        assert!(is_diff_row(&[fg_span("--- a/README.md", Color::LightRed)]));
        assert!(is_diff_row(&[fg_span("+++ b/README.md", Color::LightGreen)]));
        assert!(is_diff_row(&[fg_span("@@ -100 +100 @@", Color::Cyan)]));
        assert!(is_diff_row(&[fg_span("+ new line", Color::LightGreen)]));
        assert!(is_diff_row(&[fg_span("- old line", Color::Rgb(255, 180, 180))]));
        assert!(is_diff_row(&[fg_span("+ light insertion", Color::Rgb(0, 92, 43))]));
        assert!(is_diff_row(&[fg_span("- light deletion", Color::Rgb(140, 20, 25))]));
        assert!(!is_diff_row(&[fg_span("- bullet item", Color::Gray)]));
        assert!(!is_diff_row(&[span("plain tool output", None)]));
    }

    #[test]
    fn pad_diff_row_extends_tint_to_full_width() {
        let bg = Some(Color::Rgb(20, 58, 45));
        let mut line = Line::from(vec![span("+ 1 │ hi", bg), span("", bg)]);
        pad_diff_row_to_width(&mut line, 20);
        let width: usize = line.spans.iter().map(Span::width).sum();
        assert_eq!(width, 20);
        assert_eq!(line.spans.last().and_then(|s| s.style.bg), bg);
    }

    #[test]
    fn pad_diff_row_leaves_non_diff_lines_alone() {
        let mut line = Line::from(vec![span("plain tool output", None)]);
        pad_diff_row_to_width(&mut line, 40);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(UnicodeWidthStr::width(line.spans[0].content.as_ref()), "plain tool output".len());
    }

    #[test]
    fn wrapped_diff_continuations_keep_the_source_row_tint() {
        let session = Session::new(InlineTheme::default(), None, 20);
        let row_bg = Color::Rgb(20, 58, 45);
        let content = vec![Span::styled(
            "+fn main() { this line is long enough to wrap without a space after the marker",
            Style::default().bg(row_bg),
        )];

        let wrapped = session.wrap_block_lines_no_right_border("", "", content, 20, Style::default());

        assert!(wrapped.len() > 1);
        for line in wrapped {
            assert!(line.spans.iter().any(|span| span.style.bg == Some(row_bg)));
            assert_eq!(line.spans.iter().map(Span::width).sum::<usize>(), 20);
        }
    }
}
