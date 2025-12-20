// Diff color palette for consistent git diff styling
// Centralizes RGB color values for additions/deletions

use anstyle::{Color, RgbColor, Style};

#[derive(Debug, Clone, Copy)]
pub struct DiffColorPalette {
    pub added_fg: RgbColor,
    pub added_bg: RgbColor,
    pub removed_fg: RgbColor,
    pub removed_bg: RgbColor,
    pub header_fg: RgbColor,
    pub header_bg: RgbColor,
}

impl Default for DiffColorPalette {
    /// Green on soft green for additions, red on soft red for deletions
    /// Background colors use very low saturation (~20% brightness) with slight desaturation
    /// for a subtle, non-intrusive appearance that doesn't strain the eyes
    fn default() -> Self {
        Self {
            added_fg: RgbColor(180, 240, 180),
            added_bg: RgbColor(10, 24, 10), // Very soft green (~20% brightness, desaturated)
            removed_fg: RgbColor(240, 180, 180),
            removed_bg: RgbColor(24, 10, 10), // Very soft red (~20% brightness, desaturated)
            header_fg: RgbColor(150, 200, 220), // Soft cyan foreground
            header_bg: RgbColor(10, 16, 20),  // Very soft cyan background (~15% brightness)
        }
    }
}

impl DiffColorPalette {
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
        Style::new()
            .fg_color(Some(Color::Rgb(self.header_fg)))
            .bg_color(Some(Color::Rgb(self.header_bg)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_palette_defaults() {
        let palette = DiffColorPalette::default();
        assert_eq!(palette.added_fg, RgbColor(180, 240, 180));
        assert_eq!(palette.added_bg, RgbColor(10, 24, 10));
        assert_eq!(palette.removed_fg, RgbColor(240, 180, 180));
        assert_eq!(palette.removed_bg, RgbColor(24, 10, 10));
    }

    #[test]
    fn test_added_style() {
        let palette = DiffColorPalette::default();
        let style = palette.added_style();
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_removed_style() {
        let palette = DiffColorPalette::default();
        let style = palette.removed_style();
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_header_style() {
        let palette = DiffColorPalette::default();
        let style = palette.header_style();
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_header_colors() {
        let palette = DiffColorPalette::default();
        assert_eq!(palette.header_fg, RgbColor(150, 200, 220));
        assert_eq!(palette.header_bg, RgbColor(10, 16, 20));
    }
}
