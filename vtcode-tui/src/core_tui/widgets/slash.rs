use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Paragraph, Widget, Wrap},
};

use crate::config::constants::ui;
use crate::ui::tui::session::{
    Session,
    modal::compute_modal_area,
    slash_palette::{SlashPalette, SlashPaletteSuggestion},
    terminal_capabilities,
};
use crate::ui::tui::style::{
    measure_text_width, ratatui_color_from_ansi, ratatui_style_from_inline,
};
use crate::ui::tui::types::InlineTextStyle;

/// Widget for rendering the slash command palette
///
/// # Example
/// ```ignore
/// SlashWidget::new(session, palette, viewport)
///     .highlight_style(accent_style)
///     .render(area, buf);
/// ```
pub struct SlashWidget<'a> {
    session: &'a Session,
    palette: &'a SlashPalette,
    viewport: Rect,
    highlight_style: Option<Style>,
}

impl<'a> SlashWidget<'a> {
    /// Create a new SlashWidget with required parameters
    pub fn new(session: &'a Session, palette: &'a SlashPalette, viewport: Rect) -> Self {
        Self {
            session,
            palette,
            viewport,
            highlight_style: None,
        }
    }

    /// Set a custom highlight style
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = Some(style);
        self
    }
}

impl<'a> Widget for SlashWidget<'a> {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        if self.viewport.height == 0 || self.viewport.width == 0 || self.palette.is_empty() {
            return;
        }

        let suggestions = self.palette.suggestions();
        if suggestions.is_empty() {
            return;
        }

        // Calculate width hint based on the longest suggestion
        let mut width_hint = measure_text_width(ui::SLASH_PALETTE_HINT_PRIMARY);
        width_hint = width_hint.max(measure_text_width(ui::SLASH_PALETTE_HINT_SECONDARY));

        for suggestion in suggestions.iter().take(ui::SLASH_SUGGESTION_LIMIT) {
            let label = match suggestion {
                SlashPaletteSuggestion::Static(cmd) => {
                    if !cmd.description.is_empty() {
                        format!("/{} {}", cmd.name, cmd.description)
                    } else {
                        format!("/ {}", cmd.name)
                    }
                }
            };
            width_hint = width_hint.max(measure_text_width(&label));
        }

        let instructions = self.instructions();
        let modal_height = suggestions.len() + instructions.len() + 2;
        let area = compute_modal_area(self.viewport, width_hint, modal_height, 0, 0, true);

        // Clear the background
        Clear.render(area, buf);

        // Create the bordered block with title
        let block = Block::bordered()
            .title("Slash Commands")
            .border_type(terminal_capabilities::get_border_type())
            .style(self.session.styles.default_style())
            .border_style(self.session.styles.border_style());
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Render instructions at the top
        let inst_height = instructions.len().min(inner.height as usize);
        if inst_height > 0 {
            let inst_area = Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: inst_height as u16,
            };
            let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
            paragraph.render(inst_area, buf);
        }

        // Render the slash command list
        let list_y = inner.y + inst_height as u16;
        let list_height = inner.height.saturating_sub(inst_height as u16);
        if list_height > 0 {
            let list_area = Rect {
                x: inner.x,
                y: list_y,
                width: inner.width,
                height: list_height,
            };

            let list_items = self.create_list_items(suggestions);
            let list = List::new(list_items)
                .style(self.session.styles.default_style())
                .highlight_style(
                    self.highlight_style
                        .unwrap_or_else(|| self.slash_highlight_style()),
                )
                .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
                .repeat_highlight_symbol(true);

            // Render the list - since we can't get mutable access to list state,
            // we'll render without stateful highlighting and let the caller handle selection
            list.render(list_area, buf);
        }
    }
}

impl<'a> SlashWidget<'a> {
    /// Create list items from slash palette suggestions
    fn create_list_items(&self, suggestions: &[SlashPaletteSuggestion]) -> Vec<ListItem<'static>> {
        suggestions
            .iter()
            .map(|suggestion| match suggestion {
                SlashPaletteSuggestion::Static(command) => ListItem::new(Line::from(vec![
                    Span::styled(format!("/ {}", command.name), self.slash_name_style()),
                    Span::raw(" "),
                    Span::styled(
                        command.description.to_owned(),
                        self.slash_description_style(),
                    ),
                ])),
            })
            .collect()
    }

    /// Create instructions for the slash palette
    fn instructions(&self) -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled(
                ui::SLASH_PALETTE_HINT_PRIMARY.to_owned(),
                self.session.styles.default_style(),
            )),
            Line::from(Span::styled(
                ui::SLASH_PALETTE_HINT_SECONDARY.to_owned(),
                self.session
                    .styles
                    .default_style()
                    .add_modifier(Modifier::DIM),
            )),
        ]
    }

    /// Get the highlight style for selected items
    fn slash_highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.session.theme.primary.or(self.session.theme.secondary) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    /// Get the style for command names
    fn slash_name_style(&self) -> Style {
        let style = InlineTextStyle::default()
            .bold()
            .with_color(self.session.theme.primary.or(self.session.theme.foreground));
        ratatui_style_from_inline(&style, self.session.theme.foreground)
    }

    /// Get the style for command descriptions
    fn slash_description_style(&self) -> Style {
        self.session
            .styles
            .default_style()
            .add_modifier(Modifier::DIM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tui::InlineTheme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn create_test_session() -> Session {
        let theme = InlineTheme::default();
        Session::new(theme, None, 24)
    }

    fn create_test_palette() -> SlashPalette {
        let mut palette = SlashPalette::new();
        // Add some test suggestions
        palette.update(Some("he"), 5);
        palette
    }

    #[test]
    fn test_slash_widget_creation() {
        let session = create_test_session();
        let palette = create_test_palette();
        let viewport = ratatui::layout::Rect::new(0, 0, 80, 24);

        let _widget = SlashWidget::new(&session, &palette, viewport);

        // Widget should be created successfully
        assert_eq!(viewport.width, 80);
        assert_eq!(viewport.height, 24);
    }

    #[test]
    fn test_slash_widget_render_empty() {
        let session = create_test_session();
        let palette = SlashPalette::new(); // Empty palette
        let viewport = ratatui::layout::Rect::new(0, 0, 80, 24);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let widget = SlashWidget::new(&session, &palette, viewport);
                widget.render(viewport, frame.buffer_mut());
            })
            .unwrap();

        // Should render without panicking even with empty palette
        assert!(true);
    }

    #[test]
    fn test_slash_widget_render_with_suggestions() {
        let session = create_test_session();
        let mut palette = SlashPalette::new();
        palette.update(Some(""), 5); // This should populate with some default suggestions
        let viewport = ratatui::layout::Rect::new(0, 0, 80, 24);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let widget = SlashWidget::new(&session, &palette, viewport);
                widget.render(viewport, frame.buffer_mut());
            })
            .unwrap();

        // Should render without panicking with suggestions
        assert!(true);
    }
}
