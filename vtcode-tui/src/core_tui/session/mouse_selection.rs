use ratatui::buffer::Buffer;
use ratatui::crossterm::{clipboard::CopyToClipboard, execute};
use ratatui::layout::Rect;
use std::io::Write;
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(450);

/// Tracks mouse-driven text selection state for the TUI transcript.
#[derive(Debug, Default)]
pub struct MouseSelectionState {
    /// Whether the user is currently dragging to select text.
    pub is_selecting: bool,
    /// Screen coordinates where the selection started (column, row).
    pub start: (u16, u16),
    /// Screen coordinates where the selection currently ends (column, row).
    pub end: (u16, u16),
    /// Whether a completed selection exists (ready for highlight rendering).
    pub has_selection: bool,
    /// Whether the current selection has already been copied to clipboard.
    copied: bool,
    /// Whether Ctrl+C was pressed to explicitly copy the current selection.
    copy_requested: bool,
    /// Tracks the previous mouse click so double-clicks can be detected.
    last_click: Option<ClickRecord>,
}

#[derive(Clone, Copy, Debug)]
struct ClickRecord {
    column: u16,
    row: u16,
    at: Instant,
}

impl MouseSelectionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a new selection at the given screen position.
    pub fn start_selection(&mut self, col: u16, row: u16) {
        self.is_selecting = true;
        self.has_selection = false;
        self.copied = false;
        self.start = (col, row);
        self.end = (col, row);
    }

    /// Set a selection directly, bypassing drag state.
    pub fn set_selection(&mut self, start: (u16, u16), end: (u16, u16)) {
        self.is_selecting = false;
        self.has_selection = start != end;
        self.copied = false;
        self.start = start;
        self.end = end;
    }

    /// Update the end position while dragging.
    pub fn update_selection(&mut self, col: u16, row: u16) {
        if self.is_selecting {
            self.end = (col, row);
            self.has_selection = true;
        }
    }

    /// Finalize the selection on mouse-up.
    pub fn finish_selection(&mut self, col: u16, row: u16) {
        if self.is_selecting {
            self.end = (col, row);
            self.is_selecting = false;
            // Only mark as having a selection if start != end
            self.has_selection = self.start != self.end;
        }
    }

    /// Adjust selection row coordinates after a scroll event.
    ///
    /// `row_delta` is positive when content moves down on screen (scroll up / showing
    /// older content) and negative when content moves up (scroll down / showing newer
    /// content).  If the adjustment pushes the selection completely off-screen the
    /// selection is cleared.
    pub fn adjust_for_scroll(&mut self, row_delta: i32) {
        if !self.has_selection && !self.is_selecting {
            return;
        }
        if row_delta == 0 {
            return;
        }

        let new_start_row = self.start.1 as i32 + row_delta;
        let new_end_row = self.end.1 as i32 + row_delta;

        // If both ends are completely off-screen in the same direction, clear.
        // Clamp to screen bounds (0..=viewport_height, roughly 0..=u16::MAX).
        // If after clamping both are the same and off-screen, or both are off-screen
        // in a way that suggests selection is gone, clear.

        let clamped_start = new_start_row.clamp(0, i32::from(u16::MAX));
        let clamped_end = new_end_row.clamp(0, i32::from(u16::MAX));

        // If the selection is now completely off-screen in a way that means
        // the original selection range was entirely off-screen, clear.
        if (new_start_row < 0 && new_end_row < 0)
            || (new_start_row > i32::from(u16::MAX) && new_end_row > i32::from(u16::MAX))
        {
            self.is_selecting = false;
            self.has_selection = false;
            self.copied = false;
            self.copy_requested = false;
            return;
        }

        self.start.1 = clamped_start as u16;
        self.end.1 = clamped_end as u16;
    }

    /// Clear any active selection.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.is_selecting = false;
        self.has_selection = false;
        self.copied = false;
        self.copy_requested = false;
        self.last_click = None;
    }

    /// Clears only the mouse click history used for double-click detection.
    pub fn clear_click_history(&mut self) {
        self.last_click = None;
    }

    /// Records a click and returns `true` when it matches the previous click closely enough
    /// to be treated as a double click.
    pub fn register_click(&mut self, col: u16, row: u16, at: Instant) -> bool {
        let is_double_click = self.last_click.is_some_and(|last| {
            last.column == col
                && last.row == row
                && at.saturating_duration_since(last.at) <= DOUBLE_CLICK_INTERVAL
        });

        self.last_click = Some(ClickRecord {
            column: col,
            row,
            at,
        });
        is_double_click
    }

    /// Returns the selection range normalized so that `from` is before `to`.
    fn normalized(&self) -> ((u16, u16), (u16, u16)) {
        let (s, e) = (self.start, self.end);
        if s.1 < e.1 || (s.1 == e.1 && s.0 <= e.0) {
            (s, e)
        } else {
            (e, s)
        }
    }

    /// Extract selected text from a ratatui `Buffer`.
    pub fn extract_text(&self, buf: &Buffer, area: Rect) -> String {
        if !self.has_selection && !self.is_selecting {
            return String::new();
        }

        // Clamp to the actual buffer area to avoid out-of-range buffer indexing panics.
        let area = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 {
            return String::new();
        }

        let ((start_col, start_row), (end_col, end_row)) = self.normalized();
        let mut result = String::new();

        for row in start_row..=end_row {
            if row < area.y || row >= area.bottom() {
                continue;
            }
            let line_start = if row == start_row {
                start_col.max(area.x)
            } else {
                area.x
            };
            let line_end = if row == end_row {
                end_col.min(area.right())
            } else {
                area.right()
            };

            for col in line_start..line_end {
                if col < area.x || col >= area.right() {
                    continue;
                }
                let cell = &buf[(col, row)];
                let symbol = cell.symbol();
                if !symbol.is_empty() {
                    result.push_str(symbol);
                }
            }

            // Add newline between rows (but not after the last)
            if row < end_row {
                // Trim trailing whitespace from each line
                let trimmed = result.trim_end().len();
                result.truncate(trimmed);
                result.push('\n');
            }
        }

        // Trim trailing whitespace from the final line
        let trimmed = result.trim_end();
        trimmed.to_string()
    }

    /// Apply selection highlight (inverted colors) to the frame buffer.
    pub fn apply_highlight(&self, buf: &mut Buffer, area: Rect) {
        if !self.has_selection && !self.is_selecting {
            return;
        }

        // Clamp to the actual buffer area to avoid out-of-range buffer indexing panics.
        let area = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 {
            return;
        }

        let ((start_col, start_row), (end_col, end_row)) = self.normalized();

        for row in start_row..=end_row {
            if row < area.y || row >= area.bottom() {
                continue;
            }
            let line_start = if row == start_row {
                start_col.max(area.x)
            } else {
                area.x
            };
            let line_end = if row == end_row {
                end_col.min(area.right())
            } else {
                area.right()
            };

            for col in line_start..line_end {
                if col < area.x || col >= area.right() {
                    continue;
                }
                let cell = &mut buf[(col, row)];
                // Swap foreground and background to show selection
                let fg = cell.fg;
                let bg = cell.bg;
                cell.set_fg(bg);
                cell.set_bg(fg);
            }
        }
    }

    /// Returns `true` if the selection needs to be copied (finalized and not yet copied).
    pub fn needs_copy(&self) -> bool {
        self.has_selection && !self.is_selecting && !self.copied
    }

    /// Returns `true` if an explicit copy was requested via Ctrl+C.
    pub fn has_copy_request(&self) -> bool {
        self.copy_requested
    }

    /// Request an explicit copy of the current selection (triggered by Ctrl+C).
    pub fn request_copy(&mut self) {
        if self.has_selection {
            self.copy_requested = true;
        }
    }

    /// Mark the selection as already copied.
    pub fn mark_copied(&mut self) {
        self.copied = true;
    }

    /// Copy the selected text to the system clipboard.
    ///
    /// Tries native OS clipboard utilities first (`pbcopy` on macOS, `xclip`/`xsel`
    /// on Linux, `clip.exe` on Windows/WSL) for maximum compatibility, then falls
    /// back to the OSC 52 escape sequence.
    pub fn copy_to_clipboard(text: &str) {
        if text.is_empty() {
            return;
        }

        if Self::copy_via_native(text) {
            return;
        }

        // Fallback: OSC 52 escape sequence
        let _ = execute!(
            std::io::stderr(),
            CopyToClipboard::to_clipboard_from(text.as_bytes())
        );
        let _ = std::io::stderr().flush();
    }

    /// Attempt to copy text using native OS clipboard utilities.
    /// Returns `true` if successful.
    fn copy_via_native(text: &str) -> bool {
        use std::process::Command;

        #[cfg(test)]
        if let Some(program) = clipboard_command_override() {
            return spawn_clipboard_command(Command::new(program), text);
        }

        let candidates: &[&str] = if cfg!(target_os = "macos") {
            &["pbcopy"]
        } else if cfg!(target_os = "linux") {
            &["xclip", "xsel"]
        } else if cfg!(target_os = "windows") {
            &["clip.exe"]
        } else {
            &[]
        };

        for program in candidates {
            let mut cmd = Command::new(program);
            match *program {
                "xclip" => {
                    cmd.arg("-selection").arg("clipboard");
                }
                "xsel" => {
                    cmd.arg("--clipboard").arg("--input");
                }
                _ => {}
            }
            if spawn_clipboard_command(cmd, text) {
                return true;
            }
        }
        false
    }
}

