//! Terminal grid state for the desktop's user-controlled PTY.
//!
//! Rio owns the grid, scrollback, alternate screen, and parser. Keeping the
//! grid here avoids flattening styles or losing terminal replies on the way
//! to the desktop renderer.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::Rgba;
use rio_vt::ansi::CursorShape;
use rio_vt::config::colors::{AnsiColor, ColorArray, NamedColor, term::List, term::TermColors};
use rio_vt::crosswords::grid::{Dimensions, Grid, Scroll};
use rio_vt::crosswords::pos::{Column, CursorState, Line, Pos, Side};
use rio_vt::crosswords::square::{Square, Wide};
use rio_vt::crosswords::style::StyleFlags;
use rio_vt::crosswords::{Crosswords, CrosswordsSize, Mode};
use rio_vt::event::{EventListener, RioEvent, WindowId};
use rio_vt::performer::handler::Processor;
use rio_vt::selection::{Selection, SelectionRange, SelectionType};

#[derive(Clone, Default)]
struct TerminalEvents(Rc<RefCell<Vec<RioEvent>>>);

impl EventListener for TerminalEvents {
    fn send_event(&self, event: RioEvent, _: WindowId) {
        self.0.borrow_mut().push(event);
    }

    fn send_event_with_high_priority(&self, event: RioEvent, _: WindowId) {
        self.0.borrow_mut().push(event);
    }
}

pub(super) struct TerminalScreen {
    terminal: Crosswords<TerminalEvents>,
    parser: Processor,
    events: TerminalEvents,
}

impl TerminalScreen {
    pub(super) fn new(columns: u16, rows: u16, scrollback: usize) -> Self {
        let events = TerminalEvents::default();
        let terminal =
            Crosswords::new(size(columns, rows), CursorShape::Block, events.clone(), WindowId::from(0), 0, scrollback);
        Self { terminal, parser: Processor::default(), events }
    }

    pub(super) fn process(&mut self, bytes: &[u8]) -> Vec<RioEvent> {
        self.parser.advance(&mut self.terminal, bytes);
        std::mem::take(&mut *self.events.0.borrow_mut())
    }

    pub(super) fn resize(&mut self, columns: u16, rows: u16) -> Vec<RioEvent> {
        self.terminal.resize(size(columns, rows));
        std::mem::take(&mut *self.events.0.borrow_mut())
    }

    pub(super) fn scroll(&mut self, lines: i32) -> Vec<RioEvent> {
        self.terminal.scroll_display(Scroll::Delta(lines));
        std::mem::take(&mut *self.events.0.borrow_mut())
    }

    pub(super) fn grid(&self) -> &Grid<Square> {
        &self.terminal.grid
    }

    /// Per-session palette overrides set by terminal applications.
    pub(super) fn color_overrides(&self) -> &TermColors {
        &self.terminal.colors
    }

    /// Resolve an ANSI palette entry against the current desktop palette.
    pub(super) fn palette_color(&self, index: u8, fallback: ColorArray) -> ColorArray {
        self.terminal.colors[usize::from(index)].unwrap_or(fallback)
    }

    pub(super) fn cursor(&self) -> CursorState {
        self.terminal.cursor()
    }

    pub(super) fn mode(&self) -> Mode {
        self.terminal.mode()
    }

    pub(super) fn paste_bytes(&self, text: &str) -> Vec<u8> {
        if self.mode().contains(Mode::BRACKETED_PASTE) {
            // Escape characters in clipboard text could terminate the paste
            // early and turn the remainder into terminal input.
            let filtered = text.replace(['\x1b', '\u{009b}'], "");
            [b"\x1b[200~".as_slice(), filtered.as_bytes(), b"\x1b[201~"].concat()
        } else {
            text.as_bytes().to_vec()
        }
    }

    pub(super) fn history_size(&self) -> usize {
        self.terminal.history_size()
    }

