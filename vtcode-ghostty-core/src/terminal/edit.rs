use crate::cell::Cell;
use crate::cursor::Cursor;
use crate::screen::Screen;
use unicode_width::UnicodeWidthChar;

use super::Terminal;

impl Terminal {
    /// Process a UTF-8 continuation byte.
    pub(crate) fn advance_utf8(&mut self, byte: u8) {
        self.utf8_buffer.push(byte);
        self.utf8_remaining -= 1;

        if self.utf8_remaining == 0 {
            if let Ok(s) = std::str::from_utf8(&self.utf8_buffer) {
                if let Some(ch) = s.chars().next() {
                    self.print_char(ch);
                }
            }
            self.utf8_buffer.clear();
        }
    }

    /// Print a character at the current cursor position.
    pub(crate) fn print_char(&mut self, ch: char) {
        let width = ch.width().unwrap_or(1).max(1);
        let cols = self.cols;
        let rows = self.rows;
        let scroll_bottom = self.scroll_region.bottom;
        let style = self.current_style;

        if cols == 0 || rows == 0 {
            return;
        }

        let screen = self.screen_mut();

        // Handle pending wrap
        if screen.pending_wrap {
            screen.pending_wrap = false;
            screen.cursor.col = 0;
            let new_row = screen.cursor.row + 1;
            if new_row > scroll_bottom {
                // Need to scroll -- drop screen borrow first
                let _ = screen;
                self.scroll_up_n(1);
                let screen = self.screen_mut();
                screen.cursor.row = scroll_bottom;
            } else {
                screen.cursor.row = new_row;
            }
        }

        let screen = self.screen_mut();
        let col = screen.cursor.col;
        let row = screen.cursor.row;

        if col + width > cols {
            screen.pending_wrap = true;
            return;
        }

        let idx = Screen::index(cols, col, row);
        if idx < screen.grid.len() {
            screen.grid[idx] = Cell::printable(ch, style);

            if width == 2 && col + 1 < cols {
                let cont_idx = Screen::index(cols, col + 1, row);
                if cont_idx < screen.grid.len() {
                    screen.grid[cont_idx] = Cell::wide_continuation(style);
                }
            }
        }

        let screen = self.screen_mut();
        screen.cursor.col += width;
        if screen.cursor.col >= cols {
            screen.cursor.col = cols - 1;
            screen.pending_wrap = true;
        }
        self.last_printed = Some(ch);
    }

    /// Repeat the preceding character `count` times.
    pub(crate) fn repeat_preceding_char(&mut self, count: usize) {
        if let Some(ch) = self.last_printed {
            for _ in 0..count {
                self.print_char(ch);
            }
        }
    }

    pub(crate) fn backspace(&mut self) {
        let screen = self.screen_mut();
        if screen.cursor.col > 0 {
            screen.cursor.col -= 1;
        }
        screen.pending_wrap = false;
    }

    pub(crate) fn carriage_return(&mut self) {
        let screen = self.screen_mut();
        screen.cursor.col = 0;
        screen.pending_wrap = false;
    }

    pub(crate) fn linefeed(&mut self) {
        let row = self.screen_mut().cursor.row;
        let region_bottom = self.scroll_region.bottom;

        if row >= region_bottom {
            self.scroll_up_n(1);
            let screen = self.screen_mut();
            screen.cursor.row = region_bottom;
            screen.cursor.col = 0;
            screen.pending_wrap = false;
        } else {
            let screen = self.screen_mut();
            screen.cursor.row += 1;
            screen.cursor.col = 0;
            screen.pending_wrap = false;
        }
    }

    pub(crate) fn reverse_index(&mut self) {
        let region_top = self.scroll_region.top;
        let row = self.screen_mut().cursor.row;

        if row == region_top {
            self.scroll_down_n(1);
        } else {
            self.screen_mut().cursor.row = row.saturating_sub(1);
        }
        self.screen_mut().pending_wrap = false;
    }

    pub(crate) fn cursor_up(&mut self, n: usize) {
        let region_top = self.scroll_region.top;
        let screen = self.screen_mut();
        screen.cursor.row = screen.cursor.row.saturating_sub(n).max(region_top);
        screen.pending_wrap = false;
    }

    pub(crate) fn cursor_down(&mut self, n: usize) {
        let region_bottom = self.scroll_region.bottom;
        let screen = self.screen_mut();
        screen.cursor.row = (screen.cursor.row + n).min(region_bottom);
        screen.pending_wrap = false;
    }

