use portable_pty::PtySize;
use vt100::Parser;
use vtcode_ghostty_core::Terminal;

use crate::config::PtyEmulationBackend;

// ---------------------------------------------------------------------------
// PtyBackend trait
// ---------------------------------------------------------------------------

/// Abstraction over terminal emulation backends.
///
/// Each backend maintains its own incremental state. Call `process()` as bytes
/// arrive, then `screen_text()` / `scrollback_text()` to extract content.
pub(super) trait PtyBackend: Send {
    /// Feed raw PTY output bytes into the backend.
    fn process(&mut self, chunk: &[u8]);

    /// Resize the emulated terminal.
    fn resize(&mut self, rows: u16, cols: u16);

    /// Extract the current visible screen as plain text.
    fn screen_text(&self) -> String;

    /// Extract accumulated scrollback as plain text.
    fn scrollback_text(&self) -> String;
}

// ---------------------------------------------------------------------------
// GhosttyCoreBackend (pure-Rust, incremental)
// ---------------------------------------------------------------------------

struct GhosttyCoreBackend {
    terminal: Terminal,
}

impl GhosttyCoreBackend {
    fn new(size: PtySize, max_scrollback: usize) -> Self {
        let mut terminal = Terminal::new(size.cols as usize, size.rows as usize);
        terminal.set_max_scrollback(max_scrollback);
        Self { terminal }
    }
}

impl PtyBackend for GhosttyCoreBackend {
    fn process(&mut self, chunk: &[u8]) {
        self.terminal.write(chunk);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.terminal.resize(cols as usize, rows as usize);
    }

    fn screen_text(&self) -> String {
        self.terminal.plain_text()
    }

    fn scrollback_text(&self) -> String {
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
    }
}

// ---------------------------------------------------------------------------
// Vt100Backend (legacy vt100 crate)
// ---------------------------------------------------------------------------

struct Vt100Backend {
    parser: Parser,
}

impl Vt100Backend {
    fn new(size: PtySize, scrollback_lines: usize) -> Self {
        Self {
            parser: Parser::new(size.rows, size.cols, scrollback_lines),
        }
    }
}

impl PtyBackend for Vt100Backend {
    fn process(&mut self, chunk: &[u8]) {
        self.parser.process(chunk);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    fn screen_text(&self) -> String {
        self.parser.screen().contents()
    }

    fn scrollback_text(&self) -> String {
        self.parser.screen().contents()
    }
}

// ---------------------------------------------------------------------------
// PtyScreenState -- uses Box<dyn PtyBackend>
// ---------------------------------------------------------------------------

pub(super) struct PtyScreenState {
    backend: Box<dyn PtyBackend>,
}

impl PtyScreenState {
    pub(super) fn new(
        size: PtySize,
        scrollback_lines: usize,
        backend: PtyEmulationBackend,
        _max_scrollback_bytes: usize,
    ) -> Self {
        let backend: Box<dyn PtyBackend> = match backend {
            PtyEmulationBackend::GhosttyCore => {
                Box::new(GhosttyCoreBackend::new(size, scrollback_lines))
            }
            PtyEmulationBackend::LegacyVt100 => Box::new(Vt100Backend::new(size, scrollback_lines)),
        };
        Self { backend }
    }

    pub(super) fn process(&mut self, chunk: &[u8]) {
        self.backend.process(chunk);
    }

    pub(super) fn resize(&mut self, size: PtySize) {
        self.backend.resize(size.rows, size.cols);
    }

    pub(super) fn prepare_snapshot(&self) -> ScreenSnapshot {
        ScreenSnapshot {
            screen_contents: self.backend.screen_text(),
            scrollback: self.backend.scrollback_text(),
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
    use crate::config::PtyEmulationBackend;

    fn test_size() -> PtySize {
        PtySize {
            rows: 4,
            cols: 10,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[test]
    fn ghostty_core_backend_processes_plain_text() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100, PtyEmulationBackend::GhosttyCore, 0);
        state.process(b"hello");
        let snapshot = state.prepare_snapshot();
        assert!(
            snapshot.screen_contents.contains("hello"),
            "screen_contents = {:?}",
            snapshot.screen_contents
        );
    }

    #[test]
    fn legacy_backend_uses_legacy_snapshot() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100, PtyEmulationBackend::LegacyVt100, 0);
        state.process(b"legacy");
        let snapshot = state.prepare_snapshot();
        assert!(
            snapshot.screen_contents.contains("legacy"),
            "screen_contents = {:?}",
            snapshot.screen_contents
        );
    }

    #[test]
    fn ghostty_core_backend_handles_newlines() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100, PtyEmulationBackend::GhosttyCore, 0);
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
    fn ghostty_core_backend_handles_ansi_colors() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100, PtyEmulationBackend::GhosttyCore, 0);
        state.process(b"\x1B[1;31mcolored\x1B[0m");
        let snapshot = state.prepare_snapshot();
        assert!(
            snapshot.screen_contents.contains("colored"),
            "screen_contents = {:?}",
            snapshot.screen_contents
        );
    }
}
