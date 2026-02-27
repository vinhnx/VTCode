use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Paragraph, Widget, Wrap},
};

use crate::config::constants::ui;
use crate::ui::tui::session::{
    Session, file_palette::FilePalette, modal::compute_modal_area, terminal_capabilities,
};
use crate::ui::tui::style::measure_text_width;

/// Widget for rendering the file browser palette
///
/// # Example
/// ```ignore
/// FilePaletteWidget::new(session, palette, viewport)
///     .highlight_style(accent_style)
///     .render(area, buf);
/// ```
pub struct FilePaletteWidget<'a> {
    session: &'a Session,
    palette: &'a FilePalette,
    viewport: Rect,
    highlight_style: Option<Style>,
}

impl<'a> FilePaletteWidget<'a> {
    /// Create a new FilePaletteWidget with required parameters
    pub fn new(session: &'a Session, palette: &'a FilePalette, viewport: Rect) -> Self {
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

impl<'a> Widget for FilePaletteWidget<'a> {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        if self.viewport.height == 0 || self.viewport.width == 0 {
            return;
        }

        // Show loading state if no files loaded yet
        if !self.palette.has_files() {
            self.render_loading(buf);
            return;
        }

        let items = self.palette.current_page_items();
        if items.is_empty() && self.palette.filter_query().is_empty() {
            return;
        }

        // Calculate width hint
        let mut width_hint = 40u16;
        for (_, entry, _) in &items {
            let display = if entry.is_dir {
                format!("{}/ ", entry.display_name)
            } else {
                entry.display_name.clone()
            };
            width_hint = width_hint.max(measure_text_width(&display) + 4);
        }

        let instructions = self.instructions();
        let modal_height = items.len()
            + instructions.len()
            + 2
            + if self.palette.has_more_items() { 1 } else { 0 };
        let area = compute_modal_area(self.viewport, width_hint, modal_height, 0, 0, true);

        Clear.render(area, buf);
        let block = Block::bordered()
            .title(format!(
                "File Browser (Page {}/{})",
                self.palette.current_page_number(),
                self.palette.total_pages()
            ))
            .border_type(terminal_capabilities::get_border_type())
            .style(self.session.styles.default_style())
            .border_style(self.session.styles.border_style());
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Render instructions
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

        // Render file list
        let list_y = inner.y + inst_height as u16;
        let list_height = inner.height.saturating_sub(inst_height as u16);
        if list_height > 0 {
            let list_area = Rect {
                x: inner.x,
                y: list_y,
                width: inner.width,
                height: list_height,
            };

            let mut list_items: Vec<ListItem> = items
                .iter()
                .map(|(_, entry, is_selected)| {
                    let base_style = if *is_selected {
                        self.session.styles.modal_list_highlight_style()
                    } else {
                        self.session.styles.default_style()
                    };

                    let mut style = base_style;
                    let (prefix, is_dir) = if entry.is_dir {
                        ("↳  ", true)
                    } else {
                        ("  · ", false)
                    };

                    if is_dir {
                        style = style.add_modifier(Modifier::BOLD);
                    }

                    let display = format!(
                        "{}{}",
                        prefix,
                        if entry.is_dir {
                            format!("{}/", entry.display_name)
                        } else {
                            entry.display_name.clone()
                        }
                    );
                    ListItem::new(Line::from(display).style(style))
                })
                .collect();

            if self.palette.has_more_items() {
                let continuation_style = self
                    .session
                    .styles
                    .default_style()
                    .add_modifier(Modifier::DIM | Modifier::ITALIC);
                list_items.push(ListItem::new(Line::from(Span::styled(
                    format!(
                        "  ... ({} more items)",
                        self.palette
                            .total_items()
                            .saturating_sub(self.palette.current_page_number() * 20)
                    ),
                    continuation_style,
                ))));
            }

            let list = List::new(list_items)
                .style(self.session.styles.default_style())
                .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
                .repeat_highlight_symbol(true);
            list.render(list_area, buf);
        }
    }
}

impl<'a> FilePaletteWidget<'a> {
    fn render_loading(&self, buf: &mut Buffer) {
        let width_hint = 40u16;
        let modal_height = 3;
        let area = compute_modal_area(self.viewport, width_hint, modal_height, 0, 0, true);

        Clear.render(area, buf);
        let block = Block::bordered()
            .title("File Browser")
            .border_type(terminal_capabilities::get_border_type())
            .style(self.session.styles.default_style())
            .border_style(self.session.styles.border_style());
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height > 0 && inner.width > 0 {
            let loading_text = vec![Line::from(Span::styled(
                "Loading workspace files...".to_owned(),
                self.session
                    .styles
                    .default_style()
                    .add_modifier(Modifier::DIM),
            ))];
            let paragraph = Paragraph::new(loading_text).wrap(Wrap { trim: true });
            paragraph.render(inner, buf);
        }
    }

    fn instructions(&self) -> Vec<Line<'static>> {
        let mut lines = vec![];

        if self.palette.is_empty() {
            lines.push(Line::from(Span::styled(
                "No files found matching filter".to_owned(),
                self.session
                    .styles
                    .default_style()
                    .add_modifier(Modifier::DIM),
            )));
        } else {
            let total = self.palette.total_items();
            let count_text = if total == 1 {
                "1 file".to_owned()
            } else {
                format!("{} files", total)
            };

            let nav_text = "↑↓ Navigate · PgUp/PgDn Page · Tab/Enter Select";

            lines.push(Line::from(vec![Span::styled(
                format!("{} · Esc Close", nav_text),
                self.session.styles.default_style(),
            )]));

            lines.push(Line::from(vec![
                Span::styled(
                    format!("Showing {}", count_text),
                    self.session
                        .styles
                        .default_style()
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    if !self.palette.filter_query().is_empty() {
                        format!(" matching '{}'", self.palette.filter_query())
                    } else {
                        String::new()
                    },
                    self.session.styles.accent_style(),
                ),
            ]));
        }

        lines
    }
}
