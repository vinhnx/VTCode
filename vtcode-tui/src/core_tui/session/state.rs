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
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use super::super::types::{
    DiffOverlayRequest, DiffPreviewState, InlineEvent, ListOverlayRequest, ModalOverlayRequest,
    OverlayRequest, WizardOverlayRequest,
};
use super::status_requires_shimmer;
use super::{
    ActiveOverlay, Session,
    modal::{ModalListState, ModalSearchState, ModalState, WizardModalState},
};
use crate::config::constants::ui;

impl Session {
    pub(crate) fn inline_lists_visible(&self) -> bool {
        self.inline_lists_visible
    }

    pub(crate) fn ensure_inline_lists_visible_for_trigger(&mut self) {
        if self.inline_lists_visible {
            return;
        }
        self.inline_lists_visible = true;
        self.mark_dirty();
    }

    pub(crate) fn toggle_inline_lists_visibility(&mut self) {
        self.inline_lists_visible = !self.inline_lists_visible;
        if !self.inline_lists_visible {
            if self.file_palette_active {
                self.close_file_palette();
            }
            if self.history_picker_state.active {
                self.history_picker_state.cancel(&mut self.input_manager);
            }
        }
        self.mark_dirty();
    }

    /// Get the next revision counter for message tracking
    pub(crate) fn next_revision(&mut self) -> u64 {
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
        self.header_lines_cache = None;
        self.queued_inputs_preview_cache = None;
    }

    /// Invalidate only the header cache (e.g. when provider/model changes)
    pub fn invalidate_header_cache(&mut self) {
        self.header_lines_cache = None;
        self.header_height_cache.clear();
        self.mark_dirty();
    }

    /// Invalidate only the sidebar cache (e.g. when queue changes)
    pub fn invalidate_sidebar_cache(&mut self) {
        self.queued_inputs_preview_cache = None;
        self.mark_dirty();
    }

    pub(crate) fn set_transcript_area(&mut self, area: Option<Rect>) {
        self.transcript_area = area;
    }

    pub(crate) fn set_input_area(&mut self, area: Option<Rect>) {
        self.input_area = area;
    }

    /// Advance animation state on tick and request redraw when a frame changes.
    pub fn handle_tick(&mut self) {
        let motion_reduced = self.appearance.motion_reduced();
        let mut animation_updated = false;
        if !motion_reduced && self.thinking_spinner.is_active && self.thinking_spinner.update() {
            animation_updated = true;
        }
        let shimmer_active = if self.appearance.should_animate_progress_status() {
            self.is_shimmer_active()
        } else {
            false
        };
        if shimmer_active && self.shimmer_state.update() {
            animation_updated = true;
        }
        if let Some(until) = self.scroll_cursor_steady_until
            && Instant::now() >= until
        {
            self.scroll_cursor_steady_until = None;
            self.needs_redraw = true;
        }
        if self.last_shimmer_active && !shimmer_active {
            self.needs_redraw = true;
        }
        self.last_shimmer_active = shimmer_active;
        if animation_updated {
            self.needs_redraw = true;
        }
    }

    pub(crate) fn is_running_activity(&self) -> bool {
        let left = self.input_status_left.as_deref().unwrap_or("");
        let running_status = self.appearance.should_animate_progress_status()
            && (left.contains("Running command:")
                || left.contains("Running tool:")
                || left.contains("Running:")
                || status_requires_shimmer(left));
        let active_pty = self
            .active_pty_sessions
            .as_ref()
            .map(|counter| counter.load(Ordering::Relaxed) > 0)
            .unwrap_or(false);
        running_status || active_pty
    }

    pub(crate) fn has_status_spinner(&self) -> bool {
        if !self.appearance.should_animate_progress_status() {
            return false;
        }
        let Some(left) = self.input_status_left.as_deref() else {
            return false;
        };
        status_requires_shimmer(left)
    }

    pub(super) fn is_shimmer_active(&self) -> bool {
        self.has_status_spinner()
    }

    pub(crate) fn use_steady_cursor(&self) -> bool {
        if !self.appearance.should_animate_progress_status() {
            self.scroll_cursor_steady_until.is_some()
        } else {
            self.is_shimmer_active() || self.scroll_cursor_steady_until.is_some()
        }
    }

    pub(super) fn mark_scrolling(&mut self) {
        let steady_duration = Duration::from_millis(ui::TUI_SCROLL_CURSOR_STEADY_MS);
        if steady_duration.is_zero() {
            self.scroll_cursor_steady_until = None;
        } else {
            self.scroll_cursor_steady_until = Some(Instant::now() + steady_duration);
        }
    }

    /// Mark a specific line as dirty to optimize reflow scans
    pub(crate) fn mark_line_dirty(&mut self, index: usize) {
        self.first_dirty_line = match self.first_dirty_line {
            Some(current) => Some(current.min(index)),
            None => Some(index),
        };
        self.mark_dirty();
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
        self.collapsed_pastes.clear();
        self.user_scrolled = false;
        self.scroll_manager.set_offset(0);
        self.invalidate_transcript_cache();
        self.invalidate_scroll_metrics();
        self.needs_full_clear = true;
        self.mark_dirty();
    }

