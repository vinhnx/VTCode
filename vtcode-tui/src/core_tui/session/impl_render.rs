use super::*;
use super::layout::{BottomPanelKind, resolve_bottom_panel_spec, split_input_and_bottom_panel_area};

impl Session {
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let viewport = frame.area();
        if viewport.height == 0 || viewport.width == 0 {
            return;
        }

        // Clear entire frame if modal was just closed to remove artifacts
        if self.needs_full_clear {
            frame.render_widget(Clear, viewport);
            self.needs_full_clear = false;
        }

        // Calculate layout constraints
        let header_lines = self.header_lines();
        let header_height = self.header_height_from_lines(viewport.width, &header_lines);
        if header_height != self.header_rows {
            self.header_rows = header_height;
            self.recalculate_transcript_rows();
        }

        let inner_width = viewport.width.saturating_sub(2);
        let desired_lines = self.desired_input_lines(inner_width);
        let block_height = Self::input_block_height_for_lines(desired_lines);
        let status_height = ui::INLINE_INPUT_STATUS_HEIGHT;
        let input_core_height = block_height.saturating_add(status_height);
        let panel = resolve_bottom_panel_spec(self, viewport, header_height, input_core_height);
        let input_height = input_core_height.saturating_add(panel.height);
        self.apply_input_height(input_height);

        let mut constraints = vec![Constraint::Length(header_height), Constraint::Min(1)];
        constraints.push(Constraint::Length(input_height));

        let segments = Layout::vertical(constraints).split(viewport);

        let header_area = segments[0];
        let main_area = segments[1];
        let input_index = segments.len().saturating_sub(1);
        let input_area = segments[input_index];

        let _available_width = main_area.width;
        let _horizontal_minimum = ui::INLINE_CONTENT_MIN_WIDTH + ui::INLINE_NAVIGATION_MIN_WIDTH;

        let modal_in_bottom = matches!(panel.kind, BottomPanelKind::InlineModal);
        let (transcript_area, modal_area) = if modal_in_bottom {
            (main_area, None)
        } else {
            render::split_inline_modal_area(self, main_area)
        };
        self.set_modal_list_area(None);
        let (input_area, bottom_panel_area) =
            split_input_and_bottom_panel_area(input_area, panel.height);
        self.set_bottom_panel_area(bottom_panel_area);
        let navigation_area = Rect::new(main_area.x, main_area.y, 0, 0); // No navigation area since timeline pane is removed

        // Use SessionWidget for buffer-based rendering (header, transcript, overlays)
        SessionWidget::new(self)
            .header_lines(header_lines.clone())
            .header_area(header_area)
            .transcript_area(transcript_area)
            .navigation_area(navigation_area) // Pass empty navigation area
            .render(viewport, frame.buffer_mut());

        // Handle frame-based rendering for components that need it
        // Note: header, transcript, and overlays are handled by SessionWidget
        // Timeline pane has been removed, so no navigation rendering
        self.render_input(frame, input_area);
        let mut rendered_bottom_modal = false;
        if !modal_in_bottom {
            if let Some(modal_area) = modal_area {
                render::render_modal(self, frame, modal_area);
            } else {
                render::render_modal(self, frame, viewport);
            }
        }
        if let Some(panel_area) = bottom_panel_area {
            match panel.kind {
                BottomPanelKind::InlineModal => {
                    frame.render_widget(Clear, panel_area);
                    render::render_modal(self, frame, panel_area);
                    rendered_bottom_modal = true;
                }
                BottomPanelKind::FilePalette => {
                    render::render_file_palette(self, frame, panel_area);
                }
                BottomPanelKind::HistoryPicker => {
                    render::render_history_picker(self, frame, panel_area);
                }
                BottomPanelKind::SlashPalette => {
                    slash::render_slash_palette(self, frame, panel_area);
                }
                BottomPanelKind::TaskPanel => {
                    render_task_panel(self, frame, panel_area);
                }
                BottomPanelKind::None => {
                    frame.render_widget(Clear, panel_area);
                }
            }
        }
        if modal_in_bottom && !rendered_bottom_modal {
            render::render_modal(self, frame, viewport);
        }

        // Render diff preview modal if active
        if self.diff_preview_state().is_some() {
            diff_preview::render_diff_preview(self, frame, viewport);
        }

        // Apply mouse text selection highlight
        if self.mouse_selection.has_selection || self.mouse_selection.is_selecting {
            self.mouse_selection
                .apply_highlight(frame.buffer_mut(), viewport);

            // Copy to clipboard once when selection is finalized
            if self.mouse_selection.needs_copy() {
                let text = self
                    .mouse_selection
                    .extract_text(frame.buffer_mut(), viewport);
                if !text.is_empty() {
                    MouseSelectionState::copy_to_clipboard(&text);
                }
                self.mouse_selection.mark_copied();
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Span::raw(String::new())];
        };
        message_renderer::render_message_spans(
            line,
            &self.theme,
            &self.labels,
            |kind| self.prefix_text(kind),
            |line| self.prefix_style(line),
            |kind| self.text_fallback(kind),
        )
    }
}

fn render_task_panel(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let rows = if session.task_panel_lines.is_empty() {
        vec![(
            inline_list::InlineListRow::single(
                ui::PLAN_STATUS_EMPTY.to_string().into(),
                session.header_secondary_style(),
            ),
            1,
        )]
    } else {
        session
            .task_panel_lines
            .iter()
            .map(|line| {
                (
                    inline_list::InlineListRow::single(
                        line.clone().into(),
                        session.header_secondary_style(),
                    ),
                    1,
                )
            })
            .collect()
    };
    let item_count = session.task_panel_lines.len();
    let sections = list_panel::SharedListPanelSections {
        header: vec![Line::from(vec![Span::styled(
            ui::PLAN_BLOCK_TITLE.to_string(),
            session.section_title_style(),
        )])],
        info: vec![Line::from(format!(
            "{} item{}",
            item_count,
            if item_count == 1 { "" } else { "s" }
        ))],
        search: None,
    };
    let styles = list_panel::SharedListPanelStyles {
        base_style: session.styles.default_style(),
        selected_style: Some(session.styles.modal_list_highlight_style()),
        text_style: session.header_secondary_style(),
    };
    let mut model = list_panel::StaticRowsListPanelModel {
        rows,
        selected: None,
        offset: 0,
        visible_rows: area.height as usize,
    };
    list_panel::render_shared_list_panel(frame, area, sections, styles, &mut model);
}
