use super::*;

impl Session {
    fn input_area_contains(&self, column: u16, row: u16) -> bool {
        self.input_area.is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        })
    }

    fn bottom_panel_contains(&self, column: u16, row: u16) -> bool {
        self.bottom_panel_area.is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        })
    }

    fn shared_panel_row_index(
        &self,
        fixed_rows: u16,
        visible_rows: usize,
        column: u16,
        row: u16,
    ) -> Option<usize> {
        let area = self.bottom_panel_area?;
        if row < area.y
            || row >= area.y.saturating_add(area.height)
            || column < area.x
            || column >= area.x.saturating_add(area.width)
        {
            return None;
        }

        let relative_row = row.saturating_sub(area.y);
        if relative_row < fixed_rows {
            return None;
        }

        let list_row = usize::from(relative_row - fixed_rows);
        (list_row < visible_rows).then_some(list_row)
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
        if self.bottom_panel_area.is_none() {
            return false;
        }

        if self.file_palette_active {
            let Some(palette) = self.file_palette.as_mut() else {
                return true;
            };
            if down {
                palette.move_selection_down();
            } else {
                palette.move_selection_up();
            }
            self.mark_dirty();
            return true;
        }

        if self.history_picker_state.active {
            if down {
                self.history_picker_state.move_down();
            } else {
                self.history_picker_state.move_up();
            }
            self.mark_dirty();
            return true;
        }

        if slash::slash_navigation_available(self) {
            if down {
                slash::move_slash_selection_down(self);
            } else {
                slash::move_slash_selection_up(self);
            }
            return true;
        }

        false
    }

    fn handle_bottom_panel_click(&mut self, mouse_event: MouseEvent) -> bool {
        let column = mouse_event.column;
        let row = mouse_event.row;
        if !self.bottom_panel_contains(column, row) {
            return false;
        }

        if self.file_palette_active {
            let fixed_rows = list_panel::fixed_section_rows(1, 2, true);
            let visible_rows = self
                .file_palette
                .as_ref()
                .map(|palette| palette.current_page_items().len())
                .unwrap_or(0);
            let local_index = self.shared_panel_row_index(fixed_rows, visible_rows, column, row);
            let mut apply_path = None;
            let mut should_mark_dirty = false;
            let Some(palette) = self.file_palette.as_mut() else {
                return true;
            };
            if !palette.has_files() {
                return true;
            }

            let page_items = palette.current_page_items();
            if let Some(local_index) = local_index
                && let Some((global_index, entry, selected)) = page_items.get(local_index)
            {
                if *selected {
                    apply_path = Some(entry.relative_path.clone());
                } else if palette.select_index(*global_index) {
                    should_mark_dirty = true;
                }
            }

            if let Some(path) = apply_path {
                self.insert_file_reference(&path);
                self.close_file_palette();
                self.mark_dirty();
            } else if should_mark_dirty {
                self.mark_dirty();
            }
            return true;
        }

        if self.history_picker_state.active {
            let fixed_rows = list_panel::fixed_section_rows(1, 1, true);
            let visible_rows = if self.history_picker_state.matches.is_empty() {
                1
            } else {
                self.history_picker_state
                    .matches
                    .len()
                    .min(ui::INLINE_LIST_MAX_ROWS)
            };
            if let Some(local_index) =
                self.shared_panel_row_index(fixed_rows, visible_rows, column, row)
                && !self.history_picker_state.matches.is_empty()
            {
                let actual_index = self
                    .history_picker_state
                    .scroll_offset()
                    .saturating_add(local_index);
                if self.history_picker_state.selected_index() == Some(actual_index) {
                    self.history_picker_state.accept(&mut self.input_manager);
                } else if self.history_picker_state.select_index(actual_index) {
                    self.mark_dirty();
                }
            }
            return true;
        }

        if slash::slash_navigation_available(self) {
            let fixed_rows = list_panel::fixed_section_rows(1, 1, true);
            let visible_rows = self
                .slash_palette
                .suggestions()
                .len()
                .min(ui::INLINE_LIST_MAX_ROWS);
            if let Some(local_index) =
                self.shared_panel_row_index(fixed_rows, visible_rows, column, row)
            {
                let actual_index = self
                    .slash_palette
                    .scroll_offset()
                    .saturating_add(local_index);
                if self.slash_palette.selected_index() == Some(actual_index) {
                    slash::apply_selected_slash_suggestion(self);
                } else {
                    slash::select_slash_suggestion_index(self, actual_index);
                }
            }
            return true;
        }

        true
    }

    pub fn handle_event(
        &mut self,
        event: CrosstermEvent,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        match event {
            CrosstermEvent::Key(key) => {
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
                    if !self.handle_active_overlay_scroll(mouse_event, true, events, callback)
                        && !self.handle_bottom_panel_scroll(true)
                    {
                        self.scroll_line_down();
                        self.mark_dirty();
                    }
                }
                MouseEventKind::ScrollUp => {
                    if !self.handle_active_overlay_scroll(mouse_event, false, events, callback)
                        && !self.handle_bottom_panel_scroll(false)
                    {
                        self.scroll_line_up();
                        self.mark_dirty();
                    }
                }
                MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                    if let Some(outbound) = self.transcript_file_link_event(
                        mouse_event.column,
                        mouse_event.row,
                        mouse_event.modifiers,
                    ) {
                        self.mark_dirty();
                        self.emit_inline_event(&outbound, events, callback);
                        return;
                    }

                    if self.has_active_overlay()
                        && self.handle_active_overlay_click(mouse_event, events, callback)
                    {
                        return;
                    }

                    if self.handle_bottom_panel_click(mouse_event) {
                        return;
                    }

                    if self.handle_input_click(mouse_event) {
                        self.mouse_drag_target = MouseDragTarget::Input;
                        self.mouse_selection.clear();
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
                // No-op: focus tracking is host/application concern.
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