    /// Toggle logs panel visibility
    pub(super) fn toggle_logs(&mut self) {
        self.show_logs = !self.show_logs;
        self.invalidate_scroll_metrics();
        self.mark_dirty();
    }

    /// Show a simple modal dialog
    pub(super) fn show_modal(
        &mut self,
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<super::super::types::SecurePromptConfig>,
    ) {
        self.show_overlay(OverlayRequest::Modal(ModalOverlayRequest {
            title,
            lines,
            secure_prompt,
        }));
    }

    pub(super) fn show_overlay(&mut self, request: OverlayRequest) {
        if self.has_active_overlay() {
            self.overlay_queue.push_back(request);
            self.mark_dirty();
            return;
        }
        self.activate_overlay(request);
    }

    pub(crate) fn has_active_overlay(&self) -> bool {
        self.active_overlay.is_some()
    }

    pub(crate) fn modal_state(&self) -> Option<&ModalState> {
        self.active_overlay
            .as_ref()
            .and_then(ActiveOverlay::as_modal)
    }

    pub(crate) fn modal_state_mut(&mut self) -> Option<&mut ModalState> {
        self.active_overlay
            .as_mut()
            .and_then(ActiveOverlay::as_modal_mut)
    }

    pub(crate) fn wizard_overlay(&self) -> Option<&WizardModalState> {
        self.active_overlay
            .as_ref()
            .and_then(ActiveOverlay::as_wizard)
    }

    pub(crate) fn wizard_overlay_mut(&mut self) -> Option<&mut WizardModalState> {
        self.active_overlay
            .as_mut()
            .and_then(ActiveOverlay::as_wizard_mut)
    }

    pub(crate) fn diff_preview_state(&self) -> Option<&DiffPreviewState> {
        self.active_overlay
            .as_ref()
            .and_then(ActiveOverlay::as_diff)
    }

    pub(crate) fn diff_preview_state_mut(&mut self) -> Option<&mut DiffPreviewState> {
        self.active_overlay
            .as_mut()
            .and_then(ActiveOverlay::as_diff_mut)
    }

    pub(crate) fn modal_overlay_active(&self) -> bool {
        self.active_overlay
            .as_ref()
            .is_some_and(ActiveOverlay::is_modal_like)
    }

    pub(crate) fn take_modal_state(&mut self) -> Option<ModalState> {
        if !self
            .active_overlay
            .as_ref()
            .is_some_and(|overlay| matches!(overlay, ActiveOverlay::Modal(_)))
        {
            return None;
        }

        match self.active_overlay.take() {
            Some(ActiveOverlay::Modal(state)) => Some(*state),
            Some(ActiveOverlay::Wizard(_) | ActiveOverlay::Diff(_)) | None => None,
        }
    }

    fn activate_overlay(&mut self, request: OverlayRequest) {
        match request {
            OverlayRequest::Modal(request) => self.activate_modal_overlay(request),
            OverlayRequest::List(request) => self.activate_list_overlay(request),
            OverlayRequest::Wizard(request) => self.activate_wizard_overlay(request),
            OverlayRequest::Diff(request) => self.activate_diff_overlay(request),
        }
    }

    pub(super) fn close_overlay(&mut self) {
        let Some(state) = self.active_overlay.take() else {
            return;
        };

        self.input_enabled = state.restore_input();
        self.cursor_visible = state.restore_cursor();

        if let Some(next_request) = self.overlay_queue.pop_front() {
            self.activate_overlay(next_request);
            return;
        }

        self.mark_dirty();
    }

    fn activate_modal_overlay(&mut self, request: ModalOverlayRequest) {
        let state = ModalState {
            title: request.title,
            lines: request.lines,
            footer_hint: None,
            hotkeys: Vec::new(),
            list: None,
            search: None,
            secure_prompt: request.secure_prompt,
            restore_input: true,
            restore_cursor: true,
        };
        if state.secure_prompt.is_none() {
            self.input_enabled = false;
        }
        self.cursor_visible = false;
        self.active_overlay = Some(ActiveOverlay::Modal(Box::new(state)));
        self.mark_dirty();
    }

    fn activate_list_overlay(&mut self, request: ListOverlayRequest) {
        self.ensure_inline_lists_visible_for_trigger();
        let mut list_state = ModalListState::new(request.items, request.selected.clone());
        let search_state = request.search.map(ModalSearchState::from);
        if let Some(search) = &search_state {
            list_state.apply_search_with_preference(&search.query, request.selected);
        }
        let state = ModalState {
            title: request.title,
            lines: request.lines,
            footer_hint: request.footer_hint,
            hotkeys: request.hotkeys,
            list: Some(list_state),
            search: search_state,
            secure_prompt: None,
            restore_input: true,
            restore_cursor: true,
        };
        self.input_enabled = false;
        self.cursor_visible = false;
        self.active_overlay = Some(ActiveOverlay::Modal(Box::new(state)));
        self.mark_dirty();
    }

