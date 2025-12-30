use ratatui::{prelude::*, widgets::Clear};

use super::Session;

impl Session {
    #[allow(dead_code)]
    pub(super) fn render_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        // Navigation/ Timeline pane has been removed
    }
}
