use anstyle::Color as AnsiColorEnum;
/// Message operations for Session
///
/// This module handles message-related operations including:
/// - Adding and appending messages to the transcript
/// - Managing message lines and segments
/// - Handling tool code fence markers
/// - Message styling and prefixes
use std::cmp::min;
use std::collections::VecDeque;

use super::super::types::{InlineMessageKind, InlineSegment, InlineTextStyle};
use super::{CollapsedPaste, Session, message::MessageLine};
use crate::tui::config::constants::ui;
use crate::tui::ui::tui::types::InlineLinkRange;

const USER_PREFIX: &str = "";
const INLINE_JSON_COLLAPSE_BYTES: usize = 50_000;

fn is_large_json_payload(kind: InlineMessageKind, text: &str, line_count: usize) -> bool {
    if !matches!(kind, InlineMessageKind::Tool | InlineMessageKind::Pty) {
        return false;
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return false;
    }
    if !(trimmed.ends_with('}') || trimmed.ends_with(']')) {
        return false;
    }

    let effective_lines = if line_count == 0 {
        text.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1
    } else {
        line_count
    };

    text.len() >= INLINE_JSON_COLLAPSE_BYTES || effective_lines >= ui::INLINE_JSON_COLLAPSE_LINE_THRESHOLD
}

fn tail_lines(text: &str, limit: usize) -> Vec<&str> {
    if limit == 0 {
        return Vec::new();
    }

    let mut buffer: VecDeque<&str> = VecDeque::with_capacity(limit);
    for line in text.lines() {
        if buffer.len() == limit {
            buffer.pop_front();
        }
        buffer.push_back(line);
    }

    buffer.into_iter().collect()
}

impl Session {
    pub(crate) fn retint_lines_for_theme_change(&mut self, previous_theme: &super::super::types::InlineTheme) {
        let previous_colors = [
            previous_theme.foreground.as_ref(),
            previous_theme.primary.as_ref(),
            previous_theme.secondary.as_ref(),
            previous_theme.tool_accent.as_ref(),
            previous_theme.tool_body.as_ref(),
            previous_theme.pty_body.as_ref(),
        ];

        let mut changed_indices = Vec::with_capacity(self.lines.len());
        for (line_index, line) in self.lines.iter_mut().enumerate() {
            match line.kind {
                InlineMessageKind::Tool | InlineMessageKind::Pty => continue,
                InlineMessageKind::Agent
                | InlineMessageKind::User
                | InlineMessageKind::Policy
                | InlineMessageKind::Error
                | InlineMessageKind::Warning
                | InlineMessageKind::Info => {}
            }

            let mut line_changed = false;
            for segment in &mut line.segments {
                let mut updated_style = (*segment.style).clone();
                if let Some(color) = updated_style.color.as_ref()
                    && previous_colors.iter().flatten().any(|candidate| *candidate == color)
                {
                    updated_style.color = None;
                    line_changed = true;
                }
                if line_changed {
                    segment.style = std::sync::Arc::new(updated_style);
                }
            }
            if line_changed {
                changed_indices.push(line_index);
            }
        }

        for line_index in changed_indices.iter().copied() {
            let revision = self.next_revision();
            if let Some(line) = self.lines.get_mut(line_index) {
                line.revision = revision;
            }
        }

        if !changed_indices.is_empty() {
            self.mark_line_dirty(0);
            self.invalidate_transcript_cache();
            self.invalidate_scroll_metrics();
        }
    }

    /// Get the prefix text for a message kind
    #[expect(dead_code)]
    pub(crate) fn prefix_text(&self, kind: InlineMessageKind) -> Option<String> {
        match kind {
            InlineMessageKind::User => Some(self.labels.user.clone().unwrap_or_else(|| USER_PREFIX.to_owned())),
            InlineMessageKind::Agent => None,
            InlineMessageKind::Policy => self.labels.agent.clone(),
            InlineMessageKind::Tool
            | InlineMessageKind::Pty
            | InlineMessageKind::Error
            | InlineMessageKind::Warning => None,
            InlineMessageKind::Info => None,
        }
    }

    /// Get the prefix style for a message line
    #[expect(dead_code)]
    pub(crate) fn prefix_style(&self, line: &MessageLine) -> InlineTextStyle {
        self.styles.prefix_style(line)
    }