fn spawn_clipboard_command(mut cmd: std::process::Command, text: &str) -> bool {
    use std::process::Stdio;

    let Ok(mut child) = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(text.as_bytes());
    }
    drop(child.stdin.take());
    child.wait().is_ok()
}

#[cfg(test)]
static CLIPBOARD_COMMAND_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn set_clipboard_command_override(path: Option<PathBuf>) {
    let lock = CLIPBOARD_COMMAND_OVERRIDE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = path;
    }
}

#[cfg(test)]
pub(crate) fn clipboard_command_override() -> Option<PathBuf> {
    let lock = CLIPBOARD_COMMAND_OVERRIDE.get_or_init(|| Mutex::new(None));
    match lock.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    }
}

/// Return the half-open display-column range for the word under `column`.
pub(crate) fn word_selection_range(text: &str, column: u16) -> Option<(u16, u16)> {
    if text.is_empty() {
        return None;
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let line_width = UnicodeWidthStr::width(text);
    if usize::from(column) >= line_width {
        return None;
    }

    let mut consumed = 0usize;
    let mut char_index = 0usize;
    for ch in &chars {
        let width = UnicodeWidthChar::width(*ch).unwrap_or(0);
        if consumed.saturating_add(width) > usize::from(column) {
            break;
        }
        consumed = consumed.saturating_add(width);
        char_index += 1;
    }

    if char_index >= chars.len() || chars[char_index].is_whitespace() {
        return None;
    }

    let mut start = char_index;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }

    let mut end = char_index + 1;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }

    Some((
        display_width_for_char_count(&chars, start),
        display_width_for_char_count(&chars, end),
    ))
}

