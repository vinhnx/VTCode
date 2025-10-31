use ratatui::text::Line;

#[derive(Default)]
pub(super) struct CachedMessage {
    pub(super) revision: u64,
    pub(super) lines: Vec<Line<'static>>,
}

pub(super) struct TranscriptReflowCache {
    pub(super) width: u16,
    pub(super) total_rows: usize,
    pub(super) row_offsets: Vec<usize>,
    pub(super) messages: Vec<CachedMessage>,
}

impl TranscriptReflowCache {
    pub(super) fn new(width: u16) -> Self {
        Self {
            width,
            total_rows: 0,
            row_offsets: Vec::new(),
            messages: Vec::new(),
        }
    }
}