    /// Get the text fallback color for a message kind
    pub(crate) fn text_fallback(&self, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
        self.styles.text_fallback(kind)
    }

    /// Push a new message line to the transcript
    pub(crate) fn push_line(&mut self, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        let previous_max_offset = self.current_max_scroll_offset();
        let revision = self.next_revision();
        let index = self.lines.len();
        self.lines
            .push(MessageLine { kind, segments, link_ranges: Vec::new(), revision });
        self.mark_line_dirty(index);
        if !self.is_streaming_final_answer {
            self.invalidate_scroll_metrics();
        }
        self.adjust_scroll_after_change(previous_max_offset);

        // Mark thinking spinner as active after user message (no placeholder line - just state)
        if kind == InlineMessageKind::User {
            self.thinking_spinner.start();
        }

        // Keep the collapsed thinking summary's live line count current as a
        // reasoning block grows (otherwise it stays frozen until a click).
        if kind == InlineMessageKind::Policy {
            // Track the run currently being streamed so live updates stay O(1).
            let is_new_run = self.prev_pushed_line_kind() != Some(InlineMessageKind::Policy);
            if is_new_run {
                self.thinking_runs.begin_run(self.lines.len() - 1);
            }
            self.mark_thinking_run_starts_dirty();
        } else if self.prev_pushed_line_kind() == Some(InlineMessageKind::Policy) {
            // A non-reasoning line ends the active reasoning run.
            self.thinking_runs.end_run();
            // Clean up the trailing empty Policy line created by the streaming
            // `\n` suffix in `append_inline`. It sits right before the new line.
            if self.lines.len() >= 2 {
                let idx = self.lines.len() - 2;
                if self.lines[idx].kind == InlineMessageKind::Policy && self.lines[idx].segments.is_empty() {
                    let notify = idx;
                    self.lines.remove(idx);
                    self.mark_line_dirty(notify);
                }
            }
        }
    }

    /// Kind of the line immediately preceding the most recently pushed line, if
    /// any. Used to decide whether a `Policy` line opens a new reasoning run.
    fn prev_pushed_line_kind(&self) -> Option<InlineMessageKind> {
        self.lines
            .len()
            .checked_sub(2)
            .and_then(|i| self.lines.get(i))
            .map(|line| line.kind)
    }

    /// Append a large pasted message as a collapsible placeholder.
    pub(crate) fn append_pasted_message(&mut self, kind: InlineMessageKind, text: String, line_count: usize) {
        if is_large_json_payload(kind, &text, line_count) {
            let mut preview = format!("[...] showing last {} lines - click to expand", ui::INLINE_JSON_TAIL_LINES);

            let tail = tail_lines(&text, ui::INLINE_JSON_TAIL_LINES);
            if !tail.is_empty() {
                preview.push('\n');
                for (idx, line) in tail.iter().enumerate() {
                    if idx > 0 {
                        preview.push('\n');
                    }
                    preview.push_str(line);
                }
            }

            let line_index = self.lines.len();
            self.push_line(
                kind,
                vec![InlineSegment {
                    text: preview,
                    style: std::sync::Arc::new(InlineTextStyle::default()),
                }],
            );
            self.collapsed_pastes.push(CollapsedPaste { line_index, full_text: text });
            return;
        }

        self.push_line(
            kind,
            vec![InlineSegment {
                text,
                style: std::sync::Arc::new(InlineTextStyle::default()),
            }],
        );
    }

    /// Append a segment to the transcript, handling newlines and control characters
    pub(crate) fn append_inline(&mut self, kind: InlineMessageKind, segment: InlineSegment) {
        let previous_max_offset = self.current_max_scroll_offset();

        // For Tool messages, process the entire text as one unit to avoid excessive line breaks
        // Newlines in tool output will be preserved as actual newline characters rather than
        // triggering new message lines
        if kind == InlineMessageKind::Tool {
            self.append_text(kind, &segment.text, &segment.style);
        } else {
            let mut remaining = segment.text.as_str();
            let style = segment.style.clone();

            while !remaining.is_empty() {
                if let Some((index, control)) = remaining.char_indices().find(|(_, ch)| matches!(ch, '\n' | '\r')) {
                    let (text, _) = remaining.split_at(index);
                    if !text.is_empty() {
                        self.append_text(kind, text, &style);
                    }

                    let control_char = control;
                    let next_index = index + control_char.len_utf8();
                    remaining = &remaining[next_index..];

                    match control_char {
                        '\n' => self.start_line(kind),
                        '\r' => {
                            if remaining.starts_with('\n') {
                                remaining = &remaining[1..];
                                self.start_line(kind);
                            } else {
                                self.reset_line(kind);
                            }
                        }
                        _ => {}
                    }
                } else {
                    if !remaining.is_empty() {
                        self.append_text(kind, remaining, &style);
                    }
                    break;
                }
            }
        }

        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
    }