    pub(crate) fn cursor_right(&mut self, n: usize) {
        let max_col = self.cols.saturating_sub(1);
        let screen = self.screen_mut();
        screen.cursor.col = (screen.cursor.col + n).min(max_col);
        screen.pending_wrap = false;
    }

    pub(crate) fn cursor_left(&mut self, n: usize) {
        let screen = self.screen_mut();
        screen.cursor.col = screen.cursor.col.saturating_sub(n);
        screen.pending_wrap = false;
    }

    pub(crate) fn cursor_next_line(&mut self, n: usize) {
        let region_bottom = self.scroll_region.bottom;
        let screen = self.screen_mut();
        screen.cursor.row = (screen.cursor.row + n).min(region_bottom);
        screen.cursor.col = 0;
        screen.pending_wrap = false;
    }

    pub(crate) fn cursor_previous_line(&mut self, n: usize) {
        let region_top = self.scroll_region.top;
        let screen = self.screen_mut();
        screen.cursor.row = screen.cursor.row.saturating_sub(n).max(region_top);
        screen.cursor.col = 0;
        screen.pending_wrap = false;
    }

    pub(crate) fn set_cursor(&mut self, col: usize, row: usize) {
        let screen = self.screen_mut();
        screen.cursor.col = col;
        screen.cursor.row = row;
        screen.pending_wrap = false;
    }

    pub(crate) fn save_cursor(&mut self) {
        let screen = self.screen_mut();
        screen.saved_cursor = screen.cursor;
    }

    pub(crate) fn restore_cursor(&mut self) {
        let saved = self.screen_mut().saved_cursor;
        let screen = self.screen_mut();
        screen.cursor = saved;
        screen.pending_wrap = false;
    }

    pub(crate) fn horizontal_tab(&mut self) {
        let cols = self.cols;
        let start = self.screen_mut().cursor.col + 1;
        for col in start..cols {
            if self.tab_stops.get(col).copied().unwrap_or(false) {
                self.screen_mut().cursor.col = col;
                self.screen_mut().pending_wrap = false;
                return;
            }
        }
        self.screen_mut().cursor.col = cols.saturating_sub(1);
        self.screen_mut().pending_wrap = false;
    }

    pub(crate) fn horizontal_tab_n(&mut self, n: usize) {
        for _ in 0..n {
            self.horizontal_tab();
        }
    }

    pub(crate) fn horizontal_tab_back_n(&mut self, n: usize) {
        for _ in 0..n {
            let col = self.screen_mut().cursor.col;
            if col == 0 {
                break;
            }
            let new_col = col - 1;
            // Find previous tab stop
            let mut found = new_col;
            for c in (0..=new_col).rev() {
                if self.tab_stops.get(c).copied().unwrap_or(false) {
                    found = c;
                    break;
                }
            }
            self.screen_mut().cursor.col = found;
        }
        self.screen_mut().pending_wrap = false;
    }

    pub(crate) fn set_tab_stop(&mut self) {
        if self.cols > 0 {
            let col = self.screen_mut().cursor.col.min(self.cols - 1);
            self.tab_stops[col] = true;
        }
    }

    pub(crate) fn clear_tabs(&mut self, params: &[Option<usize>]) {
        match params.first().copied().flatten().unwrap_or(0) {
            0 => {
                if self.cols > 0 {
                    let col = self.screen_mut().cursor.col.min(self.cols - 1);
                    self.tab_stops[col] = false;
                }
            }
            3 => {
                self.tab_stops.fill(false);
            }
            _ => {}
        }
    }

    pub(crate) fn erase_display(&mut self, mode: usize) {
        let cols = self.cols;
        let style = self.current_style;
        let screen = self.screen_mut();
        let cursor = screen.cursor;

        match mode {
            0 => {
                let start = Screen::index(cols, cursor.col, cursor.row);
                for cell in &mut screen.grid[start..] {
                    *cell = Cell::blank(style);
                }
            }
            1 => {
                let end = Screen::index(cols, cursor.col, cursor.row) + 1;
                let limit = end.min(screen.grid.len());
                for cell in &mut screen.grid[..limit] {
                    *cell = Cell::blank(style);
                }
            }
            2 => {
                for cell in &mut screen.grid {
                    *cell = Cell::blank(style);
                }
            }
            3 => {
                screen.scrollback.clear();
            }
            _ => {}
        }
    }

