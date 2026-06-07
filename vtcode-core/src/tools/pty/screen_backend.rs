use portable_pty::PtySize;
use vtcode_ghostty_core::Terminal;

// ---------------------------------------------------------------------------
// PtyScreenState -- wraps GhosttyCore terminal
// ---------------------------------------------------------------------------

pub(super) struct PtyScreenState {
    terminal: Terminal,
}

impl PtyScreenState {
    pub(super) fn new(size: PtySize, scrollback_lines: usize) -> Self {
        let mut terminal = Terminal::new(size.cols as usize, size.rows as usize);
        terminal.set_max_scrollback(scrollback_lines);
        Self { terminal }
    }

    pub(super) fn process(&mut self, chunk: &[u8]) {
        self.terminal.write(chunk);
    }

    pub(super) fn resize(&mut self, size: PtySize) {
        self.terminal.resize(size.cols as usize, size.rows as usize);
    }

    pub(super) fn prepare_snapshot(&self) -> ScreenSnapshot {
        let scrollback = {
            let mut out = String::new();
            for i in 0..self.terminal.scrollback_len() {
                if let Some(row) = self.terminal.scrollback_row(i) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&row);
                }
            }
            out
        };
        ScreenSnapshot {
            screen_contents: self.terminal.plain_text(),
            scrollback,
        }
    }
}

// ---------------------------------------------------------------------------
// ScreenSnapshot (returned by prepare_snapshot)
// ---------------------------------------------------------------------------

pub(super) struct ScreenSnapshot {
    pub(super) screen_contents: String,
    pub(super) scrollback: String,
}

#[cfg(test)]
mod tests {
    use portable_pty::PtySize;

    use super::PtyScreenState;

    fn test_size() -> PtySize {
        PtySize {
            rows: 4,
            cols: 10,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[test]
    fn backend_processes_plain_text() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100);
        state.process(b"hello");
        let snapshot = state.prepare_snapshot();
        assert!(
            snapshot.screen_contents.contains("hello"),
            "screen_contents = {:?}",
            snapshot.screen_contents
        );
    }

    #[test]
    fn backend_handles_newlines() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100);
        state.process(b"line1\nline2");
        let snapshot = state.prepare_snapshot();
        assert!(
            snapshot.screen_contents.contains("line1"),
            "screen_contents = {:?}",
            snapshot.screen_contents
        );
        assert!(
            snapshot.screen_contents.contains("line2"),
            "screen_contents = {:?}",
            snapshot.screen_contents
        );
    }

    #[test]
    fn backend_handles_ansi_colors() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100);
        state.process(b"\x1B[1;31mcolored\x1B[0m");
        let snapshot = state.prepare_snapshot();
        assert!(
            snapshot.screen_contents.contains("colored"),
            "screen_contents = {:?}",
            snapshot.screen_contents
        );
    }
}
