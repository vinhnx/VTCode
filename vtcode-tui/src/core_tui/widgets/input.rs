use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::Line,
    widgets::{Clear, Paragraph, Widget, Wrap},
};

use crate::ui::tui::session::Session;

/// Widget for rendering the input area with user text entry
///
/// This widget handles:
/// - Input area rendering with a subtle background shade and padding
/// - Status line rendering below the input box
/// - Cursor positioning using buffer capabilities
/// - Text content from session's input widget data
///
/// # Example
/// ```ignore
/// InputWidget::new(session)
///     .area(area)
///     .render(area, buf);
/// ```
pub struct InputWidget<'a> {
    session: &'a mut Session,
    area: Option<Rect>,
}

impl<'a> InputWidget<'a> {
    /// Create a new InputWidget with required parameters
    pub fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            area: None,
        }
    }

    /// Set the area for rendering (used for cursor positioning calculations)
    #[must_use]
    pub fn area(mut self, area: Rect) -> Self {
        self.area = Some(area);
        self
    }
}

impl<'a> Widget for InputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        if area.height == 0 {
            return;
        }

        let mut input_area = area;
        let mut status_area = None;
        let mut status_line = None;

        // Calculate areas for input and status line
        if area.height >= 2
            && let Some(spans) = self.session.build_input_status_widget_data(area.width)
        {
            let block_height = area.height.saturating_sub(1).max(1);
            input_area.height = block_height;
            status_area = Some(Rect::new(area.x, area.y + block_height, area.width, 1));
            status_line = Some(Line::from(spans));
        }

        let temp_data = self.session.build_input_widget_data(1, 1);
        let shell_mode_title = self.session.shell_mode_border_title();
        let mut block = if shell_mode_title.is_some() {
            ratatui::widgets::Block::bordered()
        } else {
            ratatui::widgets::Block::new()
        };
        block = block
            .style(temp_data.background_style)
            .padding(self.session.input_block_padding());
        if let Some(title) = shell_mode_title {
            block = block
                .title(title)
                .border_type(crate::ui::tui::session::terminal_capabilities::get_border_type())
                .border_style(
                    self.session
                        .styles
                        .accent_style()
                        .add_modifier(Modifier::BOLD),
                );
        }
        let inner = block.inner(input_area);
        let input_data = self
            .session
            .build_input_widget_data(inner.width, inner.height);

        let paragraph = Paragraph::new(input_data.text)
            .style(input_data.background_style)
            .wrap(Wrap { trim: false })
            .block(block);
        paragraph.render(input_area, buf);

        // Handle cursor positioning using buffer API
        if input_data.cursor_should_be_visible && inner.width > 0 && inner.height > 0 {
            let cursor_x = input_data
                .cursor_x
                .min(inner.width.saturating_sub(1))
                .saturating_add(inner.x);
            let cursor_y = input_data
                .cursor_y
                .min(inner.height.saturating_sub(1))
                .saturating_add(inner.y);

            if let Some(cell) = buf.cell_mut((cursor_x, cursor_y)) {
                if input_data.use_fake_cursor {
                    let mut style = cell.style();
                    style = style.add_modifier(Modifier::REVERSED);
                    cell.set_style(style);
                    if cell.symbol().is_empty() {
                        cell.set_symbol(" ");
                    }
                } else {
                    cell.set_symbol("");
                    // The cursor position is managed by the terminal, we just need to ensure
                    // the cell is accessible for cursor placement
                }
            }
        }

        // Render status line if present
        if let (Some(status_rect), Some(line)) = (status_area, status_line) {
            let paragraph = Paragraph::new(line)
                .style(input_data.default_style)
                .wrap(Wrap { trim: false });
            paragraph.render(status_rect, buf);
        }
    }
}
