use ratatui::buffer::Buffer;
use ratatui::crossterm::{clipboard::CopyToClipboard, execute};
use ratatui::layout::Rect;
use std::io::Write;

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

    /// Clear any active selection.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.is_selecting = false;
        self.has_selection = false;
        self.copied = false;
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
        use std::process::{Command, Stdio};

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
            let Ok(mut child) = cmd
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            drop(child.stdin.take());
            if child.wait().is_ok() {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

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
}
