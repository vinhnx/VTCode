use anstyle::Color as AnsiColorEnum;
/// Message operations for Session
///
/// This module handles message-related operations including:
/// - Adding and appending messages to the transcript
/// - Managing message lines and segments
/// - Handling tool code fence markers
/// - Message styling and prefixes
use std::cmp::min;

use super::super::types::{InlineMessageKind, InlineSegment, InlineTextStyle};
use super::{Session, message::MessageLine};

const USER_PREFIX: &str = "";

impl Session {
    /// Get the prefix text for a message kind
    #[allow(dead_code)]
    pub(super) fn prefix_text(&self, kind: InlineMessageKind) -> Option<String> {
        match kind {
            InlineMessageKind::User => Some(
                self.labels
                    .user
                    .clone()
                    .unwrap_or_else(|| USER_PREFIX.to_owned()),
            ),
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
    #[allow(dead_code)]
    pub(super) fn prefix_style(&self, line: &MessageLine) -> InlineTextStyle {
        self.styles.prefix_style(line)
    }

    /// Get the text fallback color for a message kind
    pub(super) fn text_fallback(&self, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
        self.styles.text_fallback(kind)
    }

    /// Push a new message line to the transcript
    pub(super) fn push_line(&mut self, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        let previous_max_offset = self.current_max_scroll_offset();
        let revision = self.next_revision();
        let index = self.lines.len();
        self.lines.push(MessageLine {
            kind,
            segments,
            revision,
        });
        self.mark_line_dirty(index);
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);

        // Mark thinking spinner as active after user message (no placeholder line - just state)
        if kind == InlineMessageKind::User {
            self.thinking_spinner.start();
        }
    }

    /// Append a large pasted message as a collapsible placeholder.
    pub(super) fn append_pasted_message(
        &mut self,
        kind: InlineMessageKind,
        text: String,
        _line_count: usize,
    ) {
        self.push_line(
            kind,
            vec![InlineSegment {
                text,
                style: std::sync::Arc::new(InlineTextStyle::default()),
            }],
        );
    }

    /// Append a segment to the transcript, handling newlines and control characters
    pub(super) fn append_inline(&mut self, kind: InlineMessageKind, segment: InlineSegment) {
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
                if let Some((index, control)) = remaining
                    .char_indices()
                    .find(|(_, ch)| matches!(ch, '\n' | '\r'))
                {
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
    pub(super) fn replace_last(
        &mut self,
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
    ) {
        let previous_max_offset = self.current_max_scroll_offset();
        let remove_count = min(count, self.lines.len());
        let first_removed = self.lines.len().saturating_sub(remove_count);
        self.collapsed_pastes
            .retain(|paste| paste.line_index < first_removed);
        let first_dirty = self.lines.len().saturating_sub(remove_count);
        for _ in 0..remove_count {
            self.lines.pop();
        }
        for segments in lines {
            let revision = self.next_revision();
            self.lines.push(MessageLine {
                kind,
                segments,
                revision,
            });
        }
        self.mark_line_dirty(first_dirty);
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
    }

    pub(super) fn expand_collapsed_paste_at_line_index(&mut self, line_index: usize) -> bool {
        if self.collapsed_pastes.is_empty() {
            return false;
        }

        let Some(index) = self
            .collapsed_pastes
            .iter()
            .position(|paste| paste.line_index == line_index)
        else {
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
        line.revision = revision;
        self.mark_line_dirty(collapsed.line_index);
        self.invalidate_scroll_metrics();
        true
    }

    pub(super) fn expand_collapsed_paste_at_row(&mut self, width: u16, row: usize) -> bool {
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
                let height = cache
                    .messages
                    .get(idx)
                    .map(|msg| msg.lines.len())
                    .unwrap_or(1);
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

    /// Append text to the current or new message line
    ///
    /// This method handles appending text efficiently by reusing the last line if possible
    pub(super) fn append_text(
        &mut self,
        kind: InlineMessageKind,
        text: &str,
        style: &InlineTextStyle,
    ) {
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
            self.invalidate_scroll_metrics();
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
            self.invalidate_scroll_metrics();
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
            revision,
        });

        self.mark_line_dirty(index);
        self.invalidate_scroll_metrics();
    }

    /// Start a new empty message line
    pub(super) fn start_line(&mut self, kind: InlineMessageKind) {
        self.push_line(kind, Vec::new());
    }

    /// Reset the current line (clear its segments)
    pub(super) fn reset_line(&mut self, kind: InlineMessageKind) {
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
    pub(super) fn handle_tool_code_fence_marker(&mut self, text: &str) -> bool {
        let trimmed = text.trim();
        let stripped = trimmed
            .strip_prefix("```")
            .or_else(|| trimmed.strip_prefix("~~~"));

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
    pub(super) fn remove_trailing_empty_tool_line(&mut self) {
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