    /// Start a selection at a visible grid cell. Invalid coordinates clear it.
    pub(super) fn start_selection(&mut self, row: usize, column: usize, side: Side) {
        self.terminal.selection = self
            .viewport_position(row, column)
            .map(|position| Selection::new(SelectionType::Simple, position, side));
    }

    /// Extend the current selection to a visible grid cell.
    pub(super) fn update_selection(&mut self, row: usize, column: usize, side: Side) {
        if let Some(position) = self.viewport_position(row, column)
            && let Some(selection) = self.terminal.selection.as_mut()
        {
            selection.update(position, side);
        }
    }

    pub(super) fn clear_selection(&mut self) {
        self.terminal.selection = None;
    }

    /// Select the whole visible viewport (not older scrollback).
    pub(super) fn select_all(&mut self) {
        let (rows, columns) = (self.terminal.grid.screen_lines(), self.terminal.grid.columns());
        if rows == 0 || columns == 0 {
            return;
        }
        self.start_selection(0, 0, Side::Left);
        self.update_selection(rows - 1, columns - 1, Side::Right);
    }

    pub(super) fn selection_range(&self) -> Option<SelectionRange> {
        self.terminal.selection.as_ref()?.to_range(&self.terminal)
    }

    pub(super) fn selected_text(&self) -> Option<String> {
        self.terminal.selection_to_string()
    }

    fn viewport_position(&self, row: usize, column: usize) -> Option<Pos> {
        let grid = &self.terminal.grid;
        if row >= grid.screen_lines() || column >= grid.columns() {
            return None;
        }
        let row = i32::try_from(row).ok()?;
        let offset = i32::try_from(grid.display_offset()).ok()?;
        Some(Pos::new(Line(row.checked_sub(offset)?), Column(column)))
    }
}

fn size(columns: u16, rows: u16) -> CrosswordsSize {
    CrosswordsSize::new(usize::from(columns.max(1)), usize::from(rows.max(1)))
}

/// One styled text run for the panel renderer: the logical row model shared
/// by the grid projection and its tests. v1a honors bold and inverse; dim,
/// italic, and underline render plain.
#[derive(Debug, PartialEq)]
pub(super) struct TerminalRowRun {
    pub(super) text: String,
    pub(super) fg: Rgba,
    pub(super) bg: Rgba,
    pub(super) bold: bool,
}

impl TerminalScreen {
    /// Visible rows as coalesced style runs with the cursor cell inverted
    /// unless the cursor is hidden (e.g. scrolled into history).
    pub(super) fn display_rows(&self, palette: &List) -> Vec<Vec<TerminalRowRun>> {
        let grid = &self.terminal.grid;
        let columns = grid.columns();
        if columns == 0 {
            return Vec::new();
        }
        let cursor = self.cursor_cell(grid);
        let selected = self.selection_range();
        let offset = i32::try_from(grid.display_offset()).ok();
        let cells: Vec<_> = grid.display_iter().collect();
        cells
            .chunks(columns)
            .enumerate()
            .map(|(row, chunk)| {
                let line = offset.and_then(|offset| i32::try_from(row).ok().and_then(|row| row.checked_sub(offset)));
                let mut runs = Vec::new();
                for (column, cell) in chunk.iter().enumerate() {
                    let highlighted = line.is_some_and(|grid_line| {
                        selected
                            .as_ref()
                            .is_some_and(|range| selection_contains(range, grid_line, column))
                    });
                    self.push_cell(&mut runs, cell.square, (row, column), cursor, highlighted, palette);
                }
                if runs.is_empty() {
                    let fg = self.resolve_color(AnsiColor::Named(NamedColor::Foreground), palette);
                    let bg = self.resolve_color(AnsiColor::Named(NamedColor::Background), palette);
                    runs.push(TerminalRowRun { text: " ".into(), fg, bg, bold: false });
                }
                runs
            })
            .collect()
    }

