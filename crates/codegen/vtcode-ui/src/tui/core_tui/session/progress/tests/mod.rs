pub(super) use super::*;
pub(super) use crate::tui::core_tui::types::InlineSegment;
pub(super) use crate::tui::core_tui::widgets::TranscriptWidget;
pub(super) use ratatui::{Terminal, backend::TestBackend};

fn rendered_text(buf: &Buffer) -> String {
    buf.content.iter().map(|cell| cell.symbol()).collect()
}

#[derive(Clone)]
struct DiagnosticWriter(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for DiagnosticWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

mod activity;
mod feedback;
mod footer;
mod layout;
