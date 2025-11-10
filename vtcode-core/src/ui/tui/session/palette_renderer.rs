/// Generic palette renderer for displaying paginated lists of items
///
/// This module provides a trait-based design for rendering different types of palettes
/// (files, prompts, etc.) with consistent UI/UX. It reduces code duplication between
/// file and prompt palette rendering by providing a generic `PaletteRenderer<T>` that
/// works with any item type implementing `PaletteItem`.
///
/// # Architecture
///
/// The key components are:
/// - `PaletteItem`: Trait that items must implement to be renderable
/// - `PaletteRenderer<T>`: Generic renderer that works with any `PaletteItem`
/// - Helper functions for creating consistent UI elements
///
/// # Example
///
/// ```ignore
/// // Implement the trait for your item type
/// impl PaletteItem for MyEntry {
///     fn display_name(&self) -> String { /* ... */ }
/// }
///
/// // Use the generic renderer
/// let renderer = PaletteRenderer::new(
///     "My Palette".to_string(),
///     items,
///     selected_index,
/// );
/// renderer.render(frame, viewport, theme);
/// ```
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use super::super::style::measure_text_width;
use super::modal::{ModalListLayout, compute_modal_area};

/// Trait for items that can be displayed in a palette
///
/// Implementing types must provide basic display information that the generic
/// renderer uses to create consistent UI.
pub trait PaletteItem {
    /// Human-readable name to display in the palette
    fn display_name(&self) -> String;

    /// Optional icon or prefix to display before the name
    fn display_icon(&self) -> Option<String> {
        None
    }

    /// Optional style to apply to this item
    /// If not provided, uses the default style
    fn style(&self) -> Option<Style> {
        None
    }

    /// Whether this is a directory/container (for visual distinction)
    fn is_directory(&self) -> bool {
        false
    }
}

/// Configuration for palette rendering
#[derive(Clone)]
pub struct PaletteConfig {
    /// Title to display in the palette header
    pub title: String,
    /// Current page number (1-indexed)
    pub current_page: usize,
    /// Total number of pages
    pub total_pages: usize,
    /// Total number of items
    pub total_items: usize,
    /// Whether more items exist beyond current page
    pub has_more_items: bool,
    /// Filter query currently applied
    pub filter_query: String,
}

impl PaletteConfig {
    pub fn new(title: String) -> Self {
        Self {
            title,
            current_page: 1,
            total_pages: 1,
            total_items: 0,
            has_more_items: false,
            filter_query: String::new(),
        }
    }
}

/// Generic renderer for palette items
///
/// This renderer handles all the common UI rendering logic for displaying paginated
/// lists of items. It works with any type implementing `PaletteItem`, eliminating
/// the need for duplicate rendering code.
pub struct PaletteRenderer<T: PaletteItem> {
    config: PaletteConfig,
    items: Vec<(usize, T, bool)>, // (index, item, is_selected)
    instructions_fn: Box<dyn Fn() -> Vec<Line<'static>>>,
}

impl<T: PaletteItem> PaletteRenderer<T> {
    pub fn new(config: PaletteConfig, items: Vec<(usize, T, bool)>) -> Self {
        Self {
            config,
            items,
            instructions_fn: Box::new(|| vec![]),
        }
    }

    /// Set a custom function to generate instruction lines
    pub fn with_instructions_fn<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Vec<Line<'static>> + 'static,
    {
        self.instructions_fn = Box::new(f);
        self
    }

