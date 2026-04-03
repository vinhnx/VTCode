use super::*;

impl Session {
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let Some(layout) = self.prepare_frame_layout(frame, 0) else {
            return;
        };

        let (transcript_area, modal_area) = render::split_inline_modal_area(self, layout.main_area);
        self.set_modal_list_area(None);
        self.set_bottom_panel_area(None);
        self.render_base_frame(frame, &layout, transcript_area);
        self.render_input(frame, layout.input_area);
        if let Some(modal_area) = modal_area {
            render::render_modal(self, frame, modal_area);
        } else {
            render::render_modal(self, frame, layout.viewport);
        }
        self.finalize_mouse_selection(frame, layout.viewport);
    }

    #[allow(dead_code)]
    pub(crate) fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Span::raw(String::new())];
        };
        self.render_message_spans_for_line(line)
    }

    pub(crate) fn render_message_spans_for_line(&self, line: &MessageLine) -> Vec<Span<'static>> {
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
