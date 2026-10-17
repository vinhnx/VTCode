//! UI-only prompt ownership, geometry, and navigation for the normal transcript.
//!
//! Inspired by OpenAI Codex's prompt-header and anchored-navigation pattern:
//! codex-rs/tui/src/transcript_view.rs at 7219fd735bef2f9cfd0363fecdbbb212e3df5255.
//! <https://github.com/openai/codex> (Apache-2.0, Copyright 2025 OpenAI).
//! Original VT Code implementation using retained message indices and reflow rows.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{layout::Rect, text::Line};

use super::{MouseDragTarget, Session};
use crate::tui::config::constants::ui;
use crate::tui::core_tui::types::InlineMessageKind;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug)]
pub(super) struct StickyPromptTarget {
    area: Rect,
    message_index: usize,
}

#[derive(PartialEq, Eq)]
struct StickyPromptPreviewKey {
    message_index: usize,
    line_count: usize,
    revision: u64,
    width: u16,
    prefix: String,
}

pub(super) struct StickyPromptPreviewCache {
    key: StickyPromptPreviewKey,
    preview: Line<'static>,
}

pub(crate) struct StickyPromptLayout {
    pub(crate) body: Rect,
    pub(crate) source_row: usize,
    pub(crate) header: Option<Line<'static>>,
}

impl Session {
    pub(crate) fn clear_sticky_prompt_target(&mut self) {
        self.sticky_prompt_target = None;
    }

    /// The last retained user group before the first visible reflowed content.
    /// Empty grouped entries must not steal ownership from their rendered head.
    fn owning_prompt(&mut self, width: u16, source_row: usize) -> Option<usize> {
        let cache = self.ensure_reflow_cache(width);
        if source_row >= cache.total_rows {
            return None;
        }
        let end = cache.row_offsets.partition_point(|row| *row <= source_row);
        let visible_index = (0..end).rev().find(|index| !cache.messages[*index].lines.is_empty())?;
        let mut prompt_index = (0..=visible_index)
            .rev()
            .find(|index| self.lines[*index].kind == InlineMessageKind::User)?;
        while prompt_index > 0 && self.lines[prompt_index - 1].kind == InlineMessageKind::User {
            prompt_index -= 1;
        }
        if prompt_index == 0 && self.leading_user_prompt_truncated {
            return None;
        }
        let prompt_row = self.ensure_reflow_cache(width).row_offsets[prompt_index];
        (prompt_row < source_row).then_some(prompt_index)
    }

