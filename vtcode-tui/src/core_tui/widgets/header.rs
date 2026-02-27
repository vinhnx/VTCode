use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Clear, Widget},
};

use crate::ui::tui::session::Session;

/// Widget for rendering the header area with session metadata
///
/// This widget displays:
/// - Provider and model information
/// - Reasoning mode status
/// - Tool policy summary
/// - Workspace trust level
/// - Git status
/// - Plan progress (if applicable)
/// - Suggestions or highlights
///
/// # Example
/// ```ignore
/// HeaderWidget::new(session)
///     .lines(header_lines)
///     .custom_style(style)
///     .render(area, buf);
/// ```
pub struct HeaderWidget<'a> {
    session: &'a Session,
    lines: Vec<Line<'static>>,
    custom_style: Option<Style>,
}

impl<'a> HeaderWidget<'a> {
    /// Create a new HeaderWidget with required parameters
    pub fn new(session: &'a Session) -> Self {
        Self {
            session,
            lines: Vec::new(),
            custom_style: None,
        }
    }

    /// Set the header lines to display
    #[must_use]
    pub fn lines(mut self, lines: Vec<Line<'static>>) -> Self {
        self.lines = lines;
        self
    }

    /// Set a custom style for the header
    #[must_use]
    pub fn custom_style(mut self, style: Style) -> Self {
        self.custom_style = Some(style);
        self
    }
}

impl<'a> Widget for HeaderWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        if area.height == 0 || area.width == 0 {
            return;
        }

        let paragraph = self.session.build_header_paragraph(&self.lines);
        paragraph.render(area, buf);
    }
}
