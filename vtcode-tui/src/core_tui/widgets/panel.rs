use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, BorderType, Widget},
};

use super::layout_mode::LayoutMode;
use crate::ui::tui::session::styling::SessionStyles;
use crate::ui::tui::session::terminal_capabilities;

/// A consistent panel wrapper that applies standardized chrome (borders, titles)
///
/// This widget ensures visual consistency across all panels in the UI by
/// providing a unified border and title style based on the active theme.
///
/// # Example
/// ```ignore
/// let inner = Panel::new(&styles)
///     .title("Transcript")
///     .active(true)
///     .mode(layout_mode)
///     .render_and_get_inner(area, buf);
/// // Render child widget into `inner`
/// ```
pub struct Panel<'a> {
    styles: &'a SessionStyles,
    title: Option<&'a str>,
    active: bool,
    mode: LayoutMode,
    border_type: Option<BorderType>,
}

impl<'a> Panel<'a> {
    /// Create a new panel with required style reference
    pub fn new(styles: &'a SessionStyles) -> Self {
        Self {
            styles,
            title: None,
            active: false,
            mode: LayoutMode::Standard,
            border_type: None,
        }
    }

    /// Set the panel title (displayed in the border)
    #[must_use]
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Mark the panel as active (highlighted border)
    #[must_use]
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Set the layout mode (affects border visibility)
    #[must_use]
    pub fn mode(mut self, mode: LayoutMode) -> Self {
        self.mode = mode;
        self
    }

    /// Override the border type
    #[must_use]
    pub fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = Some(border_type);
        self
    }

    /// Render the panel and return the inner area for child widgets
    pub fn render_and_get_inner(self, area: Rect, buf: &mut Buffer) -> Rect {
        if !self.mode.show_borders() {
            return area;
        }

        let border_style = if self.active {
            self.styles.accent_style()
        } else {
            self.styles.border_style()
        };

        let border_type = self
            .border_type
            .unwrap_or_else(terminal_capabilities::get_border_type);

        let mut block = Block::bordered()
            .border_type(border_type)
            .style(self.styles.default_style())
            .border_style(border_style);

        if self.mode.show_titles()
            && let Some(title) = self.title
        {
            let title_style = if self.active {
                self.styles.accent_style().add_modifier(Modifier::BOLD)
            } else {
                self.styles.default_style().add_modifier(Modifier::BOLD)
            };
            block = block.title(title).title_style(title_style);
        }

        let inner = block.inner(area);
        block.render(area, buf);
        inner
    }
}

/// Extended style methods for panels and visual hierarchy
pub trait PanelStyles {
    /// Style for muted/secondary content
    fn muted_style(&self) -> Style;

    /// Style for panel titles
    fn title_style(&self) -> Style;

    /// Style for active/focused borders
    fn border_active_style(&self) -> Style;

    /// Style for dividers between sections
    fn divider_style(&self) -> Style;
}

impl PanelStyles for SessionStyles {
    fn muted_style(&self) -> Style {
        self.default_style().add_modifier(Modifier::DIM)
    }

    fn title_style(&self) -> Style {
        self.accent_style().add_modifier(Modifier::BOLD)
    }

    fn border_active_style(&self) -> Style {
        self.border_style()
            .remove_modifier(Modifier::DIM)
            .add_modifier(Modifier::BOLD)
    }

    fn divider_style(&self) -> Style {
        self.border_style().add_modifier(Modifier::DIM)
    }
}