    /// Replace the last N message lines with new lines
    pub(crate) fn replace_last(
        &mut self,
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
        link_ranges: Option<Vec<Vec<InlineLinkRange>>>,
    ) {
        let previous_max_offset = self.current_max_scroll_offset();
        let remove_count = min(count, self.lines.len());
        let first_removed = self.lines.len().saturating_sub(remove_count);
        self.collapsed_pastes.retain(|paste| paste.line_index < first_removed);
        let first_dirty = self.lines.len().saturating_sub(remove_count);
        self.lines.truncate(self.lines.len().saturating_sub(remove_count));
        let mut link_ranges = link_ranges.unwrap_or_default().into_iter();
        for segments in lines {
            let revision = self.next_revision();
            self.lines.push(MessageLine {
                kind,
                segments,
                link_ranges: link_ranges.next().unwrap_or_default(),
                revision,
            });
        }
        self.mark_line_dirty(first_dirty);
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);

        // Keep thinking-run tracking in sync: a `ReplaceLast` that creates
        // the first `Policy` line must register a new run so downstream
        // `mark_thinking_run_starts_dirty` can find an active start.
        if kind == InlineMessageKind::Policy && self.thinking_runs.active_start().is_none() {
            self.thinking_runs.begin_run(first_dirty);
        } else if kind != InlineMessageKind::Policy {
            self.thinking_runs.end_run();
        }
    }

    pub(crate) fn expand_collapsed_paste_at_line_index(&mut self, line_index: usize) -> bool {
        if self.collapsed_pastes.is_empty() {
            return false;
        }

        let Some(index) = self.collapsed_pastes.iter().position(|paste| paste.line_index == line_index) else {
            return false;
        };

        let collapsed = self.collapsed_pastes.remove(index);
        let revision = self.next_revision();
        let Some(line) = self.lines.get_mut(collapsed.line_index) else {
            return false;
        };

        line.segments = vec![InlineSegment {
            text: collapsed.full_text,
            style: std::sync::Arc::new(InlineTextStyle::default()),
        }];
        line.link_ranges.clear();
        line.revision = revision;
        self.mark_line_dirty(collapsed.line_index);
        self.invalidate_scroll_metrics();
        true
    }

    pub(crate) fn expand_collapsed_paste_at_row(&mut self, width: u16, row: usize) -> bool {
        if self.collapsed_pastes.is_empty() || width == 0 {
            return false;
        }

        let message_index = {
            let cache = self.ensure_reflow_cache(width);
            if cache.row_offsets.is_empty() {
                None
            } else {
                let idx = match cache.row_offsets.binary_search(&row) {
                    Ok(idx) => idx,
                    Err(0) => return false,
                    Err(pos) => pos.saturating_sub(1),
                };
                let start = cache.row_offsets.get(idx).copied().unwrap_or(0);
                let height = cache.messages.get(idx).map(|msg| msg.lines.len()).unwrap_or(1);
                if row < start.saturating_add(height.max(1)) {
                    Some(idx)
                } else {
                    None
                }
            }
        };

        match message_index {
            Some(index) => self.expand_collapsed_paste_at_line_index(index),
            None => false,
        }
    }

    /// Toggle the collapsed/expanded state of the thinking/reasoning block whose
    /// summary line is at the given viewport row. Returns `true` if a block was
    /// toggled (and the transcript should be re-rendered).
    pub(crate) fn toggle_thinking_block_at_row(&mut self, width: u16, row: usize) -> bool {
        if width == 0 {
            return false;
        }

        let line_index = {
            let cache = self.ensure_reflow_cache(width);
            if cache.row_offsets.is_empty() {
                return false;
            }
            let idx = match cache.row_offsets.binary_search(&row) {
                Ok(idx) => idx,
                Err(0) => return false,
                Err(pos) => pos.saturating_sub(1),
            };
            let start = cache.row_offsets.get(idx).copied().unwrap_or(0);
            let height = cache.messages.get(idx).map(|msg| msg.lines.len()).unwrap_or(1);
            if row < start.saturating_add(height.max(1)) {
                Some(idx)
            } else {
                None
            }
        };

        let Some(line_index) = line_index else {
            return false;
        };

        // Resolve the start of the contiguous Policy (thinking) run.
        let mut start = line_index;
        while start > 0 {
            let Some(prev) = self.lines.get(start - 1) else {
                break;
            };
            if prev.kind != InlineMessageKind::Policy {
                break;
            }
            start -= 1;
        }
        if self.lines.get(start).map(|line| line.kind) != Some(InlineMessageKind::Policy) {
            return false;
        }

        let collapsed = self.thinking_collapsed(start);
        self.thinking_runs.set_collapsed(start, !collapsed);

        // Bump the revision of every line in the run so the reflow cache
        // recomputes both the run-start summary and the (now hidden or shown)
        // continuation lines. Without this the cache sees no change and the
        // transcript keeps rendering the previous state.
        let run_len = self.thinking_run_len(start);
        let revision = self.next_revision();
        for line in self.lines.iter_mut().skip(start).take(run_len) {
            if line.kind == InlineMessageKind::Policy {
                line.revision = revision;
            }
        }
        self.mark_line_dirty(start);
        self.invalidate_scroll_metrics();
        // Drop the cached visible-window: it is keyed only by viewport
        // offset/width/height, so without this the post-toggle render would
        // keep returning the stale (pre-toggle) lines even though the reflow
        // cache itself was updated.
        self.invalidate_transcript_viewport();
        true
    }

    /// Bump the revision of the actively-streaming thinking run's start line and
    /// drop the visible-window cache, so its collapsed summary recomputes its
    /// live line count (and spinner frame) instead of staying frozen until a
    /// click.
    ///
    /// O(1): only the single active run-start is touched. Call this
    /// while reasoning streams (on each new reasoning line and on each spinner
    /// animation tick). It deliberately does not call `mark_dirty`/`mark_line_dirty`
    /// — the caller is responsible for requesting a redraw — to avoid clearing
    /// unrelated header/sidebar caches on every animation frame.
    pub(crate) fn mark_thinking_run_starts_dirty(&mut self) {
        if let Some(start) = self.thinking_runs.active_start() {
            let revision = self.next_revision();
            if let Some(line) = self.lines.get_mut(start) {
                line.revision = revision;
            }
            // Keep the dirty hint in sync so `ensure_reflow_cache` actually
            // rescans the bumped run-start line (it only scans from
            // `first_dirty_line`).
            self.first_dirty_line = Some(self.first_dirty_line.map_or(start, |d| d.min(start)));
        }
        self.invalidate_transcript_viewport();
    }

    /// Append text to the current or new message line
    ///
    /// This method handles appending text efficiently by reusing the last line if possible
    fn append_text(&mut self, kind: InlineMessageKind, text: &str, style: &InlineTextStyle) {
        if text.is_empty() {
            return;
        }

        if kind == InlineMessageKind::Tool && self.handle_tool_code_fence_marker(text) {
            return;
        }

        let mut appended = false;

        let mut mark_revision = false;
        {
            if let Some(line) = self.lines.last_mut()
                && line.kind == kind
            {
                if let Some(last) = line.segments.last_mut()
                    && &*last.style == style
                {
                    last.text.push_str(text);
                    appended = true;
                    mark_revision = true;
                }
                if !appended {
                    line.segments.push(InlineSegment {
                        text: text.to_owned(),
                        style: std::sync::Arc::new(style.clone()),
                    });
                    appended = true;
                    mark_revision = true;
                }
            }
        }

        if mark_revision {
            let revision = self.next_revision();
            if let Some(line) = self.lines.last_mut()
                && line.kind == kind
            {
                line.revision = revision;
            }
        }

        if appended {
            self.mark_line_dirty(self.lines.len() - 1);
            if !self.is_streaming_final_answer {
                self.invalidate_scroll_metrics();
            }
            return;
        }

        let can_reuse_last = self
            .lines
            .last()
            .map(|line| line.kind == kind && line.segments.is_empty())
            .unwrap_or(false);
        if can_reuse_last {
            let revision = self.next_revision();
            let index = self.lines.len() - 1;
            if let Some(line) = self.lines.last_mut() {
                line.segments.push(InlineSegment {
                    text: text.to_owned(),
                    style: std::sync::Arc::new(style.clone()),
                });
                line.revision = revision;
            }
            self.mark_line_dirty(index);
            if !self.is_streaming_final_answer {
                self.invalidate_scroll_metrics();
            }
            return;
        }

        let revision = self.next_revision();
        let index = self.lines.len();
        self.lines.push(MessageLine {
            kind,
            segments: vec![InlineSegment {
                text: text.to_owned(),
                style: std::sync::Arc::new(style.clone()),
            }],
            link_ranges: Vec::new(),
            revision,
        });

        // Start tracking a new reasoning run when the first `Policy` line
        // is created through `append_text` (bypasses `push_line`'s run
        // tracking). Without this, `mark_thinking_run_starts_dirty` never
        // fires and the reflow cache keeps the stale pre-append output.
        if kind == InlineMessageKind::Policy && self.thinking_runs.active_start().is_none() {
            self.thinking_runs.begin_run(index);
        }

        // Clean up the trailing empty Policy line created by the streaming
        // `\n` suffix in `append_inline`. This covers the `append_inline`
        // → `append_text` path (the most common reasoning→agent transition).
        if kind != InlineMessageKind::Policy
            && let Some(prev_idx) = index.checked_sub(1)
            && self
                .lines
                .get(prev_idx)
                .is_some_and(|l| l.kind == InlineMessageKind::Policy && l.segments.is_empty())
        {
            self.lines.remove(prev_idx);
            self.mark_transcript_line_dirty(prev_idx);
            self.thinking_runs.end_run();
            self.invalidate_scroll_metrics();
            return;
        }

        self.mark_transcript_line_dirty(index);
        self.invalidate_scroll_metrics();
    }

    /// Start a new empty message line
    fn start_line(&mut self, kind: InlineMessageKind) {
        self.push_line(kind, Vec::new());
    }

    /// Reset the current line (clear its segments)
    fn reset_line(&mut self, kind: InlineMessageKind) {
        let mut cleared = false;
        {
            if let Some(line) = self.lines.last_mut()
                && line.kind == kind
            {
                line.segments.clear();
                cleared = true;
            }
        }
        if cleared {
            let revision = self.next_revision();
            let index = self.lines.len() - 1;
            if let Some(line) = self.lines.last_mut()
                && line.kind == kind
            {
                line.revision = revision;
            }
            self.mark_line_dirty(index);
            self.invalidate_scroll_metrics();
            return;
        }
        self.start_line(kind);
    }

    /// Handle tool code fence markers (``` or ~~~)
    ///
    /// Returns true if the text was a code fence marker (and should not be displayed)
    fn handle_tool_code_fence_marker(&mut self, text: &str) -> bool {
        let trimmed = text.trim();
        let stripped = trimmed.strip_prefix("```").or_else(|| trimmed.strip_prefix("~~~"));

        let Some(rest) = stripped else {
            return false;
        };

        // If there's content after the fence marker, it's not a pure fence marker
        if rest.contains("```") || rest.contains("~~~") {
            return false;
        }

        if self.in_tool_code_fence {
            self.in_tool_code_fence = false;
            self.remove_trailing_empty_tool_line();
        } else {
            self.in_tool_code_fence = true;
        }

        true
    }

    /// Remove trailing empty tool lines
    fn remove_trailing_empty_tool_line(&mut self) {
        let should_remove = self
            .lines
            .last()
            .map(|line| line.kind == InlineMessageKind::Tool && line.segments.is_empty())
            .unwrap_or(false);
        if should_remove {
            let index = self.lines.len() - 1;
            self.lines.pop();
            self.mark_line_dirty(index);
        }
    }
}
