use ratatui::text::Line;
/// State management and lifecycle operations for Session
///
/// This module handles session state including:
/// - Session initialization and configuration
/// - Exit management
/// - Redraw and dirty state tracking
/// - Screen clearing
/// - Modal management
/// - Timeline pane toggling
/// - Scroll management operations
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

use super::super::types::{
    InlineEvent, InlineListItem, InlineListSearchConfig, InlineListSelection, SecurePromptConfig,
};
use super::{
    Session,
    modal::{ModalListState, ModalSearchState, ModalState},
    render,
};
use crate::config::constants::ui;
use tui_popup::PopupState;

impl Session {
    /// Get the next revision counter for message tracking
    pub(super) fn next_revision(&mut self) -> u64 {
        self.line_revision_counter = self.line_revision_counter.wrapping_add(1);
        self.line_revision_counter
    }

    /// Check if the session should exit
    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Request session exit
    pub fn request_exit(&mut self) {
        self.should_exit = true;
    }

    /// Take the redraw flag and reset it
    ///
    /// Returns true if a redraw was needed
    pub fn take_redraw(&mut self) -> bool {
        if self.needs_redraw {
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    /// Mark the session as needing a redraw
    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    /// Ensure the prompt style has a color set
    pub(super) fn ensure_prompt_style_color(&mut self) {
        if self.prompt_style.color.is_none() {
            self.prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
    }

    /// Clear the screen and reset scroll
    pub(super) fn clear_screen(&mut self) {
        self.lines.clear();
        self.scroll_manager.set_offset(0);
        self.invalidate_transcript_cache();
        self.invalidate_scroll_metrics();
        self.needs_full_clear = true;
        self.mark_dirty();
    }

    /// Toggle the timeline pane visibility
    pub(super) fn toggle_timeline_pane(&mut self) {
        self.show_timeline_pane = !self.show_timeline_pane;
        self.invalidate_scroll_metrics();
        self.mark_dirty();
    }

    /// Show a simple modal dialog
    pub(super) fn show_modal(
        &mut self,
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<SecurePromptConfig>,
    ) {
        let state = ModalState {
            title,
            lines,
            list: None,
            search: None,
            secure_prompt,
            popup_state: PopupState::default(),
            restore_input: true,
            restore_cursor: true,
        };
        if state.secure_prompt.is_none() {
            self.input_enabled = false;
        }
        self.cursor_visible = false;
        self.modal = Some(state);
        self.mark_dirty();
    }

    /// Show a modal with a list of selectable items
    pub(super) fn show_list_modal(
        &mut self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        let mut list_state = ModalListState::new(items, selected.clone());
        let search_state = search.map(ModalSearchState::from);
        if let Some(search) = &search_state {
            list_state.apply_search_with_preference(&search.query, selected);
        }
        let state = ModalState {
            title,
            lines,
            list: Some(list_state),
            search: search_state,
            secure_prompt: None,
            popup_state: PopupState::default(),
            restore_input: true,
            restore_cursor: true,
        };
        self.input_enabled = false;
        self.cursor_visible = false;
        self.modal = Some(state);
        self.mark_dirty();
    }

    /// Close the currently open modal
    pub(super) fn close_modal(&mut self) {
        if let Some(state) = self.modal.take() {
            self.input_enabled = true;
            self.cursor_visible = true;
            if state.secure_prompt.is_some() {
                // Secure prompt modal closed, don't restore input
            }
            self.mark_dirty();
        }
    }

    /// Scroll operations
    pub(super) fn scroll_line_up(&mut self) {
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_up(1);
        if self.scroll_manager.offset() != previous_offset {
            self.visible_lines_cache = None;
        }
    }

    pub(super) fn scroll_line_down(&mut self) {
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_down(1);
        if self.scroll_manager.offset() != previous_offset {
            self.visible_lines_cache = None;
        }
    }

    pub(super) fn scroll_page_up(&mut self) {
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_up(self.viewport_height().max(1));
        if self.scroll_manager.offset() != previous_offset {
            self.visible_lines_cache = None;
        }
    }

    pub(super) fn scroll_page_down(&mut self) {
        let page = self.viewport_height().max(1);
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_down(page);
        if self.scroll_manager.offset() != previous_offset {
            self.visible_lines_cache = None;
        }
    }

    pub(super) fn viewport_height(&self) -> usize {
        self.transcript_rows.max(1) as usize
    }

    /// Invalidate scroll metrics to force recalculation
    pub(super) fn invalidate_scroll_metrics(&mut self) {
        self.scroll_manager.invalidate_metrics();
        self.invalidate_transcript_cache();
    }

    /// Invalidate the transcript cache
    pub(super) fn invalidate_transcript_cache(&mut self) {
        self.transcript_cache = None;
        self.visible_lines_cache = None;
        self.transcript_content_changed = true;
    }

    /// Get the current maximum scroll offset
    pub(super) fn current_max_scroll_offset(&mut self) -> usize {
        self.ensure_scroll_metrics();
        self.scroll_manager.max_offset()
    }

    /// Enforce scroll bounds after viewport changes
    pub(super) fn enforce_scroll_bounds(&mut self) {
        let max_offset = self.current_max_scroll_offset();
        if self.scroll_manager.offset() > max_offset {
            self.scroll_manager.set_offset(max_offset);
        }
    }

    /// Ensure scroll metrics are up to date
    pub(super) fn ensure_scroll_metrics(&mut self) {
        if self.scroll_manager.metrics_valid() {
            return;
        }

        let viewport_rows = self.viewport_height();
        if self.transcript_width == 0 || viewport_rows == 0 {
            self.scroll_manager.set_total_rows(0);
            return;
        }

        let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
        let effective_padding = padding.min(viewport_rows.saturating_sub(1));
        let total_rows =
            render::total_transcript_rows(self, self.transcript_width) + effective_padding;
        self.scroll_manager.set_total_rows(total_rows);
    }

    /// Prepare transcript scroll parameters
    pub(super) fn prepare_transcript_scroll(
        &mut self,
        total_rows: usize,
        viewport_rows: usize,
    ) -> (usize, usize) {
        let viewport = viewport_rows.max(1);
        let clamped_total = total_rows.max(1);
        self.scroll_manager.set_total_rows(clamped_total);
        self.scroll_manager.set_viewport_rows(viewport as u16);
        let max_offset = self.scroll_manager.max_offset();

        if self.scroll_manager.offset() > max_offset {
            self.scroll_manager.set_offset(max_offset);
        }

        let top_offset = max_offset.saturating_sub(self.scroll_manager.offset());
        (top_offset, clamped_total)
    }

    /// Adjust scroll position after content changes
    pub(super) fn adjust_scroll_after_change(&mut self, previous_max_offset: usize) {
        use std::cmp::min;

        let new_max_offset = self.current_max_scroll_offset();
        let current_offset = self.scroll_manager.offset();

        if current_offset >= previous_max_offset && new_max_offset > previous_max_offset {
            self.scroll_manager.set_offset(new_max_offset);
        } else if current_offset > 0 && new_max_offset > previous_max_offset {
            let delta = new_max_offset - previous_max_offset;
            self.scroll_manager
                .set_offset(min(current_offset + delta, new_max_offset));
        }
        self.enforce_scroll_bounds();
    }

    /// Emit an inline event through the channel and callback
    #[inline]
    pub(super) fn emit_inline_event(
        &self,
        event: &InlineEvent,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        if let Some(cb) = callback {
            cb(event);
        }
        let _ = events.send(event.clone());
    }

    /// Handle scroll down event
    #[inline]
    #[allow(dead_code)]
    pub(super) fn handle_scroll_down(
        &mut self,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        self.scroll_line_down();
        self.mark_dirty();
        self.emit_inline_event(&InlineEvent::ScrollLineDown, events, callback);
    }

    /// Handle scroll up event
    #[inline]
    #[allow(dead_code)]
    pub(super) fn handle_scroll_up(
        &mut self,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        self.scroll_line_up();
        self.mark_dirty();
        self.emit_inline_event(&InlineEvent::ScrollLineUp, events, callback);
    }

    /// Collect transcript window with caching
    pub(super) fn collect_transcript_window_cached(
        &mut self,
        width: u16,
        start_row: usize,
        max_rows: usize,
    ) -> Vec<Line<'static>> {
        // Check cache
        if let Some((cached_offset, cached_width, cached_lines)) = &self.visible_lines_cache
            && *cached_offset == start_row
            && *cached_width == width
        {
            return (**cached_lines).clone();
        }

        // Not in cache, fetch from transcript
        let visible_lines = self.collect_transcript_window(width, start_row, max_rows);

        // Cache for next render
        self.visible_lines_cache = Some((start_row, width, Arc::new(visible_lines.clone())));

        visible_lines
    }

    /// Collect transcript window without caching
    pub(super) fn collect_transcript_window(
        &mut self,
        width: u16,
        start_row: usize,
        max_rows: usize,
    ) -> Vec<Line<'static>> {
        if max_rows == 0 {
            return Vec::new();
        }
        let cache = render::ensure_reflow_cache(self, width);
        cache.get_visible_range(start_row, max_rows)
    }
}