    fn activate_wizard_overlay(&mut self, request: WizardOverlayRequest) {
        self.ensure_inline_lists_visible_for_trigger();
        let wizard = WizardModalState::new(
            request.title,
            request.steps,
            request.current_step,
            request.search,
            request.mode,
        );
        self.active_overlay = Some(ActiveOverlay::Wizard(Box::new(wizard)));
        self.input_enabled = false;
        self.cursor_visible = false;
        self.mark_dirty();
    }

    fn activate_diff_overlay(&mut self, request: DiffOverlayRequest) {
        let mut state = DiffPreviewState::new_with_mode(
            request.file_path,
            request.before,
            request.after,
            request.hunks,
            request.mode,
        );
        state.current_hunk = request.current_hunk;
        self.active_overlay = Some(ActiveOverlay::Diff(Box::new(state)));
        self.input_enabled = false;
        self.cursor_visible = false;
        self.mark_dirty();
    }

    /// Scroll operations
    ///
    /// Note: The scroll offset model is inverted for chat-style display:
    /// offset=0 shows the bottom (newest content), offset=max shows the top.
    /// Therefore "scroll up" (show older content) increases the offset, and
    /// "scroll down" (show newer content) decreases it.
    pub fn scroll_line_up(&mut self) {
        self.mark_scrolling();
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_down(1);
        if self.scroll_manager.offset() != previous_offset {
            self.user_scrolled = self.scroll_manager.offset() != 0;
            self.visible_lines_cache = None;
        }
    }

    pub fn scroll_line_down(&mut self) {
        self.mark_scrolling();
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_up(1);
        if self.scroll_manager.offset() != previous_offset {
            self.user_scrolled = self.scroll_manager.offset() != 0;
            self.visible_lines_cache = None;
        }
    }

    pub(super) fn scroll_page_up(&mut self) {
        self.mark_scrolling();
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager
            .scroll_down(self.viewport_height().max(1));
        if self.scroll_manager.offset() != previous_offset {
            self.user_scrolled = self.scroll_manager.offset() != 0;
            self.visible_lines_cache = None;
        }
    }

    pub(super) fn scroll_page_down(&mut self) {
        self.mark_scrolling();
        let page = self.viewport_height().max(1);
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_up(page);
        if self.scroll_manager.offset() != previous_offset {
            self.user_scrolled = self.scroll_manager.offset() != 0;
            self.visible_lines_cache = None;
        }
    }

    pub(crate) fn viewport_height(&self) -> usize {
        self.transcript_rows.max(1) as usize
    }

    /// Apply coalesced scroll from accumulated scroll events
    /// This is more efficient than calling scroll_line_up/down multiple times
    pub(crate) fn apply_coalesced_scroll(&mut self, line_delta: i32, page_delta: i32) {
        self.mark_scrolling();
        let previous_offset = self.scroll_manager.offset();

        // Apply page scroll first (larger movements)
        // Inverted offset model: positive delta = scroll down visually = decrease offset
        if page_delta != 0 {
            let page_size = self.viewport_height().max(1);
            if page_delta > 0 {
                self.scroll_manager
                    .scroll_up(page_size * page_delta.unsigned_abs() as usize);
            } else {
                self.scroll_manager
                    .scroll_down(page_size * page_delta.unsigned_abs() as usize);
            }
        }

        // Then apply line scroll
        if line_delta != 0 {
            if line_delta > 0 {
                self.scroll_manager
                    .scroll_up(line_delta.unsigned_abs() as usize);
            } else {
                self.scroll_manager
                    .scroll_down(line_delta.unsigned_abs() as usize);
            }
        }

        // Invalidate visible lines cache if offset actually changed
        if self.scroll_manager.offset() != previous_offset {
            self.visible_lines_cache = None;
        }
    }

    /// Invalidate scroll metrics to force recalculation
    pub(super) fn invalidate_scroll_metrics(&mut self) {
        self.scroll_manager.invalidate_metrics();
        self.invalidate_transcript_cache();
    }

    /// Invalidate the transcript cache
    pub(super) fn invalidate_transcript_cache(&mut self) {
        if let Some(cache) = self.transcript_cache.as_mut() {
            cache.invalidate_content();
        }
        self.visible_lines_cache = None;
        self.transcript_content_changed = true;

        // If no specific line was marked dirty, assume everything needs reflow
        if self.first_dirty_line.is_none() {
            self.first_dirty_line = Some(0);
        }
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
        let total_rows = self.total_transcript_rows(self.transcript_width) + effective_padding;
        self.scroll_manager.set_total_rows(total_rows);
    }

    /// Prepare transcript scroll parameters
    pub(crate) fn prepare_transcript_scroll(
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
}
