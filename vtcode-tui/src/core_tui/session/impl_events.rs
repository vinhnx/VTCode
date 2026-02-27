use super::*;

impl Session {
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
                MouseEventKind::ScrollDown => {
                    // Check if history picker is active - delegate scrolling to picker
                    if self.history_picker_state.active {
                        self.history_picker_state.move_down();
                        self.mark_dirty();
                    } else {
                        self.scroll_line_down();
                        self.mark_dirty();
                    }
                }
                MouseEventKind::ScrollUp => {
                    // Check if history picker is active - delegate scrolling to picker
                    if self.history_picker_state.active {
                        self.history_picker_state.move_up();
                        self.mark_dirty();
                    } else {
                        self.scroll_line_up();
                        self.mark_dirty();
                    }
                }
                MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) => {
                    // Start mouse text selection
                    self.mouse_selection
                        .start_selection(mouse_event.column, mouse_event.row);
                    self.mark_dirty();
                    if !self.handle_input_click(mouse_event) {
                        self.handle_transcript_click(mouse_event);
                    }
                }
                MouseEventKind::Drag(ratatui::crossterm::event::MouseButton::Left) => {
                    self.mouse_selection
                        .update_selection(mouse_event.column, mouse_event.row);
                    self.mark_dirty();
                }
                MouseEventKind::Up(ratatui::crossterm::event::MouseButton::Left) => {
                    self.mouse_selection
                        .finish_selection(mouse_event.column, mouse_event.row);
                    self.mark_dirty();
                }
                _ => {}
            },
            CrosstermEvent::Paste(content) => {
                if self.input_enabled {
                    self.insert_paste_text(&content);
                    self.check_file_reference_trigger();
                    self.mark_dirty();
                } else if let Some(modal) = self.modal.as_mut()
                    && let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut())
                {
                    search.insert(&content);
                    list.apply_search(&search.query);
                    self.mark_dirty();
                }
            }
            CrosstermEvent::Resize(_, rows) => {
                self.apply_view_rows(rows);
                self.mark_dirty();
            }
            CrosstermEvent::FocusGained => {
                crate::notifications::set_global_terminal_focused(true);
            }
            CrosstermEvent::FocusLost => {
                crate::notifications::set_global_terminal_focused(false);
            }
        }
    }

    pub(crate) fn handle_transcript_click(&mut self, mouse_event: MouseEvent) -> bool {
        if !matches!(
            mouse_event.kind,
            MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left)
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
            MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left)
        ) {
            return false;
        }

        let Some(area) = self.input_area else {
            return false;
        };

        if mouse_event.row < area.y
            || mouse_event.row >= area.y.saturating_add(area.height)
            || mouse_event.column < area.x
            || mouse_event.column >= area.x.saturating_add(area.width)
        {
            return false;
        }

        let cursor_at_end = self.input_manager.cursor() == self.input_manager.content().len();
        if !self.input_compact_mode || !cursor_at_end {
            return false;
        }

        if self.input_compact_placeholder().is_none() {
            return false;
        }

        self.input_compact_mode = false;
        self.mark_dirty();
        true
    }
}
