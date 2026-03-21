use super::transcript_links::TranscriptLinkClickAction;
use super::*;
use std::time::Instant;

impl Session {
    fn input_area_contains(&self, column: u16, row: u16) -> bool {
        self.input_area.is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        })
    }

    fn handle_modal_list_result(
        &mut self,
        result: modal::ModalListKeyResult,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) -> bool {
        match result {
            modal::ModalListKeyResult::NotHandled => false,
            modal::ModalListKeyResult::HandledNoRedraw => true,
            modal::ModalListKeyResult::Redraw => {
                self.mark_dirty();
                true
            }
            modal::ModalListKeyResult::Emit(event) => {
                self.mark_dirty();
                self.emit_inline_event(&event, events, callback);
                true
            }
            modal::ModalListKeyResult::Submit(event) | modal::ModalListKeyResult::Cancel(event) => {
                self.close_overlay();
                self.mark_dirty();
                self.emit_inline_event(&event, events, callback);
                true
            }
        }
    }

    fn modal_visible_index_at(&self, row: u16) -> Option<usize> {
        let area = self.modal_list_area?;
        if row < area.y || row >= area.y.saturating_add(area.height) {
            return None;
        }

        let styles = render::modal_render_styles(self);
        let content_width =
            area.width
                .saturating_sub(inline_list::selection_padding_width() as u16) as usize;
        let relative_row = usize::from(row.saturating_sub(area.y));

        if let Some(wizard) = self.wizard_overlay() {
            let step = wizard.steps.get(wizard.current_step)?;
            let offset = step.list.list_state.offset();
            let visible_indices = &step.list.visible_indices;
            let mut consumed_rows = 0usize;
            for (visible_index, &item_index) in visible_indices.iter().enumerate().skip(offset) {
                let lines = modal::modal_list_item_lines(
                    &step.list,
                    visible_index,
                    item_index,
                    &styles,
                    content_width,
                    None,
                );
                let height = usize::from(inline_list::row_height(&lines));
                if relative_row < consumed_rows + height {
                    return Some(visible_index);
                }
                consumed_rows += height;
                if consumed_rows >= usize::from(area.height) {
                    break;
                }
            }
            return None;
        }

        let modal = self.modal_state()?;
        let list = modal.list.as_ref()?;
        let offset = list.list_state.offset();
        let mut consumed_rows = 0usize;
        for (visible_index, &item_index) in list.visible_indices.iter().enumerate().skip(offset) {
            let lines = modal::modal_list_item_lines(
                list,
                visible_index,
                item_index,
                &styles,
                content_width,
                None,
            );
            let height = usize::from(inline_list::row_height(&lines));
            if relative_row < consumed_rows + height {
                return Some(visible_index);
            }
            consumed_rows += height;
            if consumed_rows >= usize::from(area.height) {
                break;
            }
        }

        None
    }

    fn handle_active_overlay_click(
        &mut self,
        mouse_event: MouseEvent,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) -> bool {
        let column = mouse_event.column;
        let row = mouse_event.row;
        let in_modal_list = self.modal_list_area.is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        });
        if !in_modal_list {
            return self.has_active_overlay();
        }

        let Some(visible_index) = self.modal_visible_index_at(row) else {
            return true;
        };

        if let Some(wizard) = self.wizard_overlay_mut() {
            let result = wizard.handle_mouse_click(visible_index);
            return self.handle_modal_list_result(result, events, callback);
        }

        if let Some(modal) = self.modal_state_mut() {
            let result = modal.handle_list_mouse_click(visible_index);
            return self.handle_modal_list_result(result, events, callback);
        }

        true
    }

    fn handle_active_overlay_scroll(
        &mut self,
        mouse_event: MouseEvent,
        down: bool,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) -> bool {
        if !self.has_active_overlay() {
            return false;
        }

        let column = mouse_event.column;
        let row = mouse_event.row;
        let in_modal_list = self.modal_list_area.is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        });

        if !in_modal_list {
            return true;
        }

        if let Some(wizard) = self.wizard_overlay_mut() {
            let result = wizard.handle_mouse_scroll(down);
            return self.handle_modal_list_result(result, events, callback);
        }

        if let Some(modal) = self.modal_state_mut() {
            let result = modal.handle_list_mouse_scroll(down);
            return self.handle_modal_list_result(result, events, callback);
        }

        true
    }

    fn handle_bottom_panel_scroll(&mut self, down: bool) -> bool {
        let _ = down;
        false
    }

    fn handle_bottom_panel_click(&mut self, mouse_event: MouseEvent) -> bool {
        let _ = mouse_event;
        false
    }

    pub fn handle_event(
        &mut self,
        event: CrosstermEvent,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        match event {
            CrosstermEvent::Key(key) => {
                self.update_held_key_modifiers(&key);
                // Only process Press events to avoid duplicate character insertion
                // Repeat events can cause characters to be inserted multiple times
                if matches!(key.kind, KeyEventKind::Press)
                    && let Some(outbound) = events::process_key(self, key)
                {
                    self.emit_inline_event(&outbound, events, callback);
                }
            }
            CrosstermEvent::Mouse(mouse_event) => match mouse_event.kind {
                MouseEventKind::Moved => {
                    if self.update_transcript_file_link_hover(mouse_event.column, mouse_event.row) {
                        self.mark_dirty();
                    }
                }
                MouseEventKind::ScrollDown => {
                    self.mouse_selection.clear_click_history();
                    if !self.handle_active_overlay_scroll(mouse_event, true, events, callback)
                        && !self.handle_bottom_panel_scroll(true)
                    {
                        self.scroll_line_down();
                        self.mark_dirty();
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.mouse_selection.clear_click_history();
                    if !self.handle_active_overlay_scroll(mouse_event, false, events, callback)
                        && !self.handle_bottom_panel_scroll(false)
                    {
                        self.scroll_line_up();
                        self.mark_dirty();
                    }
                }
                MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                    match self.transcript_file_link_click_action(
                        mouse_event.column,
                        mouse_event.row,
                        mouse_event.modifiers,
                    ) {
                        TranscriptLinkClickAction::Open(outbound) => {
                            self.mark_dirty();
                            self.emit_inline_event(&outbound, events, callback);
                            self.mouse_selection.clear_click_history();
                            return;
                        }
                        TranscriptLinkClickAction::Consume => {
                            self.mouse_selection.clear_click_history();
                            return;
                        }
                        TranscriptLinkClickAction::Ignore => {}
                    }

                    if self.has_active_overlay()
                        && self.handle_active_overlay_click(mouse_event, events, callback)
                    {
                        self.mouse_selection.clear_click_history();
                        return;
                    }

                    if self.handle_bottom_panel_click(mouse_event) {
                        self.mouse_selection.clear_click_history();
                        return;
                    }

                    if self.handle_input_click(mouse_event) {
                        self.mouse_drag_target = MouseDragTarget::Input;
                        self.mouse_selection.clear();
                        return;
                    }

                    let is_double_click = self.mouse_selection.register_click(
                        mouse_event.column,
                        mouse_event.row,
                        Instant::now(),
                    );
                    if is_double_click {
                        self.mouse_drag_target = MouseDragTarget::None;
                        let _ = self.handle_transcript_click(mouse_event);
                        if self.select_transcript_word_at(mouse_event.column, mouse_event.row) {
                            self.mark_dirty();
                        } else {
                            self.mouse_selection.clear();
                        }
                        self.mouse_selection.clear_click_history();
                        return;
                    }

                    self.mouse_drag_target = MouseDragTarget::Transcript;
                    self.mouse_selection
                        .start_selection(mouse_event.column, mouse_event.row);
                    self.mark_dirty();
                    self.handle_transcript_click(mouse_event);
                }
                MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                    match self.mouse_drag_target {
                        MouseDragTarget::Input => {
                            if let Some(cursor) = self
                                .cursor_index_for_input_point(mouse_event.column, mouse_event.row)
                                && self.input_manager.cursor() != cursor
                            {
                                self.input_manager.set_cursor_with_selection(cursor);
                                self.mark_dirty();
                            }
                        }
                        MouseDragTarget::Transcript => {
                            self.mouse_selection
                                .update_selection(mouse_event.column, mouse_event.row);
                            self.mark_dirty();
                        }
                        MouseDragTarget::None => {}
                    }
                }
                MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                    match self.mouse_drag_target {
                        MouseDragTarget::Input => {
                            if let Some(cursor) = self
                                .cursor_index_for_input_point(mouse_event.column, mouse_event.row)
                                && self.input_manager.cursor() != cursor
                            {
                                self.input_manager.set_cursor_with_selection(cursor);
                                self.mark_dirty();
                            }
                        }
                        MouseDragTarget::Transcript => {
                            self.mouse_selection
                                .finish_selection(mouse_event.column, mouse_event.row);
                            self.mark_dirty();
                        }
                        MouseDragTarget::None => {}
                    }
                    self.mouse_drag_target = MouseDragTarget::None;
                }
                _ => {}
            },
            CrosstermEvent::Paste(content) => {
                events::handle_paste(self, &content);
            }
            CrosstermEvent::Resize(_, rows) => {
                self.apply_view_rows(rows);
                self.mark_dirty();
            }
            CrosstermEvent::FocusGained => {
                // No-op: focus tracking is host/application concern.
            }
            CrosstermEvent::FocusLost => {
                self.clear_held_key_modifiers();
            }
        }
    }

    pub(crate) fn handle_transcript_click(&mut self, mouse_event: MouseEvent) -> bool {
        if !matches!(
            mouse_event.kind,
            MouseEventKind::Down(crossterm::event::MouseButton::Left)
        ) {
            return false;
        }

        let Some(area) = self.transcript_area else {
            return false;
        };

        if mouse_event.row < area.y
            || mouse_event.row >= area.y.saturating_add(area.height)
            || mouse_event.column < area.x
            || mouse_event.column >= area.x.saturating_add(area.width)
        {
            return false;
        }

        if self.transcript_width == 0 || self.transcript_rows == 0 {
            return false;
        }

        let row_in_view = (mouse_event.row - area.y) as usize;
        if row_in_view >= self.transcript_rows as usize {
            return false;
        }

        let viewport_rows = self.transcript_rows.max(1) as usize;
        let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
        let effective_padding = padding.min(viewport_rows.saturating_sub(1));
        let total_rows = self.total_transcript_rows(self.transcript_width) + effective_padding;
        let (top_offset, _clamped_total_rows) =
            self.prepare_transcript_scroll(total_rows, viewport_rows);
        let view_top = top_offset.min(self.scroll_manager.max_offset());
        self.transcript_view_top = view_top;

        let clicked_row = view_top.saturating_add(row_in_view);
        let expanded = self.expand_collapsed_paste_at_row(self.transcript_width, clicked_row);
        if expanded {
            self.mark_dirty();
        }
        expanded
    }

    pub(crate) fn transcript_word_selection_range(
        &mut self,
        column: u16,
        row: u16,
    ) -> Option<((u16, u16), (u16, u16))> {
        let area = self.transcript_area?;
        if row < area.y
            || row >= area.y.saturating_add(area.height)
            || column < area.x
            || column >= area.x.saturating_add(area.width)
        {
            return None;
        }

        if self.transcript_width == 0 || self.transcript_rows == 0 {
            return None;
        }

        let row_in_view = usize::from(row.saturating_sub(area.y));
        if row_in_view >= self.transcript_rows as usize {
            return None;
        }

        let viewport_rows = self.transcript_rows.max(1) as usize;
        let visible_lines = self.collect_transcript_window_cached(
            self.transcript_width,
            self.transcript_view_top,
            viewport_rows,
        );
        let line = visible_lines.get(row_in_view)?;

        let text: String = line
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let local_column = column.saturating_sub(area.x);
        let (start_col, end_col) = mouse_selection::word_selection_range(&text, local_column)?;

        let start = (area.x.saturating_add(start_col), row);
        let end = (area.x.saturating_add(end_col), row);
        (start != end).then_some((start, end))
    }

    pub(crate) fn select_transcript_word_at(&mut self, column: u16, row: u16) -> bool {
        let Some((start, end)) = self.transcript_word_selection_range(column, row) else {
            return false;
        };

        self.mouse_selection.set_selection(start, end);
        true
    }

    pub(crate) fn handle_input_click(&mut self, mouse_event: MouseEvent) -> bool {
        if !matches!(
            mouse_event.kind,
            MouseEventKind::Down(crossterm::event::MouseButton::Left)
        ) {
            return false;
        }

        if !self.input_area_contains(mouse_event.column, mouse_event.row) {
            return false;
        }

        let cursor_at_end = self.input_manager.cursor() == self.input_manager.content().len();
        if self.input_compact_mode && cursor_at_end && self.input_compact_placeholder().is_some() {
            self.input_compact_mode = false;
            self.mark_dirty();
            return true;
        }

        if let Some(cursor) = self.cursor_index_for_input_point(mouse_event.column, mouse_event.row)
        {
            if self.input_manager.cursor() != cursor {
                self.input_manager.set_cursor(cursor);
                self.mark_dirty();
            }
            return true;
        }

        false
    }
}
