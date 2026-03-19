use super::*;

#[derive(Clone)]
pub(crate) struct SessionFrameMetrics {
    pub(crate) header_lines: Vec<Line<'static>>,
    pub(crate) header_height: u16,
    pub(crate) input_core_height: u16,
}

#[derive(Clone)]
pub(crate) struct SessionFrameLayout {
    pub(crate) viewport: Rect,
    pub(crate) header_lines: Vec<Line<'static>>,
    pub(crate) header_area: Rect,
    pub(crate) main_area: Rect,
    pub(crate) input_area: Rect,
}

impl Session {
    pub(crate) fn begin_frame(&mut self, frame: &mut Frame<'_>) -> Option<Rect> {
        let viewport = frame.area();
        if viewport.height == 0 || viewport.width == 0 {
            return None;
        }

        if self.needs_full_clear {
            frame.render_widget(Clear, viewport);
            self.needs_full_clear = false;
        }

        Some(viewport)
    }

    pub(crate) fn measure_frame(&mut self, viewport: Rect) -> SessionFrameMetrics {
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

        SessionFrameMetrics {
            header_lines,
            header_height,
            input_core_height,
        }
    }

    pub(crate) fn build_frame_layout(
        &mut self,
        viewport: Rect,
        metrics: SessionFrameMetrics,
        extra_bottom_height: u16,
    ) -> SessionFrameLayout {
        let input_height = metrics
            .input_core_height
            .saturating_add(extra_bottom_height);
        self.apply_input_height(input_height);

        let segments = Layout::vertical([
            Constraint::Length(metrics.header_height),
            Constraint::Min(1),
            Constraint::Length(input_height),
        ])
        .split(viewport);

        SessionFrameLayout {
            viewport,
            header_lines: metrics.header_lines,
            header_area: segments[0],
            main_area: segments[1],
            input_area: segments[2],
        }
    }

    pub(crate) fn prepare_frame_layout(
        &mut self,
        frame: &mut Frame<'_>,
        extra_bottom_height: u16,
    ) -> Option<SessionFrameLayout> {
        let viewport = self.begin_frame(frame)?;
        let metrics = self.measure_frame(viewport);
        Some(self.build_frame_layout(viewport, metrics, extra_bottom_height))
    }

    pub(crate) fn render_base_frame(
        &mut self,
        frame: &mut Frame<'_>,
        layout: &SessionFrameLayout,
        transcript_area: Rect,
    ) {
        let navigation_area = Rect::new(layout.main_area.x, layout.main_area.y, 0, 0);

        SessionWidget::new(self)
            .header_lines(layout.header_lines.clone())
            .header_area(layout.header_area)
            .transcript_area(transcript_area)
            .navigation_area(navigation_area)
            .render(layout.viewport, frame.buffer_mut());
    }

    pub(crate) fn finalize_mouse_selection(&mut self, frame: &mut Frame<'_>, viewport: Rect) {
        if !self.mouse_selection.has_selection && !self.mouse_selection.is_selecting {
            return;
        }

        self.mouse_selection
            .apply_highlight(frame.buffer_mut(), viewport);

        if self.mouse_selection.has_copy_request() || self.mouse_selection.needs_copy() {
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