    /// Render the palette to the frame
    pub fn render(
        &self,
        frame: &mut Frame,
        viewport: Rect,
        default_style: Style,
        highlight_style: Style,
        _accent_style: Style,
        border_style: Style,
    ) {
        if viewport.height == 0 || viewport.width == 0 {
            return;
        }

        // Calculate dimensions
        let mut width_hint = 40u16;
        for (_, item, _) in &self.items {
            let item_width = measure_text_width(&item.display_name());
            width_hint = width_hint.max(item_width + 4);
        }

        let instructions = (self.instructions_fn)();
        let modal_height = self.items.len()
            + instructions.len()
            + 2 // borders
            + if self.config.has_more_items { 1 } else { 0 };

        let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

        // Render background clear
        frame.render_widget(Clear, area);

        // Render block border
        let title = format!(
            "{} (Page {}/{})",
            self.config.title, self.config.current_page, self.config.total_pages
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(default_style)
            .border_style(border_style);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Render instructions
        let layout = ModalListLayout::new(inner, instructions.len());
        if let Some(text_area) = layout.text_area {
            let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, text_area);
        }

        // Render list items
        let mut list_items: Vec<ListItem> = self
            .items
            .iter()
            .map(|(_, item, is_selected)| {
                let mut item_style = if *is_selected {
                    highlight_style
                } else {
                    default_style
                };

                // Apply item-specific style if available
                if let Some(custom_style) = item.style() {
                    item_style = custom_style;
                }

                // Add bold for directories
                if item.is_directory() {
                    item_style = item_style.add_modifier(Modifier::BOLD);
                }

                // Build display text with icon
                let icon = item.display_icon().unwrap_or_else(|| {
                    if item.is_directory() {
                        "↳  ".to_string()
                    } else {
                        "  · ".to_string()
                    }
                });

                let display_text = format!("{}{}", icon, item.display_name());
                ListItem::new(Line::from(Span::styled(display_text, item_style)))
            })
            .collect();

        // Add continuation indicator
        if self.config.has_more_items {
            let remaining = self.config.total_items
                - (self.config.current_page * 20).min(self.config.total_items);
            let continuation_text = format!("  ... ({} more items)", remaining);
            let continuation_style = default_style.add_modifier(Modifier::DIM | Modifier::ITALIC);
            list_items.push(ListItem::new(Line::from(Span::styled(
                continuation_text,
                continuation_style,
            ))));
        }

        let list = List::new(list_items).style(default_style);
        frame.render_widget(list, layout.list_area);
    }
}

/// Helper to render loading state for palettes
pub fn render_palette_loading(
    frame: &mut Frame,
    viewport: Rect,
    title: &str,
    loading_message: &str,
    default_style: Style,
    border_style: Style,
) {
    if viewport.height == 0 || viewport.width == 0 {
        return;
    }

    let width_hint = 40u16;
    let modal_height = 3;
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(default_style)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height > 0 && inner.width > 0 {
        let loading_text = vec![Line::from(Span::styled(
            loading_message.to_string(),
            default_style.add_modifier(Modifier::DIM),
        ))];
        let paragraph = Paragraph::new(loading_text).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestItem {
        name: String,
        is_dir: bool,
    }

    impl PaletteItem for TestItem {
        fn display_name(&self) -> String {
            self.name.clone()
        }

        fn display_icon(&self) -> Option<String> {
            Some(if self.is_dir { "📁 " } else { "📄 " }.to_string())
        }

        fn is_directory(&self) -> bool {
            self.is_dir
        }
    }

    #[test]
    fn palette_renderer_builds_correctly() {
        let config = PaletteConfig::new("Test Palette".to_string());
        let items = vec![
            (
                0,
                TestItem {
                    name: "file.txt".to_string(),
                    is_dir: false,
                },
                false,
            ),
            (
                1,
                TestItem {
                    name: "folder".to_string(),
                    is_dir: true,
                },
                true,
            ),
        ];

        let renderer = PaletteRenderer::new(config, items);
        assert_eq!(renderer.config.title, "Test Palette");
        assert_eq!(renderer.items.len(), 2);
    }

    #[test]
    fn palette_config_initializes_correctly() {
        let config = PaletteConfig::new("My Items".to_string());
        assert_eq!(config.title, "My Items");
        assert_eq!(config.current_page, 1);
        assert_eq!(config.total_pages, 1);
        assert!(!config.has_more_items);
    }
}