    /// Cursor position as a visible `(row, column)` cell, if shown.
    fn cursor_cell(&self, grid: &Grid<Square>) -> Option<(usize, usize)> {
        let cursor = self.cursor();
        if cursor.content == CursorShape::Hidden {
            return None;
        }
        let offset = i32::try_from(grid.display_offset()).ok()?;
        let row = usize::try_from(cursor.pos.row.0.checked_add(offset)?).ok()?;
        let column = cursor.pos.col.0;
        (row < grid.screen_lines() && column < grid.columns()).then_some((row, column))
    }

    fn push_cell(
        &self,
        runs: &mut Vec<TerminalRowRun>,
        square: &Square,
        cell: (usize, usize),
        cursor: Option<(usize, usize)>,
        selected: bool,
        palette: &List,
    ) {
        if matches!(square.wide(), Wide::Spacer) {
            return;
        }
        let style = self.terminal.grid.style_of(square);
        let (mut fg, mut bg) = (self.resolve_color(style.fg, palette), self.resolve_color(style.bg, palette));
        if style.flags.contains(StyleFlags::INVERSE) || Some(cell) == cursor || selected {
            std::mem::swap(&mut fg, &mut bg);
        }
        let bold = style.flags.contains(StyleFlags::BOLD);
        // Rio blanks cells with NUL; the renderer wants spaces.
        let glyph = match square.c() {
            '\0' => ' ',
            glyph => glyph,
        };
        if let Some(run) = runs.last_mut()
            && run.fg == fg
            && run.bg == bg
            && run.bold == bold
        {
            run.text.push(glyph);
        } else {
            runs.push(TerminalRowRun { text: glyph.to_string(), fg, bg, bold });
        }
    }

    fn resolve_color(&self, color: AnsiColor, palette: &List) -> Rgba {
        let [red, green, blue, alpha] = match color {
            AnsiColor::Named(named) => palette[named],
            AnsiColor::Indexed(index) => self.palette_color(index, palette[usize::from(index)]),
            AnsiColor::Spec(rgb) => [
                f32::from(rgb.r) / 255.0,
                f32::from(rgb.g) / 255.0,
                f32::from(rgb.b) / 255.0,
                1.0,
            ],
        };
        Rgba { r: red, g: green, b: blue, a: alpha }
    }
}

