//! Rendered layout areas shared by drawing, scrolling, and hit-testing.

use ratatui::layout::Rect;

#[derive(Debug, Default)]
pub(super) struct SessionAreas {
    transcript: Option<Rect>,
    progress: Option<Rect>,
    progress_feedback: Option<Rect>,
    input: Option<Rect>,
    bottom_panel: Option<Rect>,
    modal_list: Option<Rect>,
    modal_text: Vec<Rect>,
}

impl SessionAreas {
    pub(super) fn progress(&self) -> Option<Rect> {
        self.progress
    }

    pub(super) fn set_progress(&mut self, area: Option<Rect>) {
        self.progress = area;
    }

    pub(super) fn progress_feedback(&self) -> Option<Rect> {
        self.progress_feedback
    }

    pub(super) fn set_progress_feedback(&mut self, area: Option<Rect>) {
        self.progress_feedback = area;
    }

    pub(super) fn transcript(&self) -> Option<Rect> {
        self.transcript
    }

    pub(super) fn set_transcript(&mut self, area: Option<Rect>) {
        self.transcript = area;
    }

    pub(super) fn input(&self) -> Option<Rect> {
        self.input
    }

    pub(super) fn set_input(&mut self, area: Option<Rect>) {
        self.input = area;
    }

    pub(super) fn bottom_panel(&self) -> Option<Rect> {
        self.bottom_panel
    }

    pub(super) fn set_bottom_panel(&mut self, area: Option<Rect>) {
        self.bottom_panel = area;
    }

    pub(super) fn modal_list(&self) -> Option<Rect> {
        self.modal_list
    }

    pub(super) fn set_modal_list(&mut self, area: Option<Rect>) {
        self.modal_list = area;
    }

    pub(super) fn modal_text(&self) -> &[Rect] {
        &self.modal_text
    }

    pub(super) fn set_modal_text(&mut self, areas: Vec<Rect>) {
        self.modal_text = areas;
    }

    pub(super) fn clear_modal_areas(&mut self) {
        self.modal_list = None;
        self.modal_text.clear();
    }
}
