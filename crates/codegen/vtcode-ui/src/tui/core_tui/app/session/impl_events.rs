use super::*;
use crate::tui::config::constants::ui;
use crate::tui::core_tui::app::types::InlineEvent;
use crate::tui::core_tui::session::MouseDragTarget;
use crate::tui::core_tui::session::render::modal_render_styles;
use crate::tui::core_tui::session::{TranscriptLinkClickAction, inline_list, list_panel, modal};
use crate::tui::core_tui::types::{InlineEvent as CoreInlineEvent, OverlayEvent, OverlaySelectionChange};
use crate::tui::ui::theme;
use std::time::Instant;

impl Session {
    #[cfg(test)]
    pub(crate) fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
        events::process_key(self, key)
    }

    #[cfg(test)]
    pub(crate) fn process_key_with_clipboard_image_reader(
        &mut self,
        key: KeyEvent,
        image_reader: impl FnMut() -> Result<
            crate::tui::core_tui::types::ContentPart,
            crate::tui::core_tui::session::clipboard_image::ClipboardImageError,
        >,
    ) -> Option<InlineEvent> {
        events::process_key_with_clipboard_image_reader(self, key, image_reader)
    }

    #[cfg(test)]
    pub(crate) fn process_key_with_clipboard_text_reader(
        &mut self,
        key: KeyEvent,
        text_reader: impl FnMut() -> Result<String, crate::tui::core_tui::session::clipboard_image::ClipboardTextError>,
    ) -> Option<InlineEvent> {
        events::process_key_with_clipboard_text_reader(self, key, text_reader)
    }

    fn input_area_contains(&self, column: u16, row: u16) -> bool {
        self.core.input_area().is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        })
    }

    fn bottom_panel_contains(&self, column: u16, row: u16) -> bool {
        self.core.bottom_panel_area().is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        })
    }

    fn handle_tool_output_viewer_scroll(&mut self, column: u16, row: u16, down: bool) -> bool {
        let fallback_height = self.core.transcript_rows.max(1);
        let Some(viewer) = self.tool_output_viewer_state_mut() else {
            return false;
        };
        if !viewer.viewer_contains(column, row) {
            return false;
        }

        let viewport_height = viewer.content_height_or(fallback_height);
        if down {
            viewer.scroll_line_down(viewport_height);
        } else {
            viewer.scroll_line_up(viewport_height);
        }
        self.mark_dirty();
        true
    }

    fn panel_row_index(&self, layout: &list_panel::ListPanelLayout, column: u16, row: u16) -> Option<usize> {
        let area = self.core.bottom_panel_area()?;
        layout.row_index(area, column, row)
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
                // Synchronous preview: fire the callback and sync session theme
                // before returning so the render picks up the preview in the
                // same frame as the cursor movement.
                if let Some(ref cb) = self.preview_callback
                    && let CoreInlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
                        ref selection,
                    ))) = event
                {
                    let _ = cb(Some(selection));
                    if theme::has_preview_theme() {
                        self.sync_theme_from_runtime();
                    }
                }
                let outbound: InlineEvent = event.into();
                events::emit_inline_event(&outbound, events, callback);
                true
            }
            modal::ModalListKeyResult::Submit(event) => {
                self.close_overlay();
                self.mark_dirty();
                let outbound: InlineEvent = event.into();
                events::emit_inline_event(&outbound, events, callback);
                true
            }
            modal::ModalListKeyResult::Cancel(event) => {
                self.cancel_theme_preview();
                self.close_overlay();
                self.mark_dirty();
                let outbound: InlineEvent = event.into();
                events::emit_inline_event(&outbound, events, callback);
                true
            }
        }
    }

    fn handle_link_click_action(
        &mut self,
        action: TranscriptLinkClickAction,
        clear_drag_target: bool,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) -> bool {
        match action {
            TranscriptLinkClickAction::Open(outbound) => {
                if clear_drag_target {
                    self.core.mouse_drag_target = MouseDragTarget::None;
                }
                self.mark_dirty();
                let outbound: InlineEvent = outbound.into();
                events::emit_inline_event(&outbound, events, callback);
                self.core.mouse_selection.clear_click_history();
                true
            }
            TranscriptLinkClickAction::Consume => {
                if clear_drag_target {
                    self.core.mouse_drag_target = MouseDragTarget::None;
                }
                self.core.mouse_selection.clear_click_history();
                true
            }
            TranscriptLinkClickAction::Ignore => false,
        }
    }

    fn modal_visible_index_at(&self, row: u16) -> Option<usize> {
        let area = self.core.modal_list_area()?;
        let styles = modal_render_styles(self);
        if let Some(wizard) = self.wizard_overlay() {
            let step = wizard.steps.get(wizard.current_step)?;
            // Render passes the inline custom-note editor, which appends one row
            // to its item; the shared helper measures the same heights.
            let inline_editor = modal::inline_editor_for_step(step);
            return modal::visible_index_at_row(
                &step.list,
                None,
                inline_editor.as_ref(),
                &styles,
                area,
                row,
                wizard.numbered_shortcuts(),
                false,
            );
        }

        let modal = self.modal_state()?;
        let list = modal.list.as_ref()?;
        // Plain modals never render an inline editor (render passes `None`).
        modal::visible_index_at_row(
            list,
            modal.footer_hint.as_deref(),
            None,
            &styles,
            area,
            row,
            modal.search.is_none(),
            modal.status.is_some(),
        )
    }

    fn handle_active_overlay_click(
        &mut self,
        mouse_event: MouseEvent,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) -> bool {
        let column = mouse_event.column;
        let row = mouse_event.row;
        let in_modal_list = self.core.modal_list_area().is_some_and(|area| {
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

    fn modal_text_area_contains(&self, column: u16, row: u16) -> bool {
        self.core.modal_text_areas().iter().any(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        })
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
        let in_modal_list = self.core.modal_list_area().is_some_and(|area| {
            row >= area.y
                && row < area.y.saturating_add(area.height)
                && column >= area.x
                && column < area.x.saturating_add(area.width)
        });

        if !in_modal_list {
            // The floating modal only owns input inside its rendered list.
            // Let wheel events outside that hitbox reach the transcript so a
            // long plan remains reviewable while approval is open.
            return false;
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
        if self.local_agents_visible() {
            let changed = if down {
                self.local_agents_state.move_selection_down()
            } else {
                self.local_agents_state.move_selection_up()
            };
            if changed {
                self.mark_dirty();
            }
            return true;
        }

        if self.core.bottom_panel_area().is_none() {
            return false;
        }

        if self.agent_palette_visible() {
            let Some(palette) = self.agent_palette.as_mut() else {
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

        if self.file_palette_visible() {
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

        if self.history_picker_visible() {
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
        if self.local_agents_visible() {
            let pos = Position { x: column, y: row };
            let Some(window) = self.local_agents_state.window_area() else {
                return false;
            };
            if !window.contains(pos) {
                return false;
            }
            // Header/title click toggles compact <-> 75% expanded.
            if row <= window.y.saturating_add(1) {
                self.local_agents_state.toggle_expanded();
                self.mark_dirty();
                return true;
            }
            if let Some(list_area) = self.local_agents_state.list_area()
                && list_area.contains(pos)
            {
                let local_index = usize::from(row.saturating_sub(list_area.y));
                let actual_index = self.local_agents_state.scroll_offset().saturating_add(local_index);
                if self.local_agents_state.select_index(actual_index) {
                    self.mark_dirty();
                }
            }
            return true;
        }
        if !self.bottom_panel_contains(column, row) {
            return false;
        }

        if self.agent_palette_visible() {
            let Some(layout) = render::agent_palette_panel_layout(self) else {
                return true;
            };
            let bottom_area = self.core.bottom_panel_area();
            let Some(palette) = self.agent_palette.as_mut() else {
                return true;
            };
            let local_index = bottom_area.and_then(|area| layout.row_index(area, column, row));
            let mut apply_name = None;
            let mut should_mark_dirty = false;
            if !palette.has_agents() {
                return true;
            }

            let page_items = palette.current_page_items();
            if let Some(local_index) = local_index
                && let Some((global_index, entry, selected)) = page_items.get(local_index)
            {
                if *selected {
                    apply_name = Some(entry.name.clone());
                } else if palette.select_index(*global_index) {
                    should_mark_dirty = true;
                }
            }

            if let Some(name) = apply_name {
                self.insert_agent_reference(&name);
                self.close_agent_palette();
                self.mark_dirty();
            } else if should_mark_dirty {
                self.mark_dirty();
            }
            return true;
        }

        if self.file_palette_visible() {
            let Some(layout) = render::file_palette_panel_layout(self) else {
                return true;
            };
            let bottom_area = self.core.bottom_panel_area();
            let Some(palette) = self.file_palette.as_mut() else {
                return true;
            };
            let local_index = bottom_area.and_then(|area| layout.row_index(area, column, row));
            let mut apply_path = None;
            let mut should_mark_dirty = false;
            if !palette.has_files() {
                return true;
            }

            if let Some(local_index) = local_index {
                let is_selected = palette.selected_index() == Some(local_index);
                if let Some(entry) = palette.list_entries().get(local_index).cloned() {
                    if is_selected {
                        if entry.is_dir {
                            palette.enter_selected_dir();
                            should_mark_dirty = true;
                        } else {
                            apply_path = Some(entry.relative_path.clone());
                        }
                    } else if palette.select_index(local_index) {
                        should_mark_dirty = true;
                    }
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

        if self.history_picker_visible() {
            let Some(layout) = render::history_picker_panel_layout(self) else {
                return true;
            };
            if let Some(local_index) = self.panel_row_index(&layout, column, row)
                && !self.history_picker_state.matches.is_empty()
            {
                let actual_index = self.history_picker_state.scroll_offset().saturating_add(local_index);
                if self.history_picker_state.selected_index() == Some(actual_index) {
                    let was_active = self.history_picker_visible();
                    self.history_picker_state.accept(&mut self.core.input_manager);
                    self.finish_history_picker_interaction(was_active);
                    self.mark_dirty();
                } else if self.history_picker_state.select_index(actual_index) {
                    self.mark_dirty();
                }
            }
            return true;
        }

        if slash::slash_navigation_available(self) {
            let Some(layout) = slash::slash_panel_layout(self) else {
                return true;
            };
            if let Some(local_index) = self.panel_row_index(&layout, column, row) {
                let actual_index = self.slash_palette.scroll_offset().saturating_add(local_index);
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

    #[cfg_attr(feature = "profiling", hotpath::measure)]
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
                    events::emit_inline_event(&outbound, events, callback);
                }
            }
            CrosstermEvent::Mouse(mouse_event) => {
                if !self.core.fullscreen.interaction.mouse_capture {
                    return;
                }

                match mouse_event.kind {
                    MouseEventKind::Moved => {
                        let viewer_visible = self.tool_output_viewer_state().is_some();
                        let mode_hover_changed = self
                            .tool_output_viewer_state_mut()
                            .is_some_and(|viewer| viewer.update_mode_hover(mouse_event.column, mouse_event.row));
                        let close_hover_changed = self
                            .tool_output_viewer_state_mut()
                            .is_some_and(|viewer| viewer.update_close_hover(mouse_event.column, mouse_event.row));
                        let link_hover_changed = if viewer_visible {
                            self.core.clear_transcript_file_link_hover()
                        } else {
                            self.update_transcript_file_link_hover(mouse_event.column, mouse_event.row)
                        };
                        if mode_hover_changed || close_hover_changed || link_hover_changed {
                            self.mark_visual_dirty();
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        self.core.clear_pending_link_click();
                        self.core.mouse_selection.clear_click_history();
                        if !self.handle_tool_output_viewer_scroll(mouse_event.column, mouse_event.row, true)
                            && !self.handle_active_overlay_scroll(mouse_event, true, events, callback)
                            && !self.handle_bottom_panel_scroll(true)
                        {
                            self.scroll_line_down();
                            self.mark_visual_dirty();
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        self.core.clear_pending_link_click();
                        self.core.mouse_selection.clear_click_history();
                        if !self.handle_tool_output_viewer_scroll(mouse_event.column, mouse_event.row, false)
                            && !self.handle_active_overlay_scroll(mouse_event, false, events, callback)
                            && !self.handle_bottom_panel_scroll(false)
                        {
                            self.scroll_line_up();
                            self.mark_visual_dirty();
                        }
                    }
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        self.core.clear_pending_link_click();
                        if self
                            .tool_output_viewer_state()
                            .is_some_and(|viewer| viewer.close_control_contains(mouse_event.column, mouse_event.row))
                        {
                            self.close_tool_output_viewer();
                            self.core.clear_mouse_selection();
                            self.mark_dirty();
                            return;
                        }
                        if self
                            .tool_output_viewer_state()
                            .is_some_and(|viewer| viewer.mode_control_contains(mouse_event.column, mouse_event.row))
                        {
                            if let Some(viewer) = self.tool_output_viewer_state_mut() {
                                viewer.toggle_render_mode();
                            }
                            self.core.clear_mouse_selection();
                            self.mark_dirty();
                            return;
                        }

                        if self.tool_output_viewer_state().is_some() {
                            if mouse_event.modifiers == crossterm::event::KeyModifiers::ALT
                                && let Some(target) = self
                                    .tool_output_viewer_state()
                                    .and_then(|viewer| viewer.evidence_at(mouse_event.column, mouse_event.row))
                                    .map(str::to_owned)
                            {
                                self.handle_link_click_action(
                                    TranscriptLinkClickAction::Open(CoreInlineEvent::OpenUrl(target)),
                                    true,
                                    events,
                                    callback,
                                );
                                return;
                            }
                            if self
                                .tool_output_viewer_state()
                                .is_some_and(|viewer| viewer.body_contains(mouse_event.column, mouse_event.row))
                            {
                                self.core.mouse_drag_target = MouseDragTarget::Transcript;
                                self.core.cancel_drag_auto_scroll();
                                self.core
                                    .mouse_selection
                                    .start_overlay_selection(mouse_event.column, mouse_event.row);
                            }
                            self.mark_dirty();
                            return;
                        }

                        if !self.has_active_overlay()
                            && !self.local_agents_visible()
                            && self.diff_preview_state().is_none()
                            && self.core.handle_sticky_prompt_click(mouse_event)
                        {
                            self.mark_dirty();
                            return;
                        }

                        if self.core.queue_link_click_action(self.transcript_file_link_click_action(
                            mouse_event.column,
                            mouse_event.row,
                            mouse_event.modifiers,
                        )) {
                            self.core.clear_mouse_selection();
                            return;
                        }

                        if let Some(review_anchor) =
                            self.compact_activity_review_anchor_at(mouse_event.column, mouse_event.row)
                        {
                            let width = self.core.transcript_width.max(1);
                            let height = self.core.transcript_rows.max(1);
                            self.open_tool_output_viewer(width, height, Some(review_anchor));
                            self.core.clear_mouse_selection();
                            return;
                        }

                        if self.has_active_overlay() {
                            let in_modal_list = self.core.modal_list_area().is_some_and(|area| {
                                mouse_event.row >= area.y
                                    && mouse_event.row < area.y.saturating_add(area.height)
                                    && mouse_event.column >= area.x
                                    && mouse_event.column < area.x.saturating_add(area.width)
                            });
                            if self.core.queue_link_click_action(self.modal_link_click_action(
                                mouse_event.column,
                                mouse_event.row,
                                mouse_event.modifiers,
                            )) {
                                self.core.clear_mouse_selection();
                                return;
                            }

                            if self.modal_text_area_contains(mouse_event.column, mouse_event.row) && !in_modal_list {
                                let is_double_click = self.core.mouse_selection.register_click(
                                    mouse_event.column,
                                    mouse_event.row,
                                    Instant::now(),
                                );
                                if is_double_click {
                                    let modal_double_click_action = self.core.throttle_link_click_action(
                                        self.modal_link_double_click_action(mouse_event.column, mouse_event.row),
                                    );
                                    if !matches!(modal_double_click_action, TranscriptLinkClickAction::Ignore) {
                                        self.core.clear_pending_link_click();
                                    }
                                    if self.handle_link_click_action(modal_double_click_action, true, events, callback)
                                    {
                                        return;
                                    }
                                }

                                self.core.mouse_drag_target = MouseDragTarget::ModalText;
                                self.core
                                    .mouse_selection
                                    .start_overlay_selection(mouse_event.column, mouse_event.row);
                                self.mark_dirty();
                                return;
                            }
                        }

                        if self.has_active_overlay() && self.handle_active_overlay_click(mouse_event, events, callback)
                        {
                            self.core.clear_mouse_selection();
                            return;
                        }

                        if self.handle_bottom_panel_click(mouse_event) {
                            self.core.clear_mouse_selection();
                            return;
                        }

                        if self.handle_input_click(mouse_event) {
                            self.core.mouse_drag_target = MouseDragTarget::Input;
                            self.core.mouse_selection.clear();
                            return;
                        }

                        let is_double_click = self.core.mouse_selection.register_click(
                            mouse_event.column,
                            mouse_event.row,
                            Instant::now(),
                        );
                        if is_double_click {
                            let transcript_double_click_action = self.core.throttle_link_click_action(
                                self.transcript_file_link_double_click_action(mouse_event.column, mouse_event.row),
                            );
                            if !matches!(transcript_double_click_action, TranscriptLinkClickAction::Ignore) {
                                self.core.clear_pending_link_click();
                            }
                            if self.handle_link_click_action(transcript_double_click_action, true, events, callback) {
                                return;
                            }

                            self.core.mouse_drag_target = MouseDragTarget::None;
                            let _ = self.handle_transcript_click(mouse_event);
                            if self.core.select_transcript_word_at(mouse_event.column, mouse_event.row) {
                                self.mark_dirty();
                            } else {
                                self.core.mouse_selection.clear();
                            }
                            self.core.mouse_selection.clear_click_history();
                            return;
                        }

                        self.core.mouse_drag_target = MouseDragTarget::Transcript;
                        self.core.cancel_drag_auto_scroll();
                        self.core.mouse_selection.start_selection(mouse_event.column, mouse_event.row);
                        self.mark_dirty();
                        self.handle_transcript_click(mouse_event);
                    }
                    MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                        self.core.clear_pending_link_click();
                        match self.core.mouse_drag_target {
                            MouseDragTarget::Input => {
                                if let Some(cursor) =
                                    self.cursor_index_for_input_point(mouse_event.column, mouse_event.row)
                                    && self.core.input_manager.cursor() != cursor
                                {
                                    self.core.input_manager.set_cursor_with_selection(cursor);
                                    self.mark_dirty();
                                }
                            }
                            MouseDragTarget::Transcript => {
                                self.core.mouse_selection.update_selection(mouse_event.column, mouse_event.row);
                                self.core.update_drag_auto_scroll(mouse_event.column, mouse_event.row);
                                self.mark_dirty();
                            }
                            MouseDragTarget::ModalText => {
                                self.core.mouse_selection.update_selection(mouse_event.column, mouse_event.row);
                                self.mark_dirty();
                            }
                            MouseDragTarget::None => {}
                        }
                    }
                    MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                        if self.tool_output_viewer_state().is_some() {
                            if self.core.mouse_drag_target == MouseDragTarget::Transcript {
                                self.core.mouse_selection.finish_selection(mouse_event.column, mouse_event.row);
                                self.core.cancel_drag_auto_scroll();
                            }
                            self.core.mouse_drag_target = MouseDragTarget::None;
                            self.core.clear_pending_link_click();
                            self.mark_dirty();
                            return;
                        }

                        let transcript_link_action =
                            self.core.pending_link_click_action(self.transcript_file_link_click_action(
                                mouse_event.column,
                                mouse_event.row,
                                mouse_event.modifiers,
                            ));
                        let modal_link_action = self.core.pending_link_click_action(self.modal_link_click_action(
                            mouse_event.column,
                            mouse_event.row,
                            mouse_event.modifiers,
                        ));
                        match self.core.mouse_drag_target {
                            MouseDragTarget::Input => {
                                if let Some(cursor) =
                                    self.cursor_index_for_input_point(mouse_event.column, mouse_event.row)
                                    && self.core.input_manager.cursor() != cursor
                                {
                                    self.core.input_manager.set_cursor_with_selection(cursor);
                                    self.mark_dirty();
                                }
                            }
                            MouseDragTarget::Transcript => {
                                self.core.mouse_selection.finish_selection(mouse_event.column, mouse_event.row);
                                self.core.cancel_drag_auto_scroll();
                                self.mark_dirty();
                            }
                            MouseDragTarget::ModalText => {
                                self.core.mouse_selection.finish_selection(mouse_event.column, mouse_event.row);
                                self.mark_dirty();
                            }
                            MouseDragTarget::None => {}
                        }
                        self.core.mouse_drag_target = MouseDragTarget::None;
                        self.core.clear_pending_link_click();
                        if self.handle_link_click_action(transcript_link_action, false, events, callback) {
                            return;
                        }
                        if self.handle_link_click_action(modal_link_action, false, events, callback) {}
                    }
                    _ => {}
                }
            }
            CrosstermEvent::Paste(content) => {
                if let Some(event) = events::handle_paste(self, &content) {
                    events::emit_inline_event(&event, events, callback);
                }
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
            #[cfg(vendored_crossterm)]
            CrosstermEvent::ColorSchemeReport { dark } => {
                self.core.apply_terminal_color_scheme_report(dark);
            }
        }
    }

    fn handle_transcript_click(&mut self, mouse_event: MouseEvent) -> bool {
        if !matches!(mouse_event.kind, MouseEventKind::Down(crossterm::event::MouseButton::Left)) {
            return false;
        }

        let Some(area) = self.core.transcript_area() else {
            return false;
        };

        if mouse_event.row < area.y
            || mouse_event.row >= area.y.saturating_add(area.height)
            || mouse_event.column < area.x
            || mouse_event.column >= area.x.saturating_add(area.width)
        {
            return false;
        }

        if self.core.transcript_width == 0 || self.core.transcript_rows == 0 {
            return false;
        }

        let row_in_view = (mouse_event.row - area.y) as usize;
        if row_in_view >= self.core.transcript_rows as usize {
            return false;
        }

        let viewport_rows = self.core.transcript_rows.max(1) as usize;
        let transcript_width = self.core.transcript_width;
        let effective_padding = ui::effective_transcript_bottom_padding(viewport_rows);
        let total_rows = self.total_transcript_rows(transcript_width) + effective_padding;
        let (top_offset, _clamped_total_rows) = self.prepare_transcript_scroll(total_rows, viewport_rows);
        let view_top = top_offset.min(self.core.scroll_manager.max_offset());
        self.core.transcript_view_top = view_top;

        let clicked_row = view_top.saturating_add(row_in_view);
        let expanded = self.expand_collapsed_paste_at_row(transcript_width, clicked_row);
        if expanded {
            self.mark_dirty();
            return expanded;
        }
        let toggled = self.toggle_thinking_block_at_row(transcript_width, clicked_row);
        if toggled {
            self.mark_dirty();
            return true;
        }
        if self.open_diff_review_from_visible_text(transcript_width, view_top, row_in_view) {
            self.mark_dirty();
            return true;
        }
        false
    }

    /// Explicit expand: click a completed-edit "review full diff" notice row.
    ///
    /// Reflow may wrap the notice across several transcript rows, so match the
    /// clicked row together with a small neighborhood rather than the single
    /// screen line alone.
    fn open_diff_review_from_visible_text(
        &mut self,
        transcript_width: u16,
        view_top: usize,
        row_in_view: usize,
    ) -> bool {
        if self.diff_review_anchors.is_empty() || transcript_width == 0 {
            return false;
        }
        let height = self.core.transcript_rows.max(1) as usize;
        let visible = self.core.collect_transcript_window_cached(transcript_width, view_top, height);
        let row_text = |index: usize| -> Option<String> {
            let line = visible.get(index)?;
            Some(line.line.spans.iter().map(|span| span.content.as_ref()).collect())
        };
        if let Some(text) = row_text(row_in_view)
            && self.open_diff_review_for_notice(&text)
        {
            return true;
        }
        // Wrapped notice fragments: scan a short window around the click.
        let start = row_in_view.saturating_sub(2);
        let end = (row_in_view + 4).min(visible.len().saturating_sub(1));
        if start > end {
            return false;
        }
        let mut window = String::new();
        for index in start..=end {
            if let Some(text) = row_text(index) {
                window.push_str(&text);
                window.push('\n');
            }
        }
        self.open_diff_review_for_notice(&window)
    }

    fn handle_input_click(&mut self, mouse_event: MouseEvent) -> bool {
        if !matches!(mouse_event.kind, MouseEventKind::Down(crossterm::event::MouseButton::Left)) {
            return false;
        }

        if self.core.background_indicator_contains(mouse_event.column, mouse_event.row) {
            if self.local_agents_visible() {
                self.close_local_agents_drawer(true);
            } else {
                self.ensure_inline_lists_visible_for_trigger();
                self.open_local_agents_drawer(false);
            }
            self.mark_dirty();
            return true;
        }

        if !self.input_area_contains(mouse_event.column, mouse_event.row) {
            return false;
        }

        let cursor_at_end = self.core.input_manager.cursor() == self.core.input_manager.content().len();
        if self.core.input_compact_mode() && cursor_at_end && self.input_compact_placeholder().is_some() {
            self.core.set_input_compact_mode(false);
            self.mark_dirty();
            return true;
        }

        if let Some(cursor) = self.cursor_index_for_input_point(mouse_event.column, mouse_event.row) {
            if self.core.input_manager.cursor() != cursor {
                self.core.input_manager.set_cursor(cursor);
                self.mark_dirty();
            }
            return true;
        }

        false
    }
}