/// Row-major membership for a normalized [`SelectionRange`]: the first line
/// runs from the start column, middle lines span fully, the last runs to the
/// end column (inclusive, matching [`TerminalScreen::selected_text`]).
fn selection_contains(range: &SelectionRange, line: i32, column: usize) -> bool {
    let (start_line, start_column) = (range.start.row.0, range.start.col.0);
    let (end_line, end_column) = (range.end.row.0, range.end.col.0);
    if line < start_line || line > end_line {
        return false;
    }
    if line > start_line && line < end_line {
        return true;
    }
    if start_line == end_line {
        return column >= start_column && column <= end_column;
    }
    if line == start_line {
        column >= start_column
    } else {
        column <= end_column
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rio_vt::config::colors::{AnsiColor, Colors, NamedColor};
    use rio_vt::crosswords::style::StyleFlags;

    fn text(screen: &TerminalScreen) -> String {
        screen.grid().display_iter().map(|cell| cell.square.c()).collect()
    }

    #[test]
    fn terminal_screen_preserves_style_and_cursor_motion() {
        let mut screen = TerminalScreen::new(8, 3, 10);
        let _events = screen.process(b"ab\x1b[1;31mC\x1b[0m\x1b[2;4HZ");
        let cells: Vec<_> = screen.grid().display_iter().collect();
        assert_eq!(cells[2].square.c(), 'C');
        let style = screen.grid().style_of(cells[2].square);
        assert_eq!(style.fg, AnsiColor::Named(NamedColor::Red));
        assert!(style.flags.contains(StyleFlags::BOLD));
        assert_eq!(cells[11].square.c(), 'Z');
        assert_eq!(screen.cursor().pos.row.0, 1);
        assert_eq!(screen.cursor().pos.col.0, 4);
        let events = screen.process(b"\x1b[6n");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RioEvent::PtyWrite(_, reply) if reply == "\x1b[2;5R"))
        );
    }

    #[test]
    fn terminal_screen_keeps_alternate_screen_and_sessions_independent() {
        let mut first = TerminalScreen::new(12, 3, 10);
        let mut second = TerminalScreen::new(12, 3, 10);
        let _events = first.process(b"first\x1b[?1049halt");
        let _events = second.process(b"second");
        assert!(text(&first).contains("alt"));
        assert!(!text(&first).contains("first"));
        assert!(text(&second).contains("second"));
        let _events = first.process(b"\x1b[?1049l");
        assert!(text(&first).contains("first"));
        assert!(!text(&first).contains("alt"));
        assert!(text(&second).contains("second"));
    }

    #[test]
    fn terminal_screen_scrollback_resize_and_device_reply() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let _events = screen.process(b"one\r\ntwo\r\nthree");
        assert!(screen.history_size() > 0);
        assert!(!text(&screen).contains("one"));
        let _events = screen.scroll(1);
        assert!(text(&screen).contains("one"));
        assert_eq!(screen.cursor().content, CursorShape::Hidden);
        let _events = screen.scroll(-1);
        let _events = screen.resize(10, 3);
        assert_eq!(screen.grid().display_iter().count(), 30);
        let events = screen.process(b"\x1b[6n");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RioEvent::PtyWrite(_, reply) if reply.ends_with('R')))
        );
    }

    #[test]
    fn terminal_screen_handles_split_utf8_and_zero_dimensions() {
        let mut screen = TerminalScreen::new(0, 0, 10);
        assert_eq!(screen.grid().display_iter().count(), 1);
        let _events = screen.resize(4, 2);
        let _events = screen.process(&[0xe7, 0x95]);
        let _events = screen.process(&[0x8c]);
        assert_eq!(screen.grid().display_iter().next().unwrap().square.c(), '界');
        assert_eq!(screen.grid().display_iter().count(), 8);
    }

    #[test]
    fn terminal_screen_scroll_events_do_not_accumulate() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        for _ in 0..100 {
            let _events = screen.scroll(1);
        }
        assert!(screen.events.0.borrow().is_empty());
        assert!(screen.process(b"x").len() < 10);
    }

    #[test]
    fn terminal_screen_selects_scrollback_and_rejects_invalid_cells() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let _events = screen.process(b"alpha\r\nbeta\r\ngamma");
        let _events = screen.scroll(1);
        screen.start_selection(0, 1, Side::Left);
        screen.update_selection(0, 3, Side::Right);
        assert_eq!(screen.selected_text().as_deref(), Some("lph"));
        let range = screen.selection_range().unwrap();
        assert_eq!(range.start.row.0, -1);
        screen.update_selection(99, 99, Side::Right);
        assert_eq!(screen.selected_text().as_deref(), Some("lph"));
        screen.start_selection(2, 0, Side::Left);
        assert!(screen.selected_text().is_none());
        screen.start_selection(1, 0, Side::Left);
        screen.update_selection(1, 3, Side::Right);
        assert_eq!(screen.selected_text().as_deref(), Some("beta"));
        screen.start_selection(1, 3, Side::Right);
        screen.update_selection(1, 0, Side::Left);
        assert_eq!(screen.selected_text().as_deref(), Some("beta"));
        screen.clear_selection();
        assert!(screen.selection_range().is_none());
    }

    #[test]
    fn terminal_screen_uses_each_sessions_input_modes_for_paste() {
        let mut first = TerminalScreen::new(8, 2, 10);
        let second = TerminalScreen::new(8, 2, 10);
        let _events = first.process(b"\x1b[?2004h\x1b[?1h");
        assert_eq!(first.paste_bytes("a\nb"), b"\x1b[200~a\nb\x1b[201~");
        assert_eq!(first.paste_bytes("a\x1b[201~b"), b"\x1b[200~a[201~b\x1b[201~");
        assert!(first.mode().contains(Mode::APP_CURSOR));
        assert_eq!(second.paste_bytes("a\nb"), b"a\nb");
        assert!(!second.mode().contains(Mode::APP_CURSOR));
        let _events = first.process(b"\x1b[?2004l\x1b[?1l");
        assert_eq!(first.paste_bytes("a\nb"), b"a\nb");
        assert!(!first.mode().contains(Mode::APP_CURSOR));
    }

    #[test]
    fn terminal_screen_palette_overrides_are_isolated_and_resettable() {
        let mut first = TerminalScreen::new(8, 2, 10);
        let second = TerminalScreen::new(8, 2, 10);
        let _events = first.process(b"\x1b]4;1;rgb:ff/00/00\x07");
        assert!(first.color_overrides()[1].is_some());
        assert!(second.color_overrides()[1].is_none());
        let fallback = [0.1, 0.2, 0.3, 1.0];
        assert_eq!(first.palette_color(1, fallback), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(second.palette_color(1, fallback), fallback);
        let _events = first.process(b"\x1b]104;1\x07");
        assert!(first.color_overrides()[1].is_none());
        assert_eq!(first.palette_color(1, fallback), fallback);
    }

    fn rgba_of(palette: &List, named: NamedColor) -> Rgba {
        let color = palette[named];
        Rgba { r: color[0], g: color[1], b: color[2], a: color[3] }
    }

    fn rgba_slot(palette: &List, index: usize) -> Rgba {
        let color = palette[index];
        Rgba { r: color[0], g: color[1], b: color[2], a: color[3] }
    }

    #[test]
    fn terminal_screen_display_rows_coalesce_styles_and_invert_cursor() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let palette = List::from(&Colors::default());
        let _events = screen.process(b"ab\x1b[1;31mC");
        let rows = screen.display_rows(&palette);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 4);
        assert_eq!(rows[0][0].text, "ab");
        assert!(!rows[0][0].bold);
        assert_eq!(rows[0][1].text, "C");
        assert!(rows[0][1].bold);
        assert_eq!(rows[0][1].fg, rgba_of(&palette, NamedColor::Red));
        assert_eq!(rows[0][2].text, " ");
        assert_eq!(rows[0][2].fg, rows[0][0].bg);
        assert_eq!(rows[0][2].bg, rows[0][0].fg);
        assert_eq!(rows[0][3].text, "    ");
        assert_eq!(rows[1].len(), 1);
        assert_eq!(rows[1][0].text, "        ");
    }

    #[test]
    fn terminal_screen_display_rows_swap_inverse_cells() {
        let mut screen = TerminalScreen::new(8, 1, 10);
        let palette = List::from(&Colors::default());
        let _events = screen.process(b"\x1b[7mZ\x1b[0mXY");
        let rows = screen.display_rows(&palette);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 4);
        assert_eq!(rows[0][0].text, "Z");
        assert_eq!(rows[0][0].fg, rows[0][1].bg);
        assert_eq!(rows[0][0].bg, rows[0][1].fg);
        assert_eq!(rows[0][1].text, "XY");
        assert_eq!(rows[0][2].text, " ");
        assert_eq!(rows[0][2].fg, rows[0][1].bg);
        assert_eq!(rows[0][3].text, "    ");
    }

    #[test]
    fn terminal_screen_display_rows_resolve_indexed_override_and_rgb() {
        let mut screen = TerminalScreen::new(8, 1, 10);
        let palette = List::from(&Colors::default());
        let _events = screen.process(b"\x1b[38;5;9mX");
        let rows = screen.display_rows(&palette);
        assert_eq!(rows[0][0].text, "X");
        assert_eq!(rows[0][0].fg, rgba_slot(&palette, 9));
        assert_ne!(rows[0][0].fg, rgba_slot(&palette, 1));
        let _events = screen.process(b"\x1b]4;9;rgb:00/ff/00\x07\x1b[38;5;9mY\x1b[38;2;10;20;30mZ");
        let rows = screen.display_rows(&palette);
        assert_eq!(rows[0][0].text, "XY");
        assert_eq!(
            rows[0][0].fg,
            Rgba {
                r: 0.0 / 255.0,
                g: 255.0 / 255.0,
                b: 0.0 / 255.0,
                a: 1.0
            }
        );
        assert_eq!(rows[0][1].text, "Z");
        assert_eq!(
            rows[0][1].fg,
            Rgba {
                r: 10.0 / 255.0,
                g: 20.0 / 255.0,
                b: 30.0 / 255.0,
                a: 1.0
            }
        );
    }

    #[test]
    fn terminal_screen_display_rows_skip_inversion_when_cursor_hidden() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let palette = List::from(&Colors::default());
        let _events = screen.process(b"one\r\ntwo\r\nthree");
        let _events = screen.scroll(1);
        let rows = screen.display_rows(&palette);
        let background = rgba_of(&palette, NamedColor::Background);
        assert!(rows.iter().flatten().all(|run| run.bg == background));
    }

    #[test]
    fn terminal_screen_display_rows_keep_wide_glyphs_without_spacer_runs() {
        let mut screen = TerminalScreen::new(8, 1, 10);
        let palette = List::from(&Colors::default());
        let _events = screen.process("界x".as_bytes());
        let rows = screen.display_rows(&palette);
        assert_eq!(rows.len(), 1);
        let text: String = rows[0].iter().map(|run| run.text.as_str()).collect();
        assert!(text.starts_with("界x"), "{text:?}");
    }

    #[test]
    fn terminal_screen_display_rows_highlight_selection_both_directions() {
        let palette = List::from(&Colors::default());
        let project = |reverse: bool| {
            let mut screen = TerminalScreen::new(8, 3, 10);
            let _events = screen.process(b"aaa\r\nbbb\r\nccc");
            if reverse {
                screen.start_selection(1, 2, Side::Right);
                screen.update_selection(0, 1, Side::Left);
            } else {
                screen.start_selection(0, 1, Side::Left);
                screen.update_selection(1, 2, Side::Right);
            }
            screen.display_rows(&palette)
        };
        let (forward, reversed) = (project(false), project(true));
        assert_eq!(forward, reversed);
        let plain = rgba_of(&palette, NamedColor::Foreground);
        let ground = rgba_of(&palette, NamedColor::Background);
        assert_eq!(forward[0].len(), 2);
        assert_eq!(forward[0][0].text, "a");
        assert_eq!(forward[0][0].fg, plain);
        assert_eq!(forward[0][1].text, "aa     ");
        assert_eq!(forward[0][1].fg, ground);
        assert_eq!(forward[0][1].bg, plain);
        assert_eq!(forward[1][0].text, "bbb");
        assert_eq!(forward[1][0].fg, ground);
    }

    #[test]
    fn terminal_screen_select_all_covers_viewport_text() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let _events = screen.process(b"ab\r\ncd");
        screen.select_all();
        let text = screen.selected_text().unwrap();
        assert!(text.contains("ab") && text.contains("cd"), "{text:?}");
    }

    #[test]
    fn terminal_screen_display_rows_highlight_scrolled_selection() {
        let mut screen = TerminalScreen::new(8, 3, 10);
        let palette = List::from(&Colors::default());
        let _events = screen.process(b"aaa\r\nbbb\r\nccc\r\nddd");
        let _events = screen.scroll(1);
        screen.start_selection(0, 0, Side::Left);
        screen.update_selection(0, 2, Side::Right);
        let rows = screen.display_rows(&palette);
        let plain = rgba_of(&palette, NamedColor::Foreground);
        let ground = rgba_of(&palette, NamedColor::Background);
        assert_eq!(rows[0][0].text, "aaa");
        assert_eq!(rows[0][0].fg, ground);
        assert_eq!(rows[0][0].bg, plain);
        assert_eq!(rows[0][1].fg, plain);
        assert_eq!(rows[1][0].fg, plain);
    }
}
