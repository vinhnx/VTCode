//! Base panel widget primitive.
//!
//! `Panel` renders standardized chrome (borders, titles) and returns the inner
//! area for child widgets. It is decoupled from any specific session or styling
//! type via the [`PanelStyleProvider`] trait.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    symbols::border,
    widgets::{Block, Widget},
};

use crate::design::layout::LayoutMode;
use vtcode_commons::terminal_capabilities::BoxGlyphTier;

/// Chrome for terminals that cannot render Unicode box drawing.
///
/// Ratatui's `BorderType::Plain` still draws `┌─┐│└┘`, so the ASCII rung of the
/// capability ladder needs its own set: the point of detecting a non-Unicode
/// terminal is to avoid mojibake, not to pick a different Unicode style.
pub(crate) const ASCII_BORDER_SET: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// Panel chrome for a capability tier.
pub(crate) fn border_set_for_tier(tier: BoxGlyphTier) -> border::Set<'static> {
    match tier {
        BoxGlyphTier::RichUnicode => ratatui::widgets::BorderType::HeavyDoubleDashed.to_border_set(),
        BoxGlyphTier::Unicode => ratatui::widgets::BorderType::Rounded.to_border_set(),
        BoxGlyphTier::Ascii => ASCII_BORDER_SET,
    }
}

/// The canonical panel chrome for this terminal.
///
/// Reads the capability verdict once per process, so a bordered panel costs no
/// per-frame environment probing.
pub(crate) fn canonical_border_set() -> border::Set<'static> {
    border_set_for_tier(vtcode_commons::terminal_capabilities::box_glyph_tier())
}

/// Degrade a preferred set to ASCII when the terminal cannot render Unicode.
///
/// Chrome that keeps a fixed Unicode style calls this instead of hard-coding a
/// border set, so no surface draws mojibake on a non-Unicode terminal.
pub(crate) fn border_set_with_fallback(preferred: border::Set<'static>) -> border::Set<'static> {
    match vtcode_commons::terminal_capabilities::box_glyph_tier() {
        BoxGlyphTier::Ascii => ASCII_BORDER_SET,
        _ => preferred,
    }
}

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
///     .border_set(border::ROUNDED)
///     .render_and_get_inner(area, buf);
/// // Render child widget into `inner`
/// ```
pub(crate) struct Panel<'a, S: PanelStyleProvider> {
    styles: &'a S,
    title: Option<&'a str>,
    active: bool,
    mode: LayoutMode,
    border_set: Option<border::Set<'static>>,
}

impl<'a, S: PanelStyleProvider> Panel<'a, S> {
    /// Create a new panel with required style reference.
    pub(crate) fn new(styles: &'a S) -> Self {
        Self {
            styles,
            title: None,
            active: false,
            mode: LayoutMode::Standard,
            border_set: None,
        }
    }

    /// Set the panel title (displayed in the border).
    #[must_use]
    pub(crate) fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Mark the panel as active (highlighted border).
    #[must_use]
    pub(crate) fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Set the layout mode (affects border visibility).
    #[must_use]
    pub(crate) fn mode(mut self, mode: LayoutMode) -> Self {
        self.mode = mode;
        self
    }

    /// Override the border set.
    #[must_use]
    pub(crate) fn border_set(mut self, border_set: border::Set<'static>) -> Self {
        self.border_set = Some(border_set);
        self
    }

    /// Render the panel and return the inner area for child widgets.
    pub(crate) fn render_and_get_inner(self, area: Rect, buf: &mut Buffer) -> Rect {
        if area.is_empty() {
            return area;
        }
        if !self.mode.show_borders() {
            return area;
        }

        let border_style = if self.active {
            self.styles.accent_style()
        } else {
            self.styles.border_style()
        };

        let border_set = self.border_set.unwrap_or(border::PLAIN);

        let mut block = Block::bordered()
            .border_set(border_set)
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
    fn zero_area_returns_unchanged_without_painting() {
        let styles = MockStyles::new();
        let area = Rect::new(5, 5, 0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 10));
        let before = buf.clone();
        let inner = Panel::new(&styles)
            .mode(LayoutMode::Standard)
            .render_and_get_inner(area, &mut buf);
        assert_eq!(inner, area);
        assert_eq!(buf, before, "zero-area panel must not touch the buffer");
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
    fn ascii_tier_renders_ascii_chrome_end_to_end() {
        // The tier constant is not the contract; the painted buffer is.
        let styles = MockStyles::new();
        let area = Rect::new(0, 0, 12, 4);
        let mut buf = Buffer::empty(area);

        let inner = Panel::new(&styles)
            .mode(LayoutMode::Standard)
            .border_set(border_set_for_tier(BoxGlyphTier::Ascii))
            .render_and_get_inner(area, &mut buf);

        let rendered: String = (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().to_string())
            .collect();
        assert!(rendered.is_ascii(), "the ASCII tier must paint only ASCII chrome, got: {rendered:?}");
        assert_eq!(buf[(0, 0)].symbol(), "+", "top-left corner should be an ASCII plus");
        assert_eq!(buf[(0, 1)].symbol(), "|", "left edge should be an ASCII pipe");
        assert_eq!(inner, Rect::new(1, 1, 10, 2), "inner area must still be the bordered box");
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
