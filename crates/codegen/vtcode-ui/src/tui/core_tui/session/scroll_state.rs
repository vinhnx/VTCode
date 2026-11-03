use super::*;

impl Session {
    /// Scroll operations
    ///
    /// Note: The scroll offset model is inverted for chat-style display:
    /// offset=0 shows the bottom (newest content), offset=max shows the top.
    /// Therefore "scroll up" (show older content) increases the offset, and
    /// "scroll down" (show newer content) decreases it.
    pub(crate) fn scroll_line_up(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_down(1);
        if self.scroll_manager.offset() != previous_offset {
            self.sync_follow_state();
            self.visible_lines_cache = None;
            self.sticky_prompt_target = None;
            // Content moves down on screen; shift selection to match.
            self.mouse_selection.adjust_for_scroll(1);
        }
    }

    pub(crate) fn scroll_line_down(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_up(1);
        if self.scroll_manager.offset() != previous_offset {
            self.sync_follow_state();
            self.visible_lines_cache = None;
            self.sticky_prompt_target = None;
            // Content moves up on screen; shift selection to match.
            self.mouse_selection.adjust_for_scroll(-1);
        }
    }

    pub(crate) fn scroll_page_up(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        let previous_offset = self.scroll_manager.offset();
        let page = self.viewport_height().max(1);
        self.scroll_manager.scroll_down(page);
        if self.scroll_manager.offset() != previous_offset {
            let actual_delta = self.scroll_manager.offset() - previous_offset;
            self.sync_follow_state();
            self.visible_lines_cache = None;
            self.sticky_prompt_target = None;
            self.mouse_selection.adjust_for_scroll(actual_delta as i32);
        }
    }

    pub(crate) fn scroll_page_down(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        let page = self.viewport_height().max(1);
        let previous_offset = self.scroll_manager.offset();
        self.scroll_manager.scroll_up(page);
        if self.scroll_manager.offset() != previous_offset {
            let actual_delta = previous_offset - self.scroll_manager.offset();
            self.sync_follow_state();
            self.visible_lines_cache = None;
            self.sticky_prompt_target = None;
            self.mouse_selection.adjust_for_scroll(-(actual_delta as i32));
        }
    }

    pub(crate) fn viewport_height(&self) -> usize {
        self.transcript_rows.max(1) as usize
    }

    /// Apply coalesced scroll from accumulated scroll events
    /// This is more efficient than calling scroll_line_up/down multiple times
    pub(crate) fn apply_coalesced_scroll(&mut self, line_delta: i32, page_delta: i32) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        let previous_offset = self.scroll_manager.offset();

        // Apply page scroll first (larger movements)
        // Inverted offset model: positive delta = scroll down visually = decrease offset
        if page_delta != 0 {
            let page_size = self.viewport_height().max(1);
            if page_delta > 0 {
                self.scroll_manager.scroll_up(page_size * page_delta.unsigned_abs() as usize);
            } else {
                self.scroll_manager.scroll_down(page_size * page_delta.unsigned_abs() as usize);
            }
        }

        // Then apply line scroll
        if line_delta != 0 {
            if line_delta > 0 {
                self.scroll_manager.scroll_up(line_delta.unsigned_abs() as usize);
            } else {
                self.scroll_manager.scroll_down(line_delta.unsigned_abs() as usize);
            }
        }

