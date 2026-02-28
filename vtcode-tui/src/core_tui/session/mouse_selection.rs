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

        let ((start_col, start_row), (end_col, end_row)) = self.normalized();
        let mut result = String::new();

        for row in start_row..=end_row {
            if row < area.y || row >= area.y + area.height {
                continue;
            }
            let line_start = if row == start_row {
                start_col.max(area.x)
            } else {
                area.x
            };
            let line_end = if row == end_row {
                end_col.min(area.x + area.width)
            } else {
                area.x + area.width
            };

            for col in line_start..line_end {
                if col < area.x || col >= area.x + area.width {
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

        let ((start_col, start_row), (end_col, end_row)) = self.normalized();

        for row in start_row..=end_row {
            if row < area.y || row >= area.y + area.height {
                continue;
            }
            let line_start = if row == start_row {
                start_col.max(area.x)
            } else {
                area.x
            };
            let line_end = if row == end_row {
                end_col.min(area.x + area.width)
            } else {
                area.x + area.width
            };

            for col in line_start..line_end {
                if col < area.x || col >= area.x + area.width {
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

    /// Copy the selected text to the system clipboard using crossterm clipboard commands.
    pub fn copy_to_clipboard(text: &str) {
        if text.is_empty() {
            return;
        }
        let _ = execute!(
            std::io::stderr(),
            CopyToClipboard::to_clipboard_from(text.as_bytes())
        );
        let _ = std::io::stderr().flush();
    }
}
