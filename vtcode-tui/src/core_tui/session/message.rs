use ratatui::text::Line;

use crate::ui::tui::types::{InlineLinkRange, InlineLinkTarget, InlineMessageKind, InlineSegment};

#[derive(Clone)]
pub struct MessageLine {
    pub kind: InlineMessageKind,
    pub segments: Vec<InlineSegment>,
    pub link_ranges: Vec<InlineLinkRange>,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedTranscriptLink {
    pub start: usize,
    pub end: usize,
    pub start_col: usize,
    pub width: usize,
    pub target: InlineLinkTarget,
}

#[derive(Clone, Debug, Default)]
pub struct TranscriptLine {
    pub line: Line<'static>,
    pub explicit_links: Vec<RenderedTranscriptLink>,
}

#[derive(Clone, Default)]
pub struct MessageLabels {
    pub agent: Option<String>,
    pub user: Option<String>,
}
