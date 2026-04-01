use portable_pty::PtySize;

use crate::config::PtyConfig;

use super::raw_vt_buffer::RawVtBuffer;
use super::screen_backend::PtyScreenState;
use super::scrollback::PtyScrollback;

/// In-memory PTY preview renderer for inline live previews.
pub struct PtyPreviewRenderer {
    size: PtySize,
    screen_state: PtyScreenState,
    raw_vt_buffer: RawVtBuffer,
    scrollback: PtyScrollback,
}

impl PtyPreviewRenderer {
    #[must_use]
    pub fn from_config(config: &PtyConfig) -> Self {
        let size = PtySize {
            rows: config.default_rows,
            cols: config.default_cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        Self {
            size,
            screen_state: PtyScreenState::new(
                size,
                config.scrollback_lines,
                config.emulation_backend,
            ),
            raw_vt_buffer: RawVtBuffer::new(config.max_scrollback_bytes),
            scrollback: PtyScrollback::new(config.scrollback_lines, config.max_scrollback_bytes),
        }
    }

    pub fn push_str(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }

        let normalized = normalize_preview_chunk(chunk);
        let bytes = normalized.as_bytes();
        self.screen_state.process(bytes);
        self.raw_vt_buffer.push(bytes);

        let mut utf8 = bytes.to_vec();
        self.scrollback.push_utf8(&mut utf8, false);
    }

    #[must_use]
    pub fn snapshot_text(&self) -> String {
        let raw_vt_stream = self.raw_vt_buffer.snapshot();
        let fallback_scrollback = self.scrollback.snapshot();
        let snapshot = self.screen_state.prepare_snapshot().render(
            self.size,
            &raw_vt_stream,
            &fallback_scrollback,
        );

        let visible = if snapshot.screen_contents.trim().is_empty() {
            snapshot.scrollback
        } else {
            snapshot.screen_contents
        };

        normalize_snapshot_text(&visible)
    }
}

fn normalize_snapshot_text(text: &str) -> String {
    let mut lines = text
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        let _ = lines.pop();
    }

    lines.join("\n")
}

fn normalize_preview_chunk(chunk: &str) -> String {
    let mut normalized = String::with_capacity(chunk.len());
    let mut previous = None;

    for ch in chunk.chars() {
        if ch == '\n' && previous != Some('\r') {
            normalized.push('\r');
        }
        normalized.push(ch);
        previous = Some(ch);
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::PtyPreviewRenderer;
    use crate::config::{PtyConfig, PtyEmulationBackend};

    #[test]
    fn carriage_return_snapshot_keeps_latest_screen_contents() {
        let mut preview = PtyPreviewRenderer::from_config(&PtyConfig::default());
        preview.push_str("start\rreplace\n");

        assert_eq!(preview.snapshot_text(), "replace");
    }

    #[test]
    fn trims_trailing_blank_screen_rows() {
        let mut preview = PtyPreviewRenderer::from_config(&PtyConfig::default());
        preview.push_str("line 1\nline 2\n");

        assert_eq!(preview.snapshot_text(), "line 1\nline 2");
    }

    #[test]
    fn ghostty_backend_falls_back_to_legacy_snapshot_when_runtime_library_is_missing() {
        let mut preview = PtyPreviewRenderer::from_config(&PtyConfig {
            emulation_backend: PtyEmulationBackend::Ghostty,
            ..PtyConfig::default()
        });
        preview.push_str("ghostty fallback\n");

        assert_eq!(preview.snapshot_text(), "ghostty fallback");
    }
}
