mod edit;
mod modes;
mod report;

use crate::cell::Cell;
use crate::cursor::Cursor;
use crate::mode::TerminalModes;
use crate::parser::{CsiSequence, ParserState};
use crate::region::Region;
use crate::screen::{Screen, ScreenKind, default_tab_stops, plain_text_for_screen};
use crate::style::Style;

/// A pure-Rust VT terminal emulator with incremental byte processing.
///
/// Feed bytes via [`write`](Terminal::write), then query the screen state via
/// accessors like [`plain_text`](Terminal::plain_text) or [`cell`](Terminal::cell).
pub struct Terminal {
    cols: usize,
    rows: usize,
    primary: Screen,
    alternate: Screen,
    active: ScreenKind,
    current_style: Style,
    state: ParserState,
    csi_buffer: String,
    csi_intermediate: u8,
    osc_buffer: Vec<u8>,
    utf8_buffer: Vec<u8>,
    utf8_remaining: usize,
    output: Vec<u8>,
    clipboard: Vec<String>,
    bell_count: usize,
    title: Option<String>,
    modes: TerminalModes,
    tab_stops: Vec<bool>,
    last_printed: Option<char>,
    scroll_region: Region,
    max_scrollback: usize,
}

impl Terminal {
    /// Create a new terminal with the given dimensions.
    pub fn new(cols: usize, rows: usize) -> Self {
        let style = Style::default();
        let scroll_region = Region {
            top: 0,
            bottom: rows.saturating_sub(1),
        };
        Self {
            cols,
            rows,
            primary: Screen::new(cols, rows, style),
            alternate: Screen::new(cols, rows, style),
            active: ScreenKind::Primary,
            current_style: style,
            state: ParserState::Ground,
            csi_buffer: String::new(),
            csi_intermediate: 0,
            osc_buffer: Vec::new(),
            utf8_buffer: Vec::new(),
            utf8_remaining: 0,
            output: Vec::new(),
            clipboard: Vec::new(),
            bell_count: 0,
            title: None,
            modes: TerminalModes::default(),
            tab_stops: default_tab_stops(cols),
            last_printed: None,
            scroll_region,
            max_scrollback: 1000,
        }
    }

    /// Set the maximum number of scrollback lines.
    pub fn set_max_scrollback(&mut self, max: usize) {
        self.max_scrollback = max;
    }

