//! Test-only probes over the terminal grid.
//!
//! The live session is a real login shell: its prompt prints the
//! workspace path into the same grid a palette probe writes into, so a
//! probe must address the cell it wrote instead of scanning the grid
//! for a marker character.

use gpui_kit::Rgba;
use rio_vt::config::colors::term::List;
use rio_vt::crosswords::grid::Dimensions;

use super::TerminalScreen;

impl TerminalScreen {
    /// Visible cell of the cursor: the position a probe reads back after
    /// writing a marker.
    pub(in crate::desktop) fn visible_cursor_cell(&self) -> Option<(usize, usize)> {
        self.cursor_cell(&self.terminal.grid)
    }

    /// Resolved foreground of one visible grid cell, against the given
    /// palette.
    pub(in crate::desktop) fn visible_cell_fg(&self, row: usize, column: usize, palette: &List) -> Option<Rgba> {
        let grid = &self.terminal.grid;
        let columns = grid.columns();
        if columns == 0 || column >= columns || row >= grid.screen_lines() {
            return None;
        }
        let cell = grid.display_iter().nth(row * columns + column)?;
        let style = grid.style_of(cell.square);
        Some(self.resolve_color(style.fg, palette))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rio_vt::config::colors::{Colors, NamedColor};

    fn rgba_of(palette: &List, named: NamedColor) -> Rgba {
        let color = palette[named];
        Rgba { r: color[0], g: color[1], b: color[2], a: color[3] }
    }

    #[test]
    fn terminal_screen_visible_cell_fg_addresses_cells_by_position() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let palette = List::from(&Colors::default());
        // Two cells holding the same glyph: only the position separates
        // them. A live session prints its prompt into the same grid as the
        // probe cell, so the probe must address cells, never scan for a
        // marker character.
        let _events = screen.process(b"XX");
        let _events = screen.process(b"\x1b[1;1H\x1b[31mX");
        assert_eq!(screen.visible_cell_fg(0, 0, &palette), Some(rgba_of(&palette, NamedColor::Red)));
        assert_eq!(screen.visible_cell_fg(0, 1, &palette), Some(rgba_of(&palette, NamedColor::Foreground)));
        assert_eq!(screen.visible_cell_fg(1, 0, &palette), Some(rgba_of(&palette, NamedColor::Foreground)));
        assert_eq!(screen.visible_cell_fg(0, 8, &palette), None, "past the last column");
        assert_eq!(screen.visible_cell_fg(9, 0, &palette), None, "past the last row");
    }

    #[test]
    fn terminal_screen_visible_cursor_cell_reports_the_next_write_position() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        assert_eq!(screen.visible_cursor_cell(), Some((0, 0)));
        let _events = screen.process(b"ab");
        assert_eq!(screen.visible_cursor_cell(), Some((0, 2)));
        let _events = screen.process(b"\r\nc");
        assert_eq!(screen.visible_cursor_cell(), Some((1, 1)));
    }
}
