use super::*;

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

        let has_status = self
            .input_status_left
            .as_ref()
            .is_some_and(|v| !v.trim().is_empty())
            || self
                .input_status_right
                .as_ref()
                .is_some_and(|v| !v.trim().is_empty());
        let mode = crate::ui::tui::widgets::LayoutMode::from_area(viewport);
        let status_height = if viewport.width > 0 && has_status && !mode.show_footer() {
            1
        } else {
            0
        };
        let inner_width = viewport.width.saturating_sub(2);
        let desired_lines = self.desired_input_lines(inner_width);
        let block_height = Self::input_block_height_for_lines(desired_lines);
        let input_height = block_height.saturating_add(status_height);
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

        let transcript_area = main_area;
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
        render::render_modal(self, frame, viewport);
        slash::render_slash_palette(self, frame, viewport);
        render::render_config_palette(self, frame, viewport);
        render::render_file_palette(self, frame, viewport);

        // Render diff preview modal if active
        if self.diff_preview.is_some() {
            diff_preview::render_diff_preview(self, frame, viewport);
        }

        // Apply mouse text selection highlight
        if self.mouse_selection.has_selection || self.mouse_selection.is_selecting {
            self.mouse_selection
                .apply_highlight(frame.buffer_mut(), viewport);

            // Copy to clipboard via OSC 52 once when selection is finalized
            if self.mouse_selection.needs_copy() {
                let text = self
                    .mouse_selection
                    .extract_text(frame.buffer_mut(), viewport);
                if !text.is_empty() {
                    super::mouse_selection::MouseSelectionState::copy_to_clipboard_osc52(&text);
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