    fn sticky_prompt_preview(&mut self, index: usize, width: u16) -> Line<'static> {
        let group = self.lines[index..]
            .iter()
            .take_while(|line| line.kind == InlineMessageKind::User);
        let (line_count, revision) =
            group.fold((0, 0), |(count, revision), line| (count + 1, revision.max(line.revision)));
        // Message revisions are session-wide: changes to this group get a newer
        // revision, while streaming another message leaves this key unchanged.
        let key = StickyPromptPreviewKey {
            message_index: index,
            line_count,
            revision,
            width,
            prefix: self.prefix_text(InlineMessageKind::User).unwrap_or_default(),
        };
        if let Some(cache) = &self.sticky_prompt_preview_cache
            && cache.key == key
        {
            return cache.preview.clone();
        }
        let preview = self.build_sticky_prompt_preview(index, width, key.prefix.clone());
        self.sticky_prompt_preview_cache = Some(StickyPromptPreviewCache { key, preview: preview.clone() });
        preview
    }

    fn build_sticky_prompt_preview(&self, index: usize, width: u16, prefix: String) -> Line<'static> {
        let mut text = prefix;
        // Segments within one line are contiguous; consecutive User lines are
        // one multiline prompt, just as in the transcript renderer.
        for (line_offset, line) in self.lines[index..]
            .iter()
            .take_while(|line| line.kind == InlineMessageKind::User)
            .enumerate()
        {
            if line_offset > 0 {
                text.push(' ');
            }
            for segment in &line.segments {
                text.push_str(&super::text_utils::strip_ansi_codes(&segment.text));
            }
        }
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let max_width = usize::from(width);
        if UnicodeWidthStr::width(collapsed.as_str()) <= max_width {
            return Line::from(collapsed);
        }
        // Keep wide and joined graphemes intact while reserving one cell for
        // the shared ellipsis. A character-wise cut can split emoji sequences.
        let mut preview = String::new();
        let mut used_width = 0;
        for grapheme in collapsed.graphemes(true) {
            let grapheme_width = UnicodeWidthStr::width(grapheme);
            if used_width + grapheme_width > max_width.saturating_sub(1) {
                break;
            }
            preview.push_str(grapheme);
            used_width += grapheme_width;
        }
        preview.push(crate::design::constants::ELLIPSIS_CHAR);
        Line::from(preview)
    }

    fn set_prompt_body(&mut self, body: Rect) -> usize {
        self.set_transcript_area(Some(body));
        self.apply_transcript_rows(body.height);
        let rows = usize::from(body.height);
        let total_rows = self.total_transcript_rows(body.width) + ui::effective_transcript_bottom_padding(rows);
        self.prepare_transcript_scroll(total_rows, rows).0
    }

    /// Resolve from the full viewport on every frame. Reading preserves its
    /// message anchor; following recomputes its top after reserving the row.
    /// If that exposes the next prompt, yield the header and restore the full
    /// viewport. Resolving from full geometry keeps that decision stable.
    pub(crate) fn layout_sticky_prompt(&mut self, area: Rect) -> StickyPromptLayout {
        let previous_body = self.transcript_area();
        self.sticky_prompt_target = None;
        let full = Rect::new(
            area.x,
            area.y,
            area.width.min(ui::TUI_MAX_VIEWPORT_WIDTH),
            area.height.min(ui::TUI_MAX_VIEWPORT_HEIGHT),
        );
        self.apply_transcript_width(full.width);
        self.ensure_scroll_metrics();
        let reading = self.scroll_manager.offset() != 0;
        let total_rows = self.total_transcript_rows(full.width);
        let bottom_top = |height: u16| {
            let rows = usize::from(height);
            (total_rows + ui::effective_transcript_bottom_padding(rows)).saturating_sub(rows)
        };
        let full_top = if reading {
            self.scroll_manager.max_offset().saturating_sub(self.scroll_manager.offset())
        } else {
            bottom_top(full.height)
        };
        // Compute the prospective body before mutating metrics. Switching
        // full/body heights on every frame would discard the visible-row cache
        // and could clamp a reading anchor to the live bottom temporarily.
        let mut body = full;
        if full.height >= 4 && self.owning_prompt(full.width, full_top).is_some() {
            let body_top = if reading { full_top } else { bottom_top(full.height - 1) };
            if self.owning_prompt(full.width, body_top).is_some() {
                body = Rect::new(full.x, full.y + 1, full.width, full.height - 1);
            }
        }
        let source_row = self.set_prompt_body(body);
        let mut layout = StickyPromptLayout { body, source_row, header: None };
        if body != full {
            if let Some(index) = self.owning_prompt(body.width, source_row) {
                layout.header = Some(self.sticky_prompt_preview(index, body.width));
                self.sticky_prompt_target = Some(StickyPromptTarget {
                    area: Rect::new(full.x, full.y, full.width, 1),
                    message_index: index,
                });
            } else {
                layout.body = full;
                layout.source_row = self.set_prompt_body(full);
            }
        }
        self.transcript_view_top = layout.source_row;
        if let Some(previous) = previous_body
            && previous.y != layout.body.y
        {
            self.mouse_selection
                .adjust_for_scroll(i32::from(layout.body.y) - i32::from(previous.y));
        }
        layout
    }

    /// Shared by the core and application mouse paths. Only an unmodified
    /// left press can navigate; overlays retain ownership of their input.
    pub(crate) fn handle_sticky_prompt_click(&mut self, event: MouseEvent) -> bool {
        if event.kind != MouseEventKind::Down(MouseButton::Left)
            || event.modifiers != KeyModifiers::empty()
            || self.has_active_overlay()
        {
            return false;
        }
        let Some(target) = self.sticky_prompt_target else {
            return false;
        };
        if !target.area.contains((event.column, event.row).into()) {
            return false;
        }
        let Some((source_row, _)) = self.transcript_message_row_range(self.transcript_width, target.message_index)
        else {
            return false;
        };
        // The destination has no header: use the restored full height when
        // computing its bottom distance, so the next render preserves the jump.
        if let Some(body) = self.transcript_area() {
            let full = Rect::new(body.x, target.area.y, body.width, body.height + 1);
            self.set_prompt_body(full);
        }
        self.ensure_scroll_metrics();
        self.scroll_manager
            .set_offset(self.scroll_manager.max_offset().saturating_sub(source_row));
        self.user_scrolled = self.scroll_manager.offset() != 0;
        self.mouse_selection.clear();
        self.mouse_drag_target = MouseDragTarget::None;
        self.cancel_drag_auto_scroll();
        self.clear_pending_link_click();
        self.clear_transcript_file_link_hover();
        self.invalidate_transcript_viewport();
        self.mark_scrolling();
        self.mark_dirty();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::core_tui::types::{InlineSegment, InlineTextStyle, InlineTheme};
    use std::sync::Arc;

    #[test]
    fn sticky_prompt_preview_reuses_large_prompt_during_streaming_and_refreshes_changes() {
        let make_segment = |text: &str| InlineSegment {
            text: text.to_owned(),
            style: Arc::new(InlineTextStyle::default()),
        };
        let mut session = Session::new(InlineTheme::default(), None, 16);
        session.push_line(InlineMessageKind::User, vec![make_segment(&"large prompt ".repeat(80_000))]);
        let original = session.sticky_prompt_preview(0, 20);
        let cached_text = session.sticky_prompt_preview_cache.as_ref().unwrap().preview.spans[0]
            .content
            .as_ptr();
        for _ in 0..3 {
            session.push_line(InlineMessageKind::Agent, vec![make_segment("streaming output")]);
            assert_eq!(session.sticky_prompt_preview(0, 20), original);
            assert_eq!(
                session.sticky_prompt_preview_cache.as_ref().unwrap().preview.spans[0]
                    .content
                    .as_ptr(),
                cached_text,
                "streaming and unchanged frames retain the preview allocation"
            );
        }
        assert_ne!(session.sticky_prompt_preview(0, 10), original, "resize refreshes the preview");
        session.replace_last(
            session.lines.len(),
            InlineMessageKind::User,
            vec![vec![make_segment("new prompt")]],
            None,
        );
        assert_eq!(session.sticky_prompt_preview(0, 20), Line::from("new prompt"));
        session.push_line(InlineMessageKind::User, vec![make_segment("continuation")]);
        assert_eq!(session.sticky_prompt_preview(0, 40), Line::from("new prompt continuation"));
    }
}
