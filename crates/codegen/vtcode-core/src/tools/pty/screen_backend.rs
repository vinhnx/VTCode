use portable_pty::PtySize;
use rio_vt::ansi::CursorShape;
use rio_vt::crosswords::formatter::FormatOptions;
use rio_vt::crosswords::{Crosswords, CrosswordsSize};
use rio_vt::event::{VoidListener, WindowId};
use rio_vt::performer::handler::Processor;
use vtcode_config::constants::defaults::DEFAULT_PTY_SCROLLBACK_LINES;

// ---------------------------------------------------------------------------
// PtyScreenState -- wraps the rio-vt terminal state
// ---------------------------------------------------------------------------

pub(super) struct PtyScreenState {
    term: Crosswords<VoidListener>,
    parser: Processor,
}

impl PtyScreenState {
    pub(super) fn new(size: PtySize, scrollback_lines: usize) -> Self {
        let scrollback = if scrollback_lines == 0 {
            DEFAULT_PTY_SCROLLBACK_LINES
        } else {
            scrollback_lines
        };
        let term = Crosswords::new(
            grid_size(size),
            CursorShape::Block,
            VoidListener,
            WindowId::from(0),
            0,
            scrollback,
        );
        Self {
            term,
            parser: Processor::default(),
        }
    }

    pub(super) fn process(&mut self, chunk: &[u8]) {
        self.parser.advance(&mut self.term, chunk);
    }

    pub(super) fn resize(&mut self, size: PtySize) {
        self.term.resize(grid_size(size));
    }

    pub(super) fn prepare_snapshot(&self) -> ScreenSnapshot {
        ScreenSnapshot {
            screen_contents: self.term.format(FormatOptions::plain()),
        }
    }
}

fn grid_size(size: PtySize) -> CrosswordsSize {
    CrosswordsSize::new(size.cols.max(1) as usize, size.rows.max(1) as usize)
}

// ---------------------------------------------------------------------------
// ScreenSnapshot (returned by prepare_snapshot)
// ---------------------------------------------------------------------------

pub(super) struct ScreenSnapshot {
    pub(super) screen_contents: String,
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
