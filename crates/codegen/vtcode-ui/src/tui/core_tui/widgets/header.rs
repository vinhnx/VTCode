use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Clear, Widget},
};

use crate::tui::ui::tui::session::Session;

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
    session: &'a mut Session,
    lines: std::sync::Arc<Vec<Line<'static>>>,
    custom_style: Option<Style>,
}

impl<'a> HeaderWidget<'a> {
    /// Create a new HeaderWidget with required parameters
    pub(crate) fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            lines: std::sync::Arc::new(Vec::new()),
            custom_style: None,
        }
    }

    /// Set the header lines to display
    #[must_use]
    pub(crate) fn lines(mut self, lines: std::sync::Arc<Vec<Line<'static>>>) -> Self {
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
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        if area.height == 0 || area.width == 0 {
            return;
        }

        // Paint header rows via set_span — avoids Paragraph wrap + title
        // rebuild every frame (hotpath: header::render was ~22KB/frame).
        let hide_header = self.session.appearance.hide_header;
        let text_style = self.session.header_primary_style().add_modifier(Modifier::DIM);

        if hide_header {
            // Paragraph used text_style as the base for the whole area.
            buf.set_style(area, text_style);
            for (y, line) in (area.y..).zip(self.lines.iter().take(usize::from(area.height))) {
                let mut x = area.x;
                for span in &line.spans {
                    if x >= area.right() {
                        break;
                    }
                    let merged = text_style.patch(span.style);
                    let (end_x, _) =
                        buf.set_stringn(x, y, span.content.as_ref(), area.right().saturating_sub(x) as usize, merged);
                    x = end_x;
                }
            }
            return;
        }

        let mut border_style = Style::default();
        if let Some(accent) = self
            .session
            .theme
            .tool_accent
            .or(self.session.theme.primary)
            .or(self.session.theme.foreground)
        {
            border_style = border_style.fg(crate::tui::core_tui::style::ratatui_color_from_ansi(accent));
        }
        let title = self.session.header_block_title_cached();
        let block = ratatui::widgets::Block::bordered()
            .title(title)
            .border_type(crate::tui::core_tui::session::terminal_capabilities::get_border_type())
            .border_style(border_style)
            .style(self.session.styles.default_style());
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }
        for (y, line) in (inner.y..).zip(self.lines.iter().take(usize::from(inner.height))) {
            let mut x = inner.x;
            for span in &line.spans {
                if x >= inner.right() {
                    break;
                }
                // Paragraph: text_style is the base, span style patches on top.
                let merged = text_style.patch(span.style);
                let (end_x, _) =
                    buf.set_stringn(x, y, span.content.as_ref(), inner.right().saturating_sub(x) as usize, merged);
                x = end_x;
            }
        }
    }
}
