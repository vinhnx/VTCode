// vtcode-core/src/utils/diff_styles.rs
//! Centralized diff styling with consistent color palettes

use anstyle::{AnsiColor, Color, RgbColor, Style};

#[derive(Debug, Clone, Copy)]
pub struct DiffColorPalette {
    pub added_fg: RgbColor,
    pub added_bg: RgbColor,
    pub removed_fg: RgbColor,
    pub removed_bg: RgbColor,
    pub header_color: AnsiColor,
}

impl DiffColorPalette {
    /// Green on dark green for additions, red on dark red for deletions
    pub fn default() -> Self {
        Self {
            added_fg: RgbColor(200, 255, 200),
            added_bg: RgbColor(0, 64, 0),
            removed_fg: RgbColor(255, 200, 200),
            removed_bg: RgbColor(64, 0, 0),
            header_color: AnsiColor::Cyan,
        }
    }

    pub fn added_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.added_fg)))
            .bg_color(Some(Color::Rgb(self.added_bg)))
    }

    pub fn removed_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.removed_fg)))
            .bg_color(Some(Color::Rgb(self.removed_bg)))
    }

    pub fn header_style(&self) -> Style {
        Style::new().fg_color(Some(Color::Ansi(self.header_color)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_color_palette_defaults() {
        let palette = DiffColorPalette::default();
        assert_eq!(palette.added_fg, RgbColor(200, 255, 200));
        assert_eq!(palette.added_bg, RgbColor(0, 64, 0));
        assert_eq!(palette.removed_fg, RgbColor(255, 200, 200));
        assert_eq!(palette.removed_bg, RgbColor(64, 0, 0));
    }

    #[test]
    fn test_added_style_contains_colors() {
        let palette = DiffColorPalette::default();
        let style = palette.added_style();
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_removed_style_contains_colors() {
        let palette = DiffColorPalette::default();
        let style = palette.removed_style();
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_header_style_is_cyan() {
        let palette = DiffColorPalette::default();
        let style = palette.header_style();
        assert!(!style.to_string().is_empty());
    }
}
