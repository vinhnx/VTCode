/// Transcript reflow and wrapping operations for Session
///
/// This module handles transcript line wrapping, reflowing, and formatting including:
/// - Message line reflowing based on width
/// - Text wrapping and justification
/// - Tool and PTY output formatting with borders
/// - Diff line padding
/// - Block line wrapping with borders
use std::path::Path;

use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::super::style::ratatui_style_from_inline;
use super::super::types::InlineMessageKind;
use super::wrapping;
use super::{
    Session,
    message::{MessageLine, TranscriptLine},
    render, text_utils, transcript_links,
};
use crate::tui::config::constants::ui;

mod blocks;
mod formatting;
mod helpers;
mod thinking;

pub(super) use helpers::parse_tool_call_prefix;
use helpers::{
    agent_code_continuation_prefix, is_bullet_summary_text, is_tool_summary_line, is_tree_detail_text,
    next_is_tool_block, push_spacing_blanks, push_spacing_transcript_lines, trim_trailing_blank_transcript_lines,
};

pub(super) fn is_info_box_line(message: &MessageLine) -> bool {
    matches!(message.kind, InlineMessageKind::Error | InlineMessageKind::Warning)
        || (message.kind == InlineMessageKind::Info && !is_tool_summary_line(message))
}

impl Session {
    /// Revisions for review sources, aggregating each diagnostic group once.
    /// The grouped block is cached under its head line; member rows retain
    /// their local revisions because they render as empty source rows.
    pub(crate) fn review_message_revisions(&self) -> Vec<u64> {
        let mut revisions = Vec::with_capacity(self.lines.len());
        let mut index = 0;
        while index < self.lines.len() {
            let message = &self.lines[index];
            if !is_info_box_line(message) {
                revisions.push(message.revision);
                index += 1;
                continue;
            }

            let group_start = index;
            let group_kind = message.kind;
            let mut group_revision = message.revision;
            index += 1;
            while let Some(line) = self.lines.get(index)
                && line.kind == group_kind
                && is_info_box_line(line)
            {
                group_revision = group_revision.max(line.revision);
                index += 1;
            }

            revisions.push(group_revision);
            revisions.extend(self.lines[group_start + 1..index].iter().map(|line| line.revision));
        }
        revisions
    }

    /// Reflow message lines for a given width (test-only method)
    #[cfg(test)]
    pub(super) fn reflow_transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            let mut lines: Vec<Line<'static>> = Vec::new();
            for (index, _) in self.lines.iter().enumerate() {
                lines.extend(self.reflow_message_lines(index, 0, false).into_iter().map(|line| line.line));
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            return lines;
        }