    /// Process a byte stream incrementally.
    pub fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.advance(byte);
        }
    }

    /// Take accumulated response bytes (DSR replies, DA responses).
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Take accumulated clipboard writes.
    pub fn take_clipboard(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard)
    }

    /// Resize the terminal.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        let style = self.current_style;
        self.primary.resize(self.cols, self.rows, cols, rows, style);
        self.alternate
            .resize(self.cols, self.rows, cols, rows, style);
        self.cols = cols;
        self.rows = rows;
        self.scroll_region = Region {
            top: 0,
            bottom: rows.saturating_sub(1),
        };
        self.tab_stops = default_tab_stops(cols);
    }

    // -- Accessors --

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn active_screen(&self) -> ScreenKind {
        self.active
    }

    pub fn cursor(&self) -> Cursor {
        self.screen().cursor
    }

    pub fn current_style(&self) -> &Style {
        &self.current_style
    }

    pub fn bell_count(&self) -> usize {
        self.bell_count
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Get the cell at the given position.
    pub fn cell(&self, col: usize, row: usize) -> Option<&Cell> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        let screen = self.screen();
        let idx = Screen::index(self.cols, col, row);
        screen.grid.get(idx)
    }

    /// Get the full grid as a flat slice.
    pub fn grid(&self) -> &[Cell] {
        &self.screen().grid
    }

    /// Number of scrollback lines available.
    pub fn scrollback_len(&self) -> usize {
        self.screen().scrollback.len()
    }

    /// Get a scrollback row by index (0 = oldest), trimmed of trailing blanks.
    pub fn scrollback_row(&self, index: usize) -> Option<String> {
        self.screen().scrollback.get(index).map(|row| {
            let end = row
                .iter()
                .rposition(|cell| !cell.is_blank())
                .map_or(0, |idx| idx + 1);
            let mut out = String::new();
            for cell in &row[..end] {
                if !cell.is_wide_continuation() {
                    out.push(cell.ch());
                }
            }
            out
        })
    }

    /// Extract plain text from the visible screen.
    pub fn plain_text(&self) -> String {
        plain_text_for_screen(self.screen(), self.cols, self.rows)
    }

    /// Extract plain text including scrollback.
    pub fn screen_dump(&self) -> String {
        let screen = self.screen();
        let mut out = String::new();

        for row in &screen.scrollback {
            if !out.is_empty() {
                out.push('\n');
            }
            for cell in row {
                if !cell.is_wide_continuation() {
                    out.push(cell.ch());
                }
            }
        }

        let visible = plain_text_for_screen(screen, self.cols, self.rows);
        if !visible.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&visible);
        }

        out
    }

    // -- Mode accessors --

    pub fn wraparound(&self) -> bool {
        self.modes.wraparound
    }
    pub fn cursor_visible(&self) -> bool {
        self.modes.cursor_visible
    }
    pub fn cursor_shape(&self) -> crate::mode::CursorShape {
        self.modes.cursor_shape
    }
    pub fn application_cursor_keys(&self) -> bool {
        self.modes.application_cursor_keys
    }
    pub fn bracketed_paste(&self) -> bool {
        self.modes.bracketed_paste
    }
    pub fn focus_reporting(&self) -> bool {
        self.modes.focus_reporting
    }
    pub fn mouse_tracking(&self) -> Option<crate::mode::MouseTracking> {
        self.modes.mouse_tracking
    }
    pub fn sgr_mouse(&self) -> bool {
        self.modes.sgr_mouse
    }
    pub fn scroll_region(&self) -> (usize, usize) {
        (self.scroll_region.top, self.scroll_region.bottom)
    }

    // -- Internal --

    fn screen(&self) -> &Screen {
        match self.active {
            ScreenKind::Primary => &self.primary,
            ScreenKind::Alternate => &self.alternate,
        }
    }

    fn screen_mut(&mut self) -> &mut Screen {
        match self.active {
            ScreenKind::Primary => &mut self.primary,
            ScreenKind::Alternate => &mut self.alternate,
        }
    }

    fn advance(&mut self, byte: u8) {
        match self.state {
            ParserState::Ground => self.advance_ground(byte),
            ParserState::Escape => self.advance_escape(byte),
            ParserState::Csi => self.advance_csi(byte),
            ParserState::Osc => self.advance_osc(byte),
        }
    }

    fn advance_ground(&mut self, byte: u8) {
        match byte {
            // C0 controls
            0x07 => self.bell_count += 1,
            0x08 => self.backspace(),
            0x09 => self.horizontal_tab(),
            0x0A | 0x0B | 0x0C => self.linefeed(),
            0x0D => self.carriage_return(),
            0x1B => {
                self.state = ParserState::Escape;
            }
            // Printable ASCII
            0x20..=0x7E => {
                let ch = byte as char;
                self.print_char(ch);
            }
            // UTF-8 multi-byte sequences
            0xC0..=0xDF => {
                self.utf8_buffer.clear();
                self.utf8_buffer.push(byte);
                self.utf8_remaining = 1;
            }
            0xE0..=0xEF => {
                self.utf8_buffer.clear();
                self.utf8_buffer.push(byte);
                self.utf8_remaining = 2;
            }
            0xF0..=0xF7 => {
                self.utf8_buffer.clear();
                self.utf8_buffer.push(byte);
                self.utf8_remaining = 3;
            }
            // UTF-8 continuation byte (if we're accumulating)
            0x80..=0xBF if self.utf8_remaining > 0 => {
                self.advance_utf8(byte);
            }
            _ => {} // Ignore other control bytes
        }
    }

    fn advance_escape(&mut self, byte: u8) {
        match byte {
            b'[' => {
                self.csi_buffer.clear();
                self.state = ParserState::Csi;
                return; // Stay in Csi state for next byte
            }
            b']' => {
                self.osc_buffer.clear();
                self.state = ParserState::Osc;
                return; // Stay in Osc state for next byte
            }
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'M' => self.reverse_index(),
            b'c' => self.full_reset(),
            b'H' => self.set_tab_stop(),
            b'(' | b')' => {} // Character set designation -- ignored
            _ => {}
        }
        self.state = ParserState::Ground;
    }

    fn advance_csi(&mut self, byte: u8) {
        match byte {
            // Parameter bytes (0x30-0x3F) -- accumulate
            0x30..=0x3F => {
                self.csi_buffer.push(byte as char);
            }
            // Intermediate bytes (0x20-0x2F) -- store separately
            0x20..=0x2F => {
                self.csi_intermediate = byte;
            }
            // Final byte (0x40-0x7E) -- dispatch
            0x40..=0x7E => {
                let raw = self.csi_buffer.clone();
                let intermediate = self.csi_intermediate;
                let csi = CsiSequence::parse(&raw);
                self.dispatch_csi(byte, &csi, intermediate);
                self.csi_buffer.clear();
                self.csi_intermediate = 0;
                self.state = ParserState::Ground;
            }
            // Abort on other bytes
            _ => {
                self.csi_buffer.clear();
                self.csi_intermediate = 0;
                self.state = ParserState::Ground;
            }
        }
    }

    fn advance_osc(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL terminates OSC
                self.finish_osc();
                self.state = ParserState::Ground;
            }
            0x1B => {
                // ESC might be start of ST (ESC \)
                self.osc_buffer.push(byte);
            }
            b'\\' if self.osc_buffer.last() == Some(&0x1B) => {
                // ESC \ = ST (String Terminator)
                self.osc_buffer.pop(); // Remove the ESC
                self.finish_osc();
                self.state = ParserState::Ground;
            }
            _ => {
                self.osc_buffer.push(byte);
            }
        }
    }

    fn dispatch_csi(&mut self, final_byte: u8, csi: &CsiSequence, intermediate: u8) {
        match final_byte {
            // Cursor movement
            b'A' => self.cursor_up(csi.param_or(0, 1)),
            b'B' => self.cursor_down(csi.param_or(0, 1)),
            b'C' => self.cursor_right(csi.param_or(0, 1)),
            b'D' => self.cursor_left(csi.param_or(0, 1)),
            b'E' => self.cursor_next_line(csi.param_or(0, 1)),
            b'F' => self.cursor_previous_line(csi.param_or(0, 1)),
            b'G' | b'`' => {
                let col = csi.one_based_to_zero(0).min(self.cols.saturating_sub(1));
                self.set_cursor(col, self.cursor().row);
            }
            b'H' | b'f' => {
                let row = csi.one_based_to_zero(0).min(self.rows.saturating_sub(1));
                let col = csi.one_based_to_zero(1).min(self.cols.saturating_sub(1));
                self.set_cursor(col, row);
            }
            b'd' => {
                let row = csi.one_based_to_zero(0).min(self.rows.saturating_sub(1));
                self.set_cursor(self.cursor().col, row);
            }
            // Erasure
            b'J' => self.erase_display(csi.param_or(0, 0)),
            b'K' => self.erase_line(csi.param_or(0, 0)),
            b'X' => self.erase_chars(csi.param_or(0, 1)),
            // Insert/Delete
            b'@' => self.insert_blank_chars(csi.param_or(0, 1)),
            b'P' => self.delete_chars(csi.param_or(0, 1)),
            b'L' => self.insert_lines(csi.param_or(0, 1)),
            b'M' => self.delete_lines(csi.param_or(0, 1)),
            // Scrolling
            b'S' => self.scroll_up_n(csi.param_or(0, 1)),
            b'T' => self.scroll_down_n(csi.param_or(0, 1)),
            // SGR
            b'm' => self.select_graphic_rendition(&csi.params),
            // Modes
            b'h' => self.set_private_modes(&csi.params, true),
            b'l' => self.set_private_modes(&csi.params, false),
            // Cursor shape (DECSCUSR)
            b'q' => {
                // q with space intermediate = cursor shape (DECSCUSR)
                if intermediate == b' ' {
                    self.set_cursor_shape(csi.param_or(0, 0));
                }
            }
            // Device reports
            b'n' => self.device_status_report(csi.private, &csi.params),
            b'c' => self.device_attributes(&csi.raw),
            // Scroll region
            b'r' => self.set_scroll_region(&csi.params),
            // Tabs
            b'I' => self.horizontal_tab_n(csi.param_or(0, 1)),
            b'Z' => self.horizontal_tab_back_n(csi.param_or(0, 1)),
            b'g' => self.clear_tabs(&csi.params),
            b'b' => self.repeat_preceding_char(csi.param_or(0, 1)),
            // Ignore unrecognized sequences
            _ => {}
        }
    }

    fn finish_osc(&mut self) {
        let raw = &self.osc_buffer;
        // OSC sequences are: Ps ; Pt ST
        // Split on first semicolon
        if let Some(semi_pos) = raw.iter().position(|&b| b == b';') {
            let ps = &raw[..semi_pos];
            let pt = &raw[semi_pos + 1..];

            match ps {
                b"0" | b"2" => {
                    // Set window title
                    if let Ok(title) = std::str::from_utf8(pt) {
                        self.title = Some(title.to_string());
                    }
                }
                b"52" => {
                    // Clipboard write (base64 encoded)
                    // Skip the selection character (e.g., 'c')
                    let data_start = if pt.first().map_or(false, |b| b.is_ascii_alphabetic()) {
                        &pt[1..]
                    } else {
                        pt
                    };
                    // We don't decode base64 here -- just store as-is for now
                    if let Ok(text) = std::str::from_utf8(data_start) {
                        self.clipboard.push(text.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    fn full_reset(&mut self) {
        let style = Style::default();
        self.current_style = style;
        self.primary.reset(self.cols, self.rows, style);
        self.alternate.reset(self.cols, self.rows, style);
        self.active = ScreenKind::Primary;
        self.modes = TerminalModes::default();
        self.tab_stops = default_tab_stops(self.cols);
        self.scroll_region = Region {
            top: 0,
            bottom: self.rows.saturating_sub(1),
        };
        self.state = ParserState::Ground;
        self.csi_buffer.clear();
        self.osc_buffer.clear();
        self.utf8_buffer.clear();
        self.utf8_remaining = 0;
        self.last_printed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cols: usize, rows: usize) -> Terminal {
        Terminal::new(cols, rows)
    }

    #[test]
    fn plain_ascii() {
        let mut t = term(10, 2);
        t.write(b"hello");
        assert_eq!(t.plain_text(), "hello");
    }

    #[test]
    fn newline() {
        let mut t = term(10, 3);
        t.write(b"line1\nline2");
        assert_eq!(t.plain_text(), "line1\nline2");
    }

    #[test]
    fn carriage_return_overwrite() {
        let mut t = term(10, 1);
        t.write(b"abc\rXY");
        assert_eq!(t.plain_text(), "XYc");
    }

    #[test]
    fn cursor_movement() {
        let mut t = term(10, 3);
        t.write(b"a\x1B[2;3Hb");
        assert_eq!(t.cursor().row, 1);
        // Cursor advances past the written character (col 2 -> 3)
        assert_eq!(t.cursor().col, 3);
    }

    #[test]
    fn erase_display() {
        let mut t = term(10, 2);
        t.write(b"hello\x1B[2J");
        assert_eq!(t.plain_text(), "");
    }

    #[test]
    fn sgr_bold_and_color() {
        let mut t = term(10, 1);
        t.write(b"\x1B[1;31mX");
        let cell = t.cell(0, 0).unwrap();
        assert!(cell.style().bold);
    }

    #[test]
    fn alternate_screen() {
        let mut t = term(10, 2);
        t.write(b"primary");
        t.write(b"\x1B[?1049h"); // Switch to alt screen
        t.write(b"alternate");
        assert_eq!(t.plain_text(), "alternate");
        assert_eq!(t.active_screen(), ScreenKind::Alternate);
        t.write(b"\x1B[?1049l"); // Back to primary
        assert_eq!(t.plain_text(), "primary");
    }

    #[test]
    fn scrollback_simple() {
        let mut t = term(10, 2);
        t.set_max_scrollback(10);
        t.write(b"line1\nline2");
        // Both lines fit on screen -- no scroll needed
        assert_eq!(t.scrollback_len(), 0);
        assert_eq!(t.plain_text(), "line1\nline2");
    }

    #[test]
    fn scrollback() {
        let mut t = term(10, 2);
        t.set_max_scrollback(10);
        t.write(b"line1\nline2\nline3");
        // One scroll: line1 moves to scrollback, line2 and line3 are on screen
        assert_eq!(t.scrollback_len(), 1);
        assert_eq!(t.scrollback_row(0), Some("line1".to_string()));
        assert_eq!(t.plain_text(), "line2\nline3");
    }

    #[test]
    fn window_title() {
        let mut t = term(10, 2);
        t.write(b"\x1B]0;My Title\x07");
        assert_eq!(t.title(), Some("My Title"));
    }

    #[test]
    fn wide_characters() {
        let mut t = term(10, 1);
        t.write("你".as_bytes());
        assert_eq!(t.cursor().col, 2); // CJK takes 2 columns
    }

    #[test]
    fn cursor_shape() {
        let mut t = term(10, 1);
        t.write(b"\x1B[5 q");
        assert_eq!(t.cursor_shape(), crate::mode::CursorShape::Bar);
    }

    #[test]
    fn resize() {
        let mut t = term(10, 2);
        t.write(b"hello");
        t.resize(20, 5);
        assert_eq!(t.cols(), 20);
        assert_eq!(t.rows(), 5);
        assert!(t.plain_text().contains("hello"));
    }
}
