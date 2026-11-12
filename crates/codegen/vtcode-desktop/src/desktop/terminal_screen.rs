//! Terminal grid state for the desktop's user-controlled PTY.
//!
//! Rio owns the grid, scrollback, alternate screen, and parser. Keeping the
//! grid here avoids flattening styles or losing terminal replies on the way
//! to the desktop renderer.

use std::cell::RefCell;
use std::rc::Rc;

use rio_vt::ansi::CursorShape;
use rio_vt::config::colors::{ColorArray, term::TermColors};
use rio_vt::crosswords::grid::{Dimensions, Grid, Scroll};
use rio_vt::crosswords::pos::{Column, CursorState, Line, Pos, Side};
use rio_vt::crosswords::square::Square;
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

#[cfg(test)]
mod tests {
    use super::*;
    use rio_vt::config::colors::{AnsiColor, NamedColor};
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
}
