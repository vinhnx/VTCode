use crate::ui::tui::types::{InlineMessageKind, InlineSegment};

#[derive(Clone)]
pub struct MessageLine {
    pub kind: InlineMessageKind,
    pub segments: Vec<InlineSegment>,
    pub revision: u64,
}

#[derive(Clone, Default)]
pub struct MessageLabels {
    pub agent: Option<String>,
    pub user: Option<String>,
}
