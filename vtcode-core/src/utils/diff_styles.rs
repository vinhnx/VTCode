//! Unified styling for git diff and similar diff renderings.
//!
//! Provides consistent color palettes for additions, deletions, and headers
//! across diff rendering implementations.

use anstyle::{AnsiColor, Color, RgbColor, Style};

/// Color palette for git diff visualization
#[derive(Debug, Clone, Copy)]
pub struct DiffColorPalette {
    pub added_fg: RgbColor,
    pub added_bg: RgbColor,
    pub removed_fg: RgbColor,
    pub removed_bg: RgbColor,
    pub header_color: AnsiColor,
}

impl DiffColorPalette {
    /// Default palette: green on dark green for additions, red on dark red for deletions
    pub fn default() -> Self {
        Self {
            added_fg: RgbColor(200, 255, 200),
            added_bg: RgbColor(0, 64, 0),
            removed_fg: RgbColor(255, 200, 200),
            removed_bg: RgbColor(64, 0, 0),
            header_color: AnsiColor::Cyan,
        }
    }

    /// Style for added lines (green on dark green)
    pub fn added_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.added_fg)))
            .bg_color(Some(Color::Rgb(self.added_bg)))
    }

    /// Style for removed lines (red on dark red)
    pub fn removed_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.removed_fg)))
            .bg_color(Some(Color::Rgb(self.removed_bg)))
    }

    /// Style for diff headers (cyan)
    pub fn header_style(&self) -> Style {
        Style::new().fg_color(Some(Color::Ansi(self.header_color)))
    }

    /// Style for context lines (normal text)
    pub fn context_style(&self) -> Style {
        Style::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_palette_colors() {
        let palette = DiffColorPalette::default();

        assert_eq!(palette.added_fg, RgbColor(200, 255, 200));
        assert_eq!(palette.added_bg, RgbColor(0, 64, 0));
        assert_eq!(palette.removed_fg, RgbColor(255, 200, 200));
        assert_eq!(palette.removed_bg, RgbColor(64, 0, 0));
        assert_eq!(palette.header_color, AnsiColor::Cyan);
    }

    #[test]
    fn test_added_style() {
        let palette = DiffColorPalette::default();
        let style = palette.added_style();

        assert!(style.get_fg_color().is_some());
        assert!(style.get_bg_color().is_some());
    }

    #[test]
    fn test_removed_style() {
        let palette = DiffColorPalette::default();
        let style = palette.removed_style();

        assert!(style.get_fg_color().is_some());
        assert!(style.get_bg_color().is_some());
    }

    #[test]
    fn test_header_style() {
        let palette = DiffColorPalette::default();
        let style = palette.header_style();

        assert!(style.get_fg_color().is_some());
    }

    #[test]
    fn test_context_style() {
        let palette = DiffColorPalette::default();
        let style = palette.context_style();

        // Context should be minimal styling
        assert!(style.get_fg_color().is_none());
    }
}