fn display_width_for_char_count(chars: &[char], char_count: usize) -> u16 {
    chars
        .iter()
        .take(char_count)
        .map(|ch| UnicodeWidthChar::width(*ch).unwrap_or(0) as u16)
        .fold(0_u16, u16::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use std::time::{Duration, Instant};

    #[test]
    fn extract_text_clamps_area_to_buffer_bounds() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        buf[(0, 0)].set_symbol("A");
        buf[(1, 0)].set_symbol("B");
        buf[(0, 1)].set_symbol("C");
        buf[(1, 1)].set_symbol("D");

        let mut selection = MouseSelectionState::new();
        selection.start_selection(0, 0);
        selection.finish_selection(5, 5);

        let text = selection.extract_text(&buf, Rect::new(0, 0, 10, 10));
        assert_eq!(text, "AB\nCD");
    }

    #[test]
    fn apply_highlight_clamps_area_to_buffer_bounds() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf[(0, 0)].set_fg(Color::Red);
        buf[(0, 0)].set_bg(Color::Blue);

        let mut selection = MouseSelectionState::new();
        selection.start_selection(0, 0);
        selection.finish_selection(5, 5);

        selection.apply_highlight(&mut buf, Rect::new(0, 0, 10, 10));

        assert_eq!(buf[(0, 0)].fg, Color::Blue);
        assert_eq!(buf[(0, 0)].bg, Color::Red);
    }

    #[test]
    fn word_selection_range_selects_clicked_word() {
        assert_eq!(word_selection_range("hello world", 1), Some((0, 5)));
        assert_eq!(word_selection_range("hello world", 7), Some((6, 11)));
    }

    #[test]
    fn word_selection_range_returns_none_for_whitespace() {
        assert_eq!(word_selection_range("hello world", 5), None);
    }

    #[test]
    fn adjust_for_scroll_shifts_rows() {
        let mut sel = MouseSelectionState::new();
        sel.set_selection((2, 5), (10, 8));

        sel.adjust_for_scroll(3);
        assert_eq!(sel.start, (2, 8));
        assert_eq!(sel.end, (10, 11));
        assert!(sel.has_selection);
    }

    #[test]
    fn adjust_for_scroll_negative() {
        let mut sel = MouseSelectionState::new();
        sel.set_selection((0, 10), (5, 15));

        sel.adjust_for_scroll(-4);
        assert_eq!(sel.start, (0, 6));
        assert_eq!(sel.end, (5, 11));
    }

    #[test]
    fn adjust_for_scroll_clears_when_offscreen() {
        let mut sel = MouseSelectionState::new();
        sel.set_selection((0, 2), (5, 4));

        sel.adjust_for_scroll(-10);
        assert!(!sel.has_selection);
        assert!(!sel.is_selecting);
    }

    #[test]
    fn adjust_for_scroll_noop_without_selection() {
        let mut sel = MouseSelectionState::new();
        sel.adjust_for_scroll(5);
        assert!(!sel.has_selection);
    }

    #[test]
    fn register_click_detects_double_clicks_at_same_position() {
        let mut selection = MouseSelectionState::new();
        let now = Instant::now();

        assert!(!selection.register_click(3, 7, now));
        assert!(selection.register_click(3, 7, now + Duration::from_millis(250)));
        assert!(!selection.register_click(4, 7, now + Duration::from_millis(250)));
    }
}
