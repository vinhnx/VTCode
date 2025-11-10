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
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use super::modal::compute_modal_area;

/// Trait for items that can be displayed in a palette
#[allow(dead_code)]
pub trait PaletteItem {
    /// Human-readable name to display in the palette
    fn display_name(&self) -> String;

    /// Optional icon or prefix to display before the name
    fn display_icon(&self) -> Option<String> {
        None
    }

    /// Optional style to apply to this item
    fn style(&self) -> Option<Style> {
        None
    }

    /// Whether this is a directory/container (for visual distinction)
    fn is_directory(&self) -> bool {
        false
    }
}

/// Helper to render loading state for palettes
#[allow(dead_code)]
// Only used in potential future code paths or tests
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
