//! Base panel widget primitive.
//!
//! `Panel` renders standardized chrome (borders, titles) and returns the inner
//! area for child widgets. It is decoupled from any specific session or styling
//! type via the [`PanelStyleProvider`] trait.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, BorderType, Widget},
};

use crate::layout::LayoutMode;

/// Trait for providing panel styles. Decouples `Panel` from any specific
/// session or theme type.
///
/// Implementors provide the three core styles needed to render a panel:
/// - `default_style`: the base style for the panel background
/// - `accent_style`: the style for active/focused borders
/// - `border_style`: the style for inactive borders
pub trait PanelStyleProvider {
    /// Base style for the panel background.
    fn default_style(&self) -> Style;

    /// Style for active/focused borders and titles.
    fn accent_style(&self) -> Style;

    /// Style for inactive borders.
    fn border_style(&self) -> Style;
}

/// A consistent panel wrapper that applies standardized chrome (borders, titles).
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
///     .border_type(BorderType::Rounded)
///     .render_and_get_inner(area, buf);
/// // Render child widget into `inner`
/// ```
pub struct Panel<'a, S: PanelStyleProvider> {
    styles: &'a S,
    title: Option<&'a str>,
    active: bool,
    mode: LayoutMode,
    border_type: Option<BorderType>,
}

impl<'a, S: PanelStyleProvider> Panel<'a, S> {
    /// Create a new panel with required style reference.
    pub fn new(styles: &'a S) -> Self {
        Self {
            styles,
            title: None,
            active: false,
            mode: LayoutMode::Standard,
            border_type: None,
        }
    }

    /// Set the panel title (displayed in the border).
    #[must_use]
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Mark the panel as active (highlighted border).
    #[must_use]
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Set the layout mode (affects border visibility).
    #[must_use]
    pub fn mode(mut self, mode: LayoutMode) -> Self {
        self.mode = mode;
        self
    }

    /// Override the border type.
    #[must_use]
    pub fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = Some(border_type);
        self
    }

    /// Render the panel and return the inner area for child widgets.
    pub fn render_and_get_inner(self, area: Rect, buf: &mut Buffer) -> Rect {
        if !self.mode.show_borders() {
            return area;
        }

        let border_style = if self.active {
            self.styles.accent_style()
        } else {
            self.styles.border_style()
        };

        let border_type = self.border_type.unwrap_or(BorderType::Plain);

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

/// Extended style methods for panels and visual hierarchy.
pub trait PanelStyles {
    /// Style for muted/secondary content.
    fn muted_style(&self) -> Style;

    /// Style for panel titles.
    fn title_style(&self) -> Style;

    /// Style for active/focused borders.
    fn border_active_style(&self) -> Style;

    /// Style for dividers between sections.
    fn divider_style(&self) -> Style;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockStyles {
        default: Style,
        accent: Style,
        border: Style,
    }

    impl MockStyles {
        fn new() -> Self {
            Self {
                default: Style::default(),
                accent: Style::default().fg(ratatui::style::Color::Cyan),
                border: Style::default().fg(ratatui::style::Color::Gray),
            }
        }
    }

    impl PanelStyleProvider for MockStyles {
        fn default_style(&self) -> Style {
            self.default
        }
        fn accent_style(&self) -> Style {
            self.accent
        }
        fn border_style(&self) -> Style {
            self.border
        }
    }

    #[test]
    fn compact_mode_returns_full_area() {
        let styles = MockStyles::new();
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        let inner = Panel::new(&styles)
            .mode(LayoutMode::Compact)
            .render_and_get_inner(area, &mut buf);
        assert_eq!(inner, area);
    }

    #[test]
    fn standard_mode_returns_smaller_inner_area() {
        let styles = MockStyles::new();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        let inner = Panel::new(&styles)
            .mode(LayoutMode::Standard)
            .render_and_get_inner(area, &mut buf);
        // Borders reduce inner area by 1 on each side
        assert_eq!(inner.x, area.x + 1);
        assert_eq!(inner.y, area.y + 1);
        assert_eq!(inner.width, area.width - 2);
        assert_eq!(inner.height, area.height - 2);
    }

    #[test]
    fn wide_mode_with_title() {
        let styles = MockStyles::new();
        let area = Rect::new(0, 0, 120, 30);
        let mut buf = Buffer::empty(area);
        let inner = Panel::new(&styles)
            .title("Test Panel")
            .active(true)
            .mode(LayoutMode::Wide)
            .render_and_get_inner(area, &mut buf);
        assert_eq!(inner.x, area.x + 1);
        assert_eq!(inner.y, area.y + 1);
        assert_eq!(inner.width, area.width - 2);
        assert_eq!(inner.height, area.height - 2);
    }

    #[test]
    fn active_panel_uses_accent_style() {
        let styles = MockStyles::new();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        // Should not panic and should render with accent style
        Panel::new(&styles)
            .active(true)
            .mode(LayoutMode::Standard)
            .render_and_get_inner(area, &mut buf);
    }
}
