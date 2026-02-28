/// Widget for rendering the history picker overlay (Ctrl+R)
///
/// Displays a fuzzy-searchable list of command history entries in a modal-style overlay.
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Paragraph, Widget},
};

use crate::config::constants::ui;
use crate::ui::tui::session::{
    Session, history_picker::HistoryPickerState, modal::compute_modal_area, terminal_capabilities,
};
use crate::ui::tui::style::ratatui_color_from_ansi;

/// Constants for the history picker UI
const HISTORY_PICKER_TITLE: &str = "History (Ctrl+R)";
const HISTORY_PICKER_HINT: &str = "Tab Navigate · Enter Accept · Esc Cancel";
const HISTORY_PICKER_MAX_VISIBLE: usize = 10;

/// Widget for rendering the history picker overlay
pub struct HistoryPickerWidget<'a> {
    session: &'a Session,
    picker: &'a HistoryPickerState,
    viewport: Rect,
}

impl<'a> HistoryPickerWidget<'a> {
    /// Create a new HistoryPickerWidget
    pub fn new(session: &'a Session, picker: &'a HistoryPickerState, viewport: Rect) -> Self {
        Self {
            session,
            picker,
            viewport,
        }
    }

    /// Render the widget
    pub fn render(self, buf: &mut Buffer) {
        if self.viewport.height == 0 || self.viewport.width == 0 {
            return;
        }

        // Calculate dimensions
        let matches = &self.picker.matches;
        let visible_count = matches.len().min(HISTORY_PICKER_MAX_VISIBLE);

        // Calculate height: hint + items + border (more compact)
        let hint_height = 1;
        let content_height = if matches.is_empty() { 1 } else { visible_count };
        let modal_height = hint_height + content_height + 2;

        let area = compute_modal_area(self.viewport, modal_height, 0, 0, true);

        // Clear the background
        Clear.render(area, buf);

        // Create the bordered block
        let title = if self.picker.search_query.is_empty() {
            HISTORY_PICKER_TITLE.to_string()
        } else {
            format!("{} \"{}\"", HISTORY_PICKER_TITLE, self.picker.search_query)
        };

        let block = Block::bordered()
            .title(title)
            .border_type(terminal_capabilities::get_border_type())
            .style(self.session.styles.default_style())
            .border_style(self.session.styles.border_style());
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let mut y_offset = inner.y;

        // Render compact hint line
        let hint_area = Rect {
            x: inner.x,
            y: y_offset,
            width: inner.width,
            height: 1,
        };
        let hint_line = Line::from(Span::styled(
            HISTORY_PICKER_HINT,
            self.session
                .styles
                .default_style()
                .add_modifier(Modifier::DIM),
        ));
        Paragraph::new(hint_line).render(hint_area, buf);
        y_offset += 1;

        // Render matches list or empty state
        let list_height = inner.height.saturating_sub(y_offset - inner.y);
        let list_area = Rect {
            x: inner.x,
            y: y_offset,
            width: inner.width,
            height: list_height,
        };

        if matches.is_empty() {
            let empty_msg = if self.picker.search_query.is_empty() {
                "No history entries"
            } else {
                "No matches found"
            };
            let empty_line = Line::from(Span::styled(
                empty_msg,
                self.session
                    .styles
                    .default_style()
                    .add_modifier(Modifier::DIM | Modifier::ITALIC),
            ));
            Paragraph::new(empty_line).render(list_area, buf);
        } else {
            let list_items: Vec<ListItem> = matches
                .iter()
                .take(visible_count)
                .enumerate()
                .map(|(idx, m)| {
                    let is_selected = self.picker.list_state.selected() == Some(idx);

                    // Truncate long entries
                    let content: String =
                        m.content.chars().take(inner.width as usize - 4).collect();
                    let display = if m.content.len() > inner.width as usize - 4 {
                        format!("{}…", content)
                    } else {
                        content
                    };

                    let style = if is_selected {
                        self.highlight_style()
                    } else {
                        self.session.styles.default_style()
                    };

                    let prefix = if is_selected { "▸ " } else { "  " };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(display, style),
                    ]))
                })
                .collect();

            let list = List::new(list_items)
                .style(self.session.styles.default_style())
                .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
                .repeat_highlight_symbol(true);

            // Render the list (selection already handled via is_selected styling)
            Widget::render(list, list_area, buf);
        }
    }

    /// Get the highlight style for selected items
    fn highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.session.theme.primary.or(self.session.theme.secondary) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
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

    #[test]
    fn test_history_picker_widget_creation() {
        let session = create_test_session();
        let picker = HistoryPickerState::new();
        let viewport = Rect::new(0, 0, 80, 24);

        let _widget = HistoryPickerWidget::new(&session, &picker, viewport);
        assert_eq!(viewport.width, 80);
    }

    #[test]
    fn test_history_picker_render_empty() {
        let session = create_test_session();
        let mut picker = HistoryPickerState::new();
        picker.active = true;
        let viewport = Rect::new(0, 0, 80, 24);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let widget = HistoryPickerWidget::new(&session, &picker, viewport);
                widget.render(frame.buffer_mut());
            })
            .unwrap();

        // Should render without panicking
        assert!(true);
    }

    #[test]
    fn test_history_picker_render_with_matches() {
        let session = create_test_session();
        let mut picker = HistoryPickerState::new();
        picker.active = true;

        // Add some test matches
        picker.matches = vec![
            crate::ui::tui::session::history_picker::HistoryMatch {
                history_index: 0,
                content: "cargo build".to_string(),
                score: 100,
                attachments: vec![],
            },
            crate::ui::tui::session::history_picker::HistoryMatch {
                history_index: 1,
                content: "cargo test".to_string(),
                score: 90,
                attachments: vec![],
            },
        ];
        picker.list_state.select(Some(0));

        let viewport = Rect::new(0, 0, 80, 24);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let widget = HistoryPickerWidget::new(&session, &picker, viewport);
                widget.render(frame.buffer_mut());
            })
            .unwrap();

        // Should render without panicking
        assert!(true);
    }
}
