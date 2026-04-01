use anyhow::Result;
use portable_pty::PtySize;
use std::sync::OnceLock;
use tracing::warn;
use vt100::Parser;
use vtcode_ghostty_vt_sys::{GhosttyRenderRequest, render_terminal_snapshot};

use crate::config::PtyEmulationBackend;

use super::raw_vt_buffer::RawVtSnapshot;

pub(super) struct PtyScreenState {
    backend: PtyEmulationBackend,
    parser: Parser,
    scrollback_lines: usize,
}

pub(super) struct ScreenSnapshot {
    pub(super) screen_contents: String,
    pub(super) scrollback: String,
}

pub(super) struct PreparedScreenSnapshot {
    backend: PtyEmulationBackend,
    legacy_screen_contents: String,
    scrollback_lines: usize,
}

impl PtyScreenState {
    pub(super) fn new(
        size: PtySize,
        scrollback_lines: usize,
        backend: PtyEmulationBackend,
    ) -> Self {
        Self {
            backend,
            parser: Parser::new(size.rows, size.cols, scrollback_lines),
            scrollback_lines,
        }
    }

    pub(super) fn process(&mut self, chunk: &[u8]) {
        self.parser.process(chunk);
    }

    pub(super) fn resize(&mut self, size: PtySize) {
        self.parser.set_size(size.rows, size.cols);
    }

    pub(super) fn prepare_snapshot(&self) -> PreparedScreenSnapshot {
        PreparedScreenSnapshot {
            backend: self.backend,
            legacy_screen_contents: self.parser.screen().contents(),
            scrollback_lines: self.scrollback_lines,
        }
    }
}

impl PreparedScreenSnapshot {
    pub(super) fn render(
        &self,
        size: PtySize,
        raw_vt_snapshot: &RawVtSnapshot,
        fallback_scrollback: &str,
    ) -> ScreenSnapshot {
        match self.backend {
            PtyEmulationBackend::Ghostty => {
                if raw_vt_snapshot.was_truncated {
                    return self.legacy_snapshot(fallback_scrollback);
                }

                match self.snapshot_with_ghostty(size, &raw_vt_snapshot.bytes) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        warn_ghostty_fallback_once(self.backend, &error);
                        self.legacy_snapshot(fallback_scrollback)
                    }
                }
            }
            PtyEmulationBackend::LegacyVt100 => self.legacy_snapshot(fallback_scrollback),
        }
    }

    fn snapshot_with_ghostty(&self, size: PtySize, raw_vt_stream: &[u8]) -> Result<ScreenSnapshot> {
        let snapshot = render_terminal_snapshot(
            GhosttyRenderRequest {
                cols: size.cols,
                rows: size.rows,
                scrollback_lines: self.scrollback_lines,
            },
            raw_vt_stream,
        )?;
        Ok(ScreenSnapshot {
            screen_contents: snapshot.screen_contents,
            scrollback: snapshot.scrollback,
        })
    }

    fn legacy_snapshot(&self, fallback_scrollback: &str) -> ScreenSnapshot {
        ScreenSnapshot {
            screen_contents: self.legacy_screen_contents.clone(),
            scrollback: fallback_scrollback.to_owned(),
        }
    }
}

fn warn_ghostty_fallback_once(configured_backend: PtyEmulationBackend, error: &anyhow::Error) {
    static WARNED: OnceLock<()> = OnceLock::new();

    if WARNED.set(()).is_ok() {
        warn!(
            configured_backend = configured_backend.as_str(),
            active_backend = PtyEmulationBackend::LegacyVt100.as_str(),
            error = %error,
            "PTY snapshot backend resolved via fallback"
        );
    }
}

#[cfg(test)]
mod tests {
    use portable_pty::PtySize;

    use super::{PreparedScreenSnapshot, PtyScreenState};
    use crate::config::PtyEmulationBackend;

    use super::super::raw_vt_buffer::RawVtSnapshot;

    fn test_size() -> PtySize {
        PtySize {
            rows: 4,
            cols: 10,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[test]
    fn truncated_raw_vt_uses_legacy_snapshot() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100, PtyEmulationBackend::Ghostty);
        state.process(b"hello");

        let prepared = state.prepare_snapshot();
        let snapshot = prepared.render(
            size,
            &RawVtSnapshot {
                bytes: b"\x1b[2J".to_vec(),
                was_truncated: true,
            },
            "hello",
        );

        assert!(snapshot.screen_contents.contains("hello"));
        assert_eq!(snapshot.scrollback, "hello");
    }

    #[test]
    fn legacy_backend_uses_legacy_snapshot() {
        let size = test_size();
        let mut state = PtyScreenState::new(size, 100, PtyEmulationBackend::LegacyVt100);
        state.process(b"legacy");

        let prepared = state.prepare_snapshot();
        assert_legacy_snapshot(prepared, size, "legacy");
    }

    fn assert_legacy_snapshot(prepared: PreparedScreenSnapshot, size: PtySize, expected: &str) {
        let snapshot = prepared.render(
            size,
            &RawVtSnapshot {
                bytes: Vec::new(),
                was_truncated: false,
            },
            expected,
        );

        assert!(snapshot.screen_contents.contains(expected));
        assert_eq!(snapshot.scrollback, expected);
    }
}
