use portable_pty::PtySize;

use crate::config::PtyConfig;

use super::screen_backend::PtyScreenState;
use super::scrollback::PtyScrollback;

/// In-memory PTY preview renderer for inline live previews.
pub struct PtyPreviewRenderer {
    screen_state: PtyScreenState,
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
            screen_state: PtyScreenState::new(size, config.scrollback_lines),
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

        let mut utf8 = bytes.to_vec();
        self.scrollback.push_utf8(&mut utf8, false);
    }

    #[must_use]
    pub fn snapshot_text(&self) -> String {
        let snapshot = self.screen_state.prepare_snapshot();

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
    use crate::config::PtyConfig;

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
    fn ghostty_core_backend_renders_snapshot() {
        let mut preview = PtyPreviewRenderer::from_config(&PtyConfig::default());
        preview.push_str("ghostty core\n");

        assert_eq!(preview.snapshot_text(), "ghostty core");
    }
}
