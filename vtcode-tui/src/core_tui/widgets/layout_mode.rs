use ratatui::layout::Rect;

/// Responsive layout mode based on terminal dimensions
///
/// This enum provides a single source of truth for layout decisions
/// across the UI, enabling consistent responsive behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutMode {
    /// Minimal chrome for tiny terminals (< 80 cols or < 20 rows)
    Compact,
    /// Default layout for standard terminals
    Standard,
    /// Enhanced layout with sidebar for wide terminals (>= 120 cols, >= 24 rows)
    Wide,
}

impl LayoutMode {
    /// Determine layout mode from viewport dimensions
    pub fn from_area(area: Rect) -> Self {
        if area.width < 80 || area.height < 20 {
            LayoutMode::Compact
        } else if area.width >= 120 && area.height >= 24 {
            LayoutMode::Wide
        } else {
            LayoutMode::Standard
        }
    }

    /// Check if borders should be shown
    pub fn show_borders(self) -> bool {
        !matches!(self, LayoutMode::Compact)
    }

    /// Check if panel titles should be shown
    pub fn show_titles(self) -> bool {
        !matches!(self, LayoutMode::Compact)
    }

    /// Check if sidebar can be shown
    pub fn allow_sidebar(self) -> bool {
        matches!(self, LayoutMode::Wide)
    }

    /// Check if logs panel should be visible
    pub fn show_logs_panel(self) -> bool {
        !matches!(self, LayoutMode::Compact)
    }

    /// Get the footer height for this mode
    /// Footer is disabled in all modes to avoid duplicating header info
    pub fn footer_height(self) -> u16 {
        0
    }

    /// Check if footer should be shown
    /// Footer is disabled to avoid duplicating the header status bar
    pub fn show_footer(self) -> bool {
        false
    }

    /// Get the maximum header height as percentage of viewport
    pub fn max_header_percent(self) -> f32 {
        match self {
            LayoutMode::Compact => 0.15,
            LayoutMode::Standard => 0.25,
            LayoutMode::Wide => 0.30,
        }
    }

    /// Get the sidebar width percentage (only meaningful in Wide mode)
    pub fn sidebar_width_percent(self) -> u16 {
        match self {
            LayoutMode::Wide => 28,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_mode() {
        let area = Rect::new(0, 0, 60, 15);
        assert_eq!(LayoutMode::from_area(area), LayoutMode::Compact);
    }

    #[test]
    fn test_standard_mode() {
        let area = Rect::new(0, 0, 100, 30);
        assert_eq!(LayoutMode::from_area(area), LayoutMode::Standard);
    }

    #[test]
    fn test_wide_mode() {
        let area = Rect::new(0, 0, 150, 40);
        assert_eq!(LayoutMode::from_area(area), LayoutMode::Wide);
    }

    #[test]
    fn test_mode_properties() {
        // Compact: no borders, no footer, no sidebar
        assert!(!LayoutMode::Compact.show_borders());
        assert!(!LayoutMode::Compact.show_footer());
        assert!(!LayoutMode::Compact.allow_sidebar());
        assert_eq!(LayoutMode::Compact.footer_height(), 0);

        // Standard: borders but no footer
        assert!(LayoutMode::Standard.show_borders());
        assert!(!LayoutMode::Standard.show_footer());
        assert!(!LayoutMode::Standard.allow_sidebar());
        assert_eq!(LayoutMode::Standard.footer_height(), 0);

        // Wide: borders + sidebar, but no footer (header already shows status)
        assert!(LayoutMode::Wide.show_borders());
        assert!(!LayoutMode::Wide.show_footer());
        assert!(LayoutMode::Wide.allow_sidebar());
        assert_eq!(LayoutMode::Wide.footer_height(), 0);
    }

    #[test]
    fn test_boundary_conditions() {
        // Exactly at 80 cols should be Standard
        let area_80 = Rect::new(0, 0, 80, 24);
        assert_eq!(LayoutMode::from_area(area_80), LayoutMode::Standard);

        // Exactly at 120 cols should be Wide
        let area_120 = Rect::new(0, 0, 120, 24);
        assert_eq!(LayoutMode::from_area(area_120), LayoutMode::Wide);

        // 79 cols should be Compact
        let area_79 = Rect::new(0, 0, 79, 24);
        assert_eq!(LayoutMode::from_area(area_79), LayoutMode::Compact);

        // Wide width but short height should be Standard (not Wide)
        let area_wide_short = Rect::new(0, 0, 150, 20);
        assert_eq!(LayoutMode::from_area(area_wide_short), LayoutMode::Standard);
    }
}