        let mut wrapped_lines = Vec::new();
        for (index, _) in self.lines.iter().enumerate() {
            wrapped_lines.extend(self.reflow_message_lines(index, width, false).into_iter().map(|line| line.line));
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
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(super) fn reflow_message_lines(
        &self,
        index: usize,
        width: u16,
        collapse_thinking: bool,
    ) -> Vec<TranscriptLine> {
        let Some(message) = self.lines.get(index) else {
            return vec![TranscriptLine::default()];
        };

        // Thinking/reasoning blocks are rendered as contiguous `Policy` runs.
        // Delegate the whole run (header + body) to the dedicated thinking
        // module so collapsed and expanded states share one layout.
        if collapse_thinking && let Some(lines) = self.reflow_thinking_lines(index, width) {
            return lines;
        }

        if message.kind == InlineMessageKind::Tool {
            return into_transcript_lines(self.reflow_tool_lines(index, width), self.workspace_root.as_deref());
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

        if width == 0 {
            // Hotpath: Agent messages rebuild spans inside
            // `reflow_agent_message_lines` and never use `base_line` — keep the
            // width==0 content path for all kinds, but avoid the redundant
            // `render_message_spans` alloc on the Agent width!=0 path below.
            let spans = render::render_message_spans(self, index);
            let base_line = Line::from(spans);
            return vec![transcript_line_with_detected_links(
                base_line,
                self.workspace_root.as_deref(),
            )];
        }

        let mut wrapped: Vec<TranscriptLine> = Vec::new();
        let max_width = width as usize;

        let is_tool_summary = message.kind == InlineMessageKind::Info && is_tool_summary_line(message);
        if is_tool_summary && self.should_add_tool_block_top_spacing(index) {
            // Single ownership: the previous block suppresses its trailing gap
            // before tool blocks (see `next_is_tool_block`), so this top gap
            // (min 1) is the only one.
            push_spacing_transcript_lines(&mut wrapped, self.tool_block_spacing());
        }

        // Check if this is the start of a new conversation turn
        let prev_kind = if index > 0 {
            self.lines.get(index - 1).map(|l| l.kind)
        } else {
            None
        };
        let is_new_turn = prev_kind.is_none()
            || (message.kind == InlineMessageKind::User && prev_kind != Some(InlineMessageKind::User))
            || (message.kind == InlineMessageKind::Agent && prev_kind != Some(InlineMessageKind::Agent));

        let spacing = self.appearance.message_block_spacing.min(2) as usize;

        // Divider before User messages. No blank rows are emitted here: the
        // previous block owns the trailing gap (possibly 0 when spacing is 0),
        // so emitting top spacing as well would double every turn
        // (Agent bottom + User top + divider). The divider alone separates.
        if message.kind == InlineMessageKind::User && is_new_turn && max_width > 0 {
            let divider = self.message_divider_line(max_width, message.kind);
            wrapped.push(transcript_line_with_detected_links(divider, self.workspace_root.as_deref()));
        }

        // Section divider before Agent synthesis following tool work. Same
        // single-ownership pattern as the User divider: the tool/PTY block
        // owns the trailing gap above (tool blocks clamp to min 1), so only
        // the thin accent rule is emitted here. Keeps long tool bursts and
        // follow-up prose visually distinct without extra vertical clutter.
        // Skipped for empty agent rows, reasoning flow (`Policy`), and
        // consecutive agent lines so the transcript reads like prose.
        if message.kind == InlineMessageKind::Agent && is_new_turn && max_width > 0 && index > 0 {
            let prev_is_tool_section = self.lines.get(index - 1).is_some_and(|prev| next_is_tool_block(Some(prev)));
            let agent_has_content = message.segments.iter().any(|segment| !segment.text.trim().is_empty());
            if prev_is_tool_section && agent_has_content {
                let divider = self.message_divider_line(max_width, InlineMessageKind::Agent);
                wrapped.push(transcript_line_with_detected_links(divider, self.workspace_root.as_deref()));
            }
        }

        let lines = if message.kind == InlineMessageKind::Agent {
            self.reflow_agent_message_lines(message, max_width, !is_new_turn)
        } else {
            let spans = render::render_message_spans(self, index);
            let base_line = Line::from(spans);
            let mut lines = self.wrap_line(base_line, max_width);
            if !lines.is_empty() {
                // Links on this path are detected after justification (see
                // `into_transcript_lines`), so no row carries links yet.
                self.justify_wrapped_lines_in_place(&mut lines, max_width, message.kind, &[]);
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            into_transcript_lines(lines, self.workspace_root.as_deref())
        };

        let mut wrapped_lines = if wrapped.is_empty() {
            // Hotpath: consecutive Agent rows carry no divider — reuse the
            // `lines` buffer directly instead of `extend` into a new Vec
            // (saves one outer Vec alloc per message, ~2.0MB in tui_hotpath).
            lines
        } else {
            wrapped.extend(lines);
            wrapped
        };

        if wrapped_lines.is_empty() {
            wrapped_lines.push(TranscriptLine::default());
        }

        // Content may end with blank rows (e.g. agent text ending in `\n\n`
        // or markdown paragraph gaps). Trim them so the separator below
        // contributes exactly one gap instead of stacking.
        trim_trailing_blank_transcript_lines(&mut wrapped_lines);
        if wrapped_lines.is_empty() {
            wrapped_lines.push(TranscriptLine::default());
            return wrapped_lines;
        }

        // Add spacing after messages for visual grouping (respects message_block_spacing config)
        let next_line = self.lines.get(index + 1);
        let next_kind = next_line.map(|l| l.kind);
        match message.kind {
            InlineMessageKind::Error | InlineMessageKind::Info | InlineMessageKind::Warning => {
                // A tool block owns its own top gap (clamped to min 1), so a
                // preceding summary must not emit trailing spacing as well.
                // (`next_is_tool_block` covers Tool/Pty and Info summaries,
                // i.e. every case the old inline match handled.)
                if !next_is_tool_block(next_line) {
                    let spacing = if is_tool_summary {
                        self.tool_block_spacing()
                    } else {
                        spacing
                    };
                    push_spacing_transcript_lines(&mut wrapped_lines, spacing);
                }
            }
            // No spacing if followed by Agent (reasoning -> content flow).
            // A tool block owns its own top gap, so suppress here as well.
            InlineMessageKind::Policy
                if next_kind != Some(InlineMessageKind::Agent) && !next_is_tool_block(next_line) =>
            {
                push_spacing_transcript_lines(&mut wrapped_lines, spacing);
            }
            // A tool block owns its own top gap (min 1 guaranteed), so a User
            // message directly before one must not emit trailing spacing.
            InlineMessageKind::User if !next_is_tool_block(next_line) => {
                // Blank lines after user message for clean separation
                push_spacing_transcript_lines(&mut wrapped_lines, spacing);
            }
            InlineMessageKind::User => {}
            // End of an agent turn. Prose keeps one blank line before a
            // following tool block so the message stays visually distinct
            // from its work log (single ownership: the agent suppresses its
            // trailing gap here and the tool block owns the top gap, clamped
            // to min 1 via `tool_block_spacing`). Any other follower keeps at
            // least one blank line (even when message_block_spacing is 0) so
            // the response stays distinct.
            InlineMessageKind::Agent
                if next_kind.is_some()
                    && next_kind != Some(InlineMessageKind::Agent)
                    && !next_is_tool_block(next_line) =>
            {
                let gap = spacing.max(1);
                push_spacing_transcript_lines(&mut wrapped_lines, gap);
            }
            InlineMessageKind::Agent => {}
            _ => {}
        }

        wrapped_lines
    }

    fn tool_block_spacing(&self) -> usize {
        self.appearance.message_block_spacing.clamp(1, 2) as usize
    }

    fn should_add_tool_block_top_spacing(&self, index: usize) -> bool {
        let Some(previous) = index.checked_sub(1).and_then(|previous| self.lines.get(previous)) else {
            return true;
        };
        let Some(current) = self.lines.get(index) else {
            return true;
        };
        should_add_tool_block_top_spacing_for_kinds(previous, current)
    }

    pub(crate) fn reflow_message_lines_for_review(&self, index: usize, width: u16) -> Vec<TranscriptLine> {
        let Some(collapsed) = self.collapsed_pastes.iter().find(|paste| paste.line_index == index) else {
            return self.reflow_message_lines(index, width, false);
        };

        let Some(line) = self.lines.get(index) else {
            return vec![TranscriptLine::default()];
        };

        let mut expanded = line.clone();
        expanded.segments = vec![crate::tui::core_tui::types::InlineSegment {
            text: collapsed.full_text.clone(),
            style: std::sync::Arc::new(crate::tui::core_tui::types::InlineTextStyle::default()),
        }];
        expanded.link_ranges.clear();

        let rendered = Line::from(self.render_message_spans_for_line(&expanded));
        if width == 0 {
            return vec![transcript_line_with_detected_links(
                rendered,
                self.workspace_root.as_deref(),
            )];
        }

        let mut wrapped = self.wrap_line(rendered, width as usize);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }
        into_transcript_lines(wrapped, self.workspace_root.as_deref())
    }

    /// Reflow error, warning, and info messages as plain colored lines.
    ///
    /// Consecutive same-kind lines stay grouped (head owns the reflow so
    /// transcript caches invalidate from the first line), but render without
    /// borders or labels: semantic text color only (Error red, Warning amber,
    /// Info dimmed foreground), normal wrapping, single trailing gap.
    fn reflow_error_warning_lines(&self, index: usize, width: u16) -> Vec<TranscriptLine> {
        let Some(line) = self.lines.get(index) else {
            return vec![TranscriptLine::default()];
        };
        let kind = line.kind;

        let max_width = if width == 0 { usize::MAX } else { width as usize };

        let mut grouped_lines = Vec::new();
        let mut cursor = index;
        while let Some(current) = self.lines.get(cursor) {
            if current.kind != kind || !is_info_box_line(current) {
                break;
            }
            let mut spans = render::render_message_spans(self, cursor);
            for span in &mut spans {
                span.style = span.style.remove_modifier(Modifier::BOLD);
                if kind == InlineMessageKind::Info {
                    span.style = span.style.add_modifier(Modifier::DIM);
                }
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
            return into_transcript_lines(grouped_lines, self.workspace_root.as_deref());
        }

        let mut lines = Vec::new();
        for line in grouped_lines {
            for wrapped_line in self.wrap_line(line, max_width) {
                let text: String = wrapped_line.spans.iter().map(|span| &*span.content).collect();
                if !text.trim().is_empty() {
                    lines.push(wrapped_line);
                }
            }
        }
        if lines.is_empty() {
            return Vec::new();
        }

        // A tool block owns its own top gap (min 1 guaranteed), so a group
        // directly before one must not emit trailing spacing as well.
        if !next_is_tool_block(self.lines.get(cursor)) {
            let spacing = self.appearance.message_block_spacing.min(2) as usize;
            push_spacing_blanks(&mut lines, spacing);
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        into_transcript_lines(lines, self.workspace_root.as_deref())
    }

    /// Wrap a line of text to fit within a maximum width.
    ///
    /// Uses URL-aware wrapping to preserve URL clickability across all views.
    /// URLs are treated as atomic units and never split across lines.
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(super) fn wrap_line(&self, line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
        wrapping::wrap_line_preserving_urls(line, max_width)
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn reflow_agent_message_lines(
        &self,
        message: &MessageLine,
        max_width: usize,
        suppress_prefix_bullet: bool,
    ) -> Vec<TranscriptLine> {
        if max_width == 0 {
            return vec![TranscriptLine::default()];
        }

        let mut prefix_spans = render::agent_prefix_spans(self, message);
        let left_padding = ui::INLINE_AGENT_MESSAGE_LEFT_PADDING;
        let prefix_width =
            prefix_spans.iter().map(|span| span.width()).sum::<usize>() + UnicodeWidthStr::width(left_padding);

        let content_width = max_width.saturating_sub(prefix_width);
        let fallback = self.text_fallback(message.kind).or(self.theme.foreground);
        // Only materialize joined text when a segment might contain a link.
        let content_may_link = message
            .segments
            .iter()
            .any(|segment| transcript_links::may_contain_link_candidate_text(&segment.text));
        let content_text: String = if content_may_link {
            message.segments.iter().map(|s| s.text.as_str()).collect()
        } else {
            String::new()
        };
        let mut content_spans = Vec::with_capacity(message.segments.len());
        for segment in &message.segments {
            let style = ratatui_style_from_inline(&segment.style, fallback);
            content_spans.push(Span::styled(segment.text.clone(), style));
        }
        let content_line = Line::from(content_spans);

        let code_continuation_prefix = agent_code_continuation_prefix(message);

        let (mut wrapped, mut explicit_links) = if content_width == 0 {
            (vec![Line::default()], vec![Vec::new()])
        } else {
            let wrapped = if let Some(prefix) = code_continuation_prefix.as_deref() {
                text_utils::wrap_line_with_hanging_prefix(content_line, content_width, prefix)
            } else {
                self.wrap_line(content_line, content_width)
            };
            // Hotpath: plain prose (no link candidates) needs no projected
            // links — leave the outer Vec empty and emit per-line empty Vecs
            // below, avoiding one outer alloc per Agent message (7401 in
            // tui_hotpath) plus the row_has_links bool Vec.
            let explicit_links = if content_may_link {
                transcript_links::project_detected_links_onto_wrapped_lines(
                    &wrapped,
                    &content_text,
                    self.workspace_root.as_deref(),
                )
            } else {
                Vec::new()
            };
            (wrapped, explicit_links)
        };

        if !wrapped.is_empty() {
            // Justification inserts interior spaces, so rows carrying
            // projected links keep their wrapped text: their link ranges were
            // computed against it and would otherwise drift out of alignment.
            if content_may_link {
                let row_has_links: Vec<bool> = explicit_links.iter().map(|links| !links.is_empty()).collect();
                self.justify_wrapped_lines_in_place(&mut wrapped, content_width, message.kind, &row_has_links);
            } else {
                self.justify_wrapped_lines_in_place(&mut wrapped, content_width, message.kind, &[]);
            }
        }
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }
        if content_may_link {
            explicit_links.resize_with(wrapped.len(), Vec::new);
        }

        let suppress_prefix_bullet = suppress_prefix_bullet
            || wrapped.first().is_some_and(|line| {
                let line_text = line
                    .spans
                    .iter()
                    .map(|span| AsRef::<str>::as_ref(&span.content))
                    .collect::<String>();
                let stripped = text_utils::strip_ansi_codes(&line_text);
                // Suppress bullet for empty messages or messages starting with list items
                stripped.trim().is_empty() || text_utils::is_list_item(stripped.as_ref())
            });
        if suppress_prefix_bullet
            && let Some(prefix_span) = prefix_spans
                .first_mut()
                .filter(|span| AsRef::<str>::as_ref(&span.content) == ui::INLINE_AGENT_QUOTE_PREFIX)
        {
            // Zero-indent: a suppressed bullet leaves no indent so the first
            // row starts at column 0 like every continuation row.
            prefix_span.content = String::new().into();
        }

        // Hotpath fast-path: default Agent prose has no bullet/label/padding,
        // so both prefix Strings are empty and link offsets need no
        // adjustment. Check before allocating the prefix String to skip one
        // String alloc per message, the per-first-line empty span, and (for
        // plain prose) any explicit-links outer Vec.
        if prefix_spans.is_empty() && left_padding.is_empty() {
            let mut lines = Vec::with_capacity(wrapped.len());
            if content_may_link {
                for (line, line_links) in wrapped.into_iter().zip(explicit_links) {
                    lines.push(TranscriptLine { line, explicit_links: line_links });
                }
            } else {
                for line in wrapped {
                    lines.push(TranscriptLine { line, explicit_links: Vec::new() });
                }
            }
            if lines.is_empty() {
                lines.push(TranscriptLine::default());
            }
            return lines;
        }
        let first_line_prefix_text = {
            let mut text: String = prefix_spans.iter().map(|span| span.content.as_ref()).collect();
            text.push_str(left_padding);
            text
        };
        let first_line_prefix_width = UnicodeWidthStr::width(first_line_prefix_text.as_str());
        let continuation_prefix = " ".repeat(first_line_prefix_width);
        // Slow path (label/prefix configured): ensure link Vecs align for zip.
        if !content_may_link {
            explicit_links.resize_with(wrapped.len(), Vec::new);
        }
        let mut lines = Vec::with_capacity(wrapped.len());
        for (index, (mut line, mut line_links)) in wrapped.into_iter().zip(explicit_links).enumerate() {
            let mut spans = Vec::new();
            // Keep wrapped prose aligned under its body when a role label or
            // other first-line prefix is present. The default empty prefix
            // keeps ordinary assistant prose flush with the transcript edge.
            let (prefix_len, prefix_col_width) = if index == 0 {
                (first_line_prefix_text.len(), first_line_prefix_width)
            } else {
                (continuation_prefix.len(), first_line_prefix_width)
            };
            if index == 0 {
                spans.append(&mut prefix_spans);
                if !left_padding.is_empty() {
                    spans.push(Span::raw(left_padding));
                }
            } else if !continuation_prefix.is_empty() {
                spans.push(Span::raw(continuation_prefix.clone()));
            }
            spans.append(&mut line.spans);
            if spans.is_empty() {
                spans.push(Span::raw(String::new()));
            }

            for link in &mut line_links {
                link.start += prefix_len;
                link.end += prefix_len;
                link.start_col += prefix_col_width;
            }

            lines.push(TranscriptLine {
                line: Line::from(spans),
                explicit_links: line_links,
            });
        }

        if lines.is_empty() {
            lines.push(TranscriptLine::default());
        }

        lines
    }
}

/// Pure, independently testable policy for tool-block top spacing.
///
/// Returns `true` if a blank-line gap should be inserted before `current`
/// given `previous`. This isolates the intricate bullet/tree grouping logic
/// from `Session` state so it can be unit-tested without constructing a full
/// transcript.
pub(crate) fn should_add_tool_block_top_spacing_for_kinds(previous: &MessageLine, current: &MessageLine) -> bool {
    match previous.kind {
        InlineMessageKind::Tool | InlineMessageKind::Pty => false,
        // Agent prose stays visually distinct from its work log: the tool
        // block owns exactly one top gap (min 1 via `tool_block_spacing`).
        InlineMessageKind::Agent => true,
        InlineMessageKind::Info => {
            if !is_tool_summary_line(previous) {
                return true;
            }
            let prev_text: String = previous.segments.iter().map(|s| s.text.as_str()).collect();
            let curr_text: String = current.segments.iter().map(|s| s.text.as_str()).collect();
            let prev_is_bullet = is_bullet_summary_text(&prev_text);
            let prev_is_tree = is_tree_detail_text(&prev_text);
            let curr_is_tree = is_tree_detail_text(&curr_text);

            match current.kind {
                InlineMessageKind::Info => {
                    // Tight within a single tool's detail block:
                    // bullet -> tree, tree -> tree. Otherwise gap.
                    !(curr_is_tree && (prev_is_bullet || prev_is_tree))
                }
                InlineMessageKind::Tool | InlineMessageKind::Pty => {
                    // A Tool/Pty block after an Info bullet/tree should start
                    // with a gap (separate visual block).
                    true
                }
                _ => true,
            }
        }
        _ => true,
    }
}

pub(super) fn transcript_line_with_detected_links(
    line: Line<'static>,
    workspace_root: Option<&Path>,
) -> TranscriptLine {
    let explicit_links = transcript_links::detect_rendered_transcript_links(&line, workspace_root);
    TranscriptLine { line, explicit_links }
}

fn into_transcript_lines(lines: Vec<Line<'static>>, workspace_root: Option<&Path>) -> Vec<TranscriptLine> {
    lines
        .into_iter()
        .map(|line| transcript_line_with_detected_links(line, workspace_root))
        .collect()
}
