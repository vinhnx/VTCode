use std::collections::VecDeque;

use gpui_kit::{FollowMode, ListAlignment, ListState, SharedString, px};

use super::markdown::prepare_markdown;

const MAX_TRANSCRIPT_ITEMS: usize = 2_000;

#[derive(Clone, Debug)]
pub(super) struct TranscriptItem {
    pub id: String,
    pub role: String,
    pub title: String,
    pub text: SharedString,
    pub markdown: SharedString,
    pub expanded: bool,
    pub diff: Option<DiffCard>,
}

/// Structured payload for a file-change activity row.
#[derive(Clone, Debug)]
pub(super) struct DiffCard {
    pub files: Vec<DiffFile>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub incomplete: bool,
    pub failed: bool,
    pub patch: String,
}

/// One file inside a [`DiffCard`], with its change kind.
#[derive(Clone, Debug)]
pub(super) struct DiffFile {
    pub path: String,
    pub kind: DiffFileKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DiffFileKind {
    Add,
    Delete,
    Update,
}

impl TranscriptItem {
    pub fn new(id: String, role: String, title: String, text: String) -> Self {
        let text = SharedString::from(text);
        let markdown = prepare_markdown(&text);
        Self { id, role, title, text, markdown, expanded: false, diff: None }
    }
}

/// Owns row data together with GPUI's measured geometry and scroll anchor.
pub(super) struct Transcript {
    items: VecDeque<TranscriptItem>,
    list: ListState,
}

impl Default for Transcript {
    fn default() -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(200.));
        list.set_follow_mode(FollowMode::Tail);
        Self { items: VecDeque::new(), list }
    }
}

impl Transcript {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&TranscriptItem> {
        self.items.get(index)
    }

    pub fn list_state(&self) -> ListState {
        self.list.clone()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.list.reset(0);
        self.follow_latest();
    }

    pub fn push(&mut self, item: TranscriptItem) {
        if self.items.len() == MAX_TRANSCRIPT_ITEMS {
            let _removed = self.items.pop_front();
            self.list.splice(0..1, 0);
        }
        let index = self.items.len();
        self.items.push_back(item);
        self.list.splice(index..index, 1);
    }

    pub fn upsert(&mut self, mut item: TranscriptItem) {
        if let Some(index) = self.items.iter().position(|existing| existing.id == item.id) {
            if let Some(existing) = self.items.get_mut(index) {
                item.expanded = existing.expanded;
                *existing = item;
            }
            self.list.remeasure_items(index..index + 1);
        } else {
            self.push(item);
        }
    }

    pub fn toggle_tool(&mut self, id: &str) {
        if let Some(index) = self.items.iter().position(|item| item.id == id && item.role == "tool") {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.list.pause_following_tail();
            self.list.remeasure_items(index..index + 1);
        }
    }

    pub fn toggle_diff(&mut self, id: &str) {
        if let Some(index) = self.items.iter().position(|item| item.id == id && item.role == "diff") {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.list.pause_following_tail();
            self.list.remeasure_items(index..index + 1);
        }
    }

    pub fn follow_latest(&self) {
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
    }

    pub fn remeasure(&self) {
        self.list.remeasure();
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{ListOffset, px};

    use super::{MAX_TRANSCRIPT_ITEMS, Transcript, TranscriptItem};

    fn item(id: usize, role: &str, text: &str) -> TranscriptItem {
        TranscriptItem::new(id.to_string(), role.to_string(), "Activity".to_string(), text.to_string())
    }

    #[test]
    fn bounds_rows_and_preserves_reader_anchor_when_oldest_row_is_evicted() {
        let mut transcript = Transcript::default();
        for id in 0..MAX_TRANSCRIPT_ITEMS {
            transcript.push(item(id, "assistant", "row"));
        }
        let list = transcript.list_state();
        list.scroll_to(ListOffset { item_ix: 12, offset_in_item: px(7.) });
        transcript.push(item(MAX_TRANSCRIPT_ITEMS, "assistant", "new row"));

        assert_eq!(list.item_count(), MAX_TRANSCRIPT_ITEMS);
        assert_eq!(transcript.get(0).unwrap().id, "1");
        let anchor = list.logical_scroll_top();
        assert_eq!(transcript.get(anchor.item_ix).unwrap().id, "12");
        assert_eq!(anchor.offset_in_item, px(7.));
        assert!(!list.is_following_tail());
        assert_eq!(transcript.get(MAX_TRANSCRIPT_ITEMS - 1).unwrap().text, "new row");
    }

    #[test]
    fn updates_keep_tool_expansion_and_reset_restores_tail_following() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "before"));
        transcript.push(item(2, "assistant", "reply"));
        transcript.toggle_tool("1");
        transcript.upsert(item(1, "tool", "after\nwith details"));

        assert_eq!(transcript.list_state().item_count(), 2);
        assert!(transcript.get(0).unwrap().expanded);
        assert_eq!(transcript.get(0).unwrap().text, "after\nwith details");
        assert!(!transcript.list_state().is_following_tail());
        transcript.clear();
        assert!(transcript.is_empty());
        assert_eq!(transcript.list_state().item_count(), 0);
        assert!(transcript.list_state().is_following_tail());
    }
}