    pub(crate) fn erase_line(&mut self, mode: usize) {
        let cols = self.cols;
        let style = self.current_style;
        let screen = self.screen_mut();
        let cursor = screen.cursor;
        let row_start = Screen::index(cols, 0, cursor.row);

        match mode {
            0 => {
                for col in cursor.col..cols {
                    let idx = row_start + col;
                    if idx < screen.grid.len() {
                        screen.grid[idx] = Cell::blank(style);
                    }
                }
            }
            1 => {
                for col in 0..=cursor.col.min(cols.saturating_sub(1)) {
                    let idx = row_start + col;
                    if idx < screen.grid.len() {
                        screen.grid[idx] = Cell::blank(style);
                    }
                }
            }
            2 => {
                for col in 0..cols {
                    let idx = row_start + col;
                    if idx < screen.grid.len() {
                        screen.grid[idx] = Cell::blank(style);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn erase_chars(&mut self, count: usize) {
        let cols = self.cols;
        let style = self.current_style;
        let screen = self.screen_mut();
        let cursor = screen.cursor;
        let row_start = Screen::index(cols, 0, cursor.row);

        for i in 0..count {
            let col = cursor.col + i;
            if col >= cols {
                break;
            }
            let idx = row_start + col;
            if idx < screen.grid.len() {
                screen.grid[idx] = Cell::blank(style);
            }
        }
    }

    pub(crate) fn insert_blank_chars(&mut self, count: usize) {
        let cols = self.cols;
        let style = self.current_style;
        let screen = self.screen_mut();
        let cursor = screen.cursor;
        let row_start = Screen::index(cols, 0, cursor.row);
        let row_end = row_start + cols;

        if row_end > screen.grid.len() {
            return;
        }

        let row = &mut screen.grid[row_start..row_end];
        let shift = count.min(cols - cursor.col);
        if shift > 0 && cursor.col + shift < cols {
            row.copy_within(cursor.col..cols - shift, cursor.col + shift);
        }

        for i in 0..shift.min(cols) {
            row[cursor.col + i] = Cell::blank(style);
        }
    }

    pub(crate) fn delete_chars(&mut self, count: usize) {
        let cols = self.cols;
        let style = self.current_style;
        let screen = self.screen_mut();
        let cursor = screen.cursor;
        let row_start = Screen::index(cols, 0, cursor.row);
        let row_end = row_start + cols;

        if row_end > screen.grid.len() {
            return;
        }

        let row = &mut screen.grid[row_start..row_end];
        let delete = count.min(cols - cursor.col);

        if cursor.col + delete < cols {
            row.copy_within(cursor.col + delete..cols, cursor.col);
        }

        for i in (cols - delete)..cols {
            row[i] = Cell::blank(style);
        }
    }

    pub(crate) fn insert_lines(&mut self, count: usize) {
        let row = self.screen_mut().cursor.row;
        let region_bottom = self.scroll_region.bottom;
        let region_top = self.scroll_region.top;

        if row < region_top || row > region_bottom {
            return;
        }

        let cols = self.cols;
        let style = self.current_style;
        let shift = count.min(region_bottom + 1 - row);

        let screen = self.screen_mut();

        // Shift rows down within scroll region
        for r in (row + shift..=region_bottom).rev() {
            let src = r - shift;
            let src_start = Screen::index(cols, 0, src);
            let dst_start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if dst_start + c < screen.grid.len() && src_start + c < screen.grid.len() {
                    screen.grid[dst_start + c] = screen.grid[src_start + c];
                }
            }
        }

        // Fill inserted rows with blanks
        for r in row..row + shift {
            let start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if start + c < screen.grid.len() {
                    screen.grid[start + c] = Cell::blank(style);
                }
            }
        }
    }

    pub(crate) fn delete_lines(&mut self, count: usize) {
        let row = self.screen_mut().cursor.row;
        let region_bottom = self.scroll_region.bottom;
        let region_top = self.scroll_region.top;

        if row < region_top || row > region_bottom {
            return;
        }

        let cols = self.cols;
        let style = self.current_style;
        let shift = count.min(region_bottom + 1 - row);

        let screen = self.screen_mut();

        // Shift rows up within scroll region
        for r in row..region_bottom {
            let src = r + shift;
            let src_start = Screen::index(cols, 0, src);
            let dst_start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if dst_start + c < screen.grid.len() && src_start + c < screen.grid.len() {
                    screen.grid[dst_start + c] = screen.grid[src_start + c];
                }
            }
        }

        // Fill vacated rows with blanks
        for r in (region_bottom + 1 - shift)..=region_bottom {
            let start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if start + c < screen.grid.len() {
                    screen.grid[start + c] = Cell::blank(style);
                }
            }
        }
    }

    pub(crate) fn scroll_up_n(&mut self, count: usize) {
        let cols = self.cols;
        let style = self.current_style;
        let region_top = self.scroll_region.top;
        let region_bottom = self.scroll_region.bottom;
        let is_full_screen = region_top == 0 && region_bottom == self.rows.saturating_sub(1);
        let is_primary = self.active == crate::screen::ScreenKind::Primary;
        let max_scrollback = self.max_scrollback;
        let shift = count.min(region_bottom - region_top + 1);

        let screen = self.screen_mut();

        // Capture scrollback lines if scrolling the full primary screen
        if is_full_screen && is_primary {
            for r in 0..shift {
                let start = Screen::index(cols, 0, r);
                let end = start + cols;
                if end <= screen.grid.len() {
                    let row_cells: Vec<Cell> = screen.grid[start..end].to_vec();
                    screen.scrollback.push(row_cells);
                    if screen.scrollback.len() > max_scrollback {
                        screen.scrollback.remove(0);
                    }
                }
            }
        }

        // Shift rows up
        for r in region_top..=region_bottom - shift {
            let src_start = Screen::index(cols, 0, r + shift);
            let dst_start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if dst_start + c < screen.grid.len() && src_start + c < screen.grid.len() {
                    screen.grid[dst_start + c] = screen.grid[src_start + c];
                }
            }
        }

        // Fill bottom rows with blanks
        for r in (region_bottom + 1 - shift)..=region_bottom {
            let start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if start + c < screen.grid.len() {
                    screen.grid[start + c] = Cell::blank(style);
                }
            }
        }
    }

    pub(crate) fn scroll_down_n(&mut self, count: usize) {
        let cols = self.cols;
        let style = self.current_style;
        let region_top = self.scroll_region.top;
        let region_bottom = self.scroll_region.bottom;
        let shift = count.min(region_bottom - region_top + 1);

        let screen = self.screen_mut();

        // Shift rows down
        for r in (region_top + shift..=region_bottom).rev() {
            let src_start = Screen::index(cols, 0, r - shift);
            let dst_start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if dst_start + c < screen.grid.len() && src_start + c < screen.grid.len() {
                    screen.grid[dst_start + c] = screen.grid[src_start + c];
                }
            }
        }

        // Fill top rows with blanks
        for r in region_top..region_top + shift {
            let start = Screen::index(cols, 0, r);
            for c in 0..cols {
                if start + c < screen.grid.len() {
                    screen.grid[start + c] = Cell::blank(style);
                }
            }
        }
    }

    pub(crate) fn set_scroll_region(&mut self, params: &[Option<usize>]) {
        let top = params
            .first()
            .and_then(|p| *p)
            .unwrap_or(1)
            .saturating_sub(1)
            .min(self.rows.saturating_sub(1));
        let bottom = params
            .get(1)
            .and_then(|p| *p)
            .unwrap_or(self.rows)
            .saturating_sub(1)
            .min(self.rows.saturating_sub(1));

        if top < bottom {
            self.scroll_region = crate::region::Region { top, bottom };
        }

        self.set_cursor(0, 0);
    }

    pub(crate) fn switch_alternate_screen(&mut self, enabled: bool) {
        let new_kind = if enabled {
            crate::screen::ScreenKind::Alternate
        } else {
            crate::screen::ScreenKind::Primary
        };
        if self.active != new_kind {
            self.active = new_kind;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn screen_alignment_test(&mut self) {
        let style = crate::style::Style::default();
        let screen = self.screen_mut();
        for cell in &mut screen.grid {
            *cell = Cell::printable('E', style);
        }
        screen.cursor = Cursor { col: 0, row: 0 };
    }

    pub(crate) fn select_graphic_rendition(&mut self, params: &[Option<usize>]) {
        // Handle colon-separated extended color sequences
        if params.len() >= 4 {
            if let (Some(38), Some(type_param)) = (params[0], params[1]) {
                match type_param {
                    5 => {
                        if let Some(idx) = params.get(2).and_then(|p| *p) {
                            if idx <= 255 {
                                self.current_style.fg =
                                    Some(crate::color::Color::Indexed(idx as u8));
                            }
                        }
                        return;
                    }
                    2 => {
                        let r = params.get(3).and_then(|p| *p);
                        let g = params.get(4).and_then(|p| *p);
                        let b = params.get(5).and_then(|p| *p);
                        if let (Some(r), Some(g), Some(b)) = (r, g, b) {
                            if r <= 255 && g <= 255 && b <= 255 {
                                self.current_style.fg = Some(crate::color::Color::Rgb {
                                    r: r as u8,
                                    g: g as u8,
                                    b: b as u8,
                                });
                            }
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }

        self.current_style.apply_sgr(params);
    }
}