        // Invalidate visible lines cache if offset actually changed
        if self.scroll_manager.offset() != previous_offset {
            self.sync_follow_state();
            self.invalidate_transcript_viewport();
            // Compute actual row delta for selection adjustment.
            // Inverted model: increasing offset → content moves down → positive row delta.
            let offset_delta = self.scroll_manager.offset() as i64 - previous_offset as i64;
            self.mouse_selection.adjust_for_scroll(offset_delta as i32);
        }
    }

    /// Invalidate scroll metrics to force recalculation
    pub(crate) fn invalidate_scroll_metrics(&mut self) {
        self.scroll_manager.invalidate_metrics();
        self.invalidate_transcript_viewport();
    }

    /// Invalidate the transcript cache
    pub(crate) fn invalidate_transcript_cache(&mut self) {
        self.transcript_presentation_revision = self.transcript_presentation_revision.wrapping_add(1);
        let had_cache = if let Some(cache) = self.transcript_cache.as_mut() {
            cache.invalidate_content();
            true
        } else {
            false
        };
        self.invalidate_transcript_viewport();
        self.request_transcript_clear();

        if had_cache || self.first_dirty_line.is_none() {
            self.first_dirty_line = Some(0);
        }
    }

    /// Get the current maximum scroll offset
    pub(crate) fn current_max_scroll_offset(&mut self) -> usize {
        self.ensure_scroll_metrics();
        self.scroll_manager.max_offset()
    }

    /// Enforce scroll bounds after viewport changes.
    /// Clamps against last-known metrics only — never forces a reflow. The next
    /// `ensure_scroll_metrics` (on render) recomputes and clamps again.
    pub(crate) fn enforce_scroll_bounds(&mut self) {
        if !self.scroll_manager.metrics_valid() {
            return;
        }
        let max_offset = self.scroll_manager.max_offset();
        if self.scroll_manager.offset() > max_offset {
            self.scroll_manager.set_offset(max_offset);
        }
    }

    /// Ensure scroll metrics are up to date
    pub(crate) fn ensure_scroll_metrics(&mut self) {
        if self.scroll_manager.metrics_valid() {
            return;
        }

        let viewport_rows = self.viewport_height();
        if self.transcript_width == 0 || viewport_rows == 0 {
            self.scroll_manager.set_viewport_rows(viewport_rows.max(1) as u16);
            self.scroll_manager.set_total_rows(0);
            return;
        }

        let effective_padding = ui::effective_transcript_bottom_padding(viewport_rows);
        let total_rows = self.total_transcript_rows(self.transcript_width) + effective_padding;
        self.scroll_manager.set_viewport_rows(viewport_rows as u16);
        self.scroll_manager.set_total_rows(total_rows);
        self.scroll_manager.clamp_offset();
    }

    /// Prepare transcript scroll parameters
    pub(crate) fn prepare_transcript_scroll(&mut self, total_rows: usize, viewport_rows: usize) -> (usize, usize) {
        let viewport = viewport_rows.max(1);
        let clamped_total = total_rows.max(1);
        self.scroll_manager.set_viewport_rows(viewport as u16);
        self.scroll_manager.set_total_rows(clamped_total);
        let max_offset = self.scroll_manager.max_offset();

        if self.scroll_manager.offset() > max_offset {
            self.scroll_manager.set_offset(max_offset);
        }

        let top_offset = max_offset.saturating_sub(self.scroll_manager.offset());
        (top_offset, clamped_total)
    }

    /// Recompute follow state from the live offset after any scroll move.
    ///
    /// Single predicate for the scrolled flag and the unseen-arrival count:
    /// reaching the bottom edge clears both, and partial moves toward it
    /// reveal rows from the top of the hidden region, so unseen arrivals cap
    /// at the rows still below the view.
    pub(crate) fn sync_follow_state(&mut self) {
        self.user_scrolled = self.scroll_manager.offset() != 0;
        self.pending_new_messages = self.pending_new_messages.min(self.scroll_manager.offset());
    }

    /// Adjust scroll position after content changes to keep the current view stable.
    /// When the viewport is at the bottom (offset 0), new content naturally stays
    /// in view without adjustment. Only when the user has scrolled up (offset > 0)
    /// do we adjust the offset to prevent the view from drifting.
    ///
    /// Bottom-follow returns immediately so appends never force a reflow.
    pub(crate) fn adjust_scroll_after_change(&mut self, previous_max_offset: usize) {
        if self.scroll_manager.offset() == 0 {
            self.pending_new_messages = 0;
            return;
        }
        let new_max_offset = self.current_max_scroll_offset();
        if new_max_offset > previous_max_offset {
            // Keep content position stable when the user has scrolled away from bottom
            use std::cmp::min;
            let current_offset = self.scroll_manager.offset();
            let delta = new_max_offset - previous_max_offset;
            self.scroll_manager.set_offset(min(current_offset + delta, new_max_offset));
            // Rows appended below the viewport while scrolled feed the "N new" hint.
            self.pending_new_messages = self.pending_new_messages.saturating_add(delta);
        }
        self.enforce_scroll_bounds();
        self.pending_new_messages = self.pending_new_messages.min(self.scroll_manager.offset());
    }

    /// Pin a bottom-following viewport after an in-place expansion.
    ///
    /// Expanding details while following (offset 0) grows the transcript but
    /// keeps the tail pinned, so continued streaming scrolls the just-expanded
    /// content out of view before it can be read. Absorb the growth into the
    /// offset so the pre-toggle top row stays put, and mark the session
    /// scrolled so later appends feed the "N new" hint instead of scrolling.
    /// Shrinks, already-scrolled views, and sub-viewport transcripts are no-ops.
    pub(crate) fn pin_viewport_after_expansion(&mut self, previous_max_offset: usize) {
        if self.scroll_manager.offset() != 0 {
            return;
        }
        let growth = self.current_max_scroll_offset().saturating_sub(previous_max_offset);
        if growth == 0 {
            return;
        }
        self.scroll_manager.set_offset(growth);
        self.sync_follow_state();
    }

    /// Emit an inline event through the channel and callback
    #[inline]
    pub(crate) fn emit_inline_event(
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
    fn handle_scroll_down(
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
    fn handle_scroll_up(
        &mut self,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        self.scroll_line_up();
        self.mark_dirty();
        self.emit_inline_event(&InlineEvent::ScrollLineUp, events, callback);
    }
}
