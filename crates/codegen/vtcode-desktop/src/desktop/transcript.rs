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
    pub accessibility_text: SharedString,
    pub expanded: bool,
    pub activity_expanded: bool,
    /// Whether this row carries structured activity status rather than archive text.
    pub has_activity_status: bool,
    pub diff: Option<DiffCard>,
    /// Raw aggregated command/tool output for the Output inspector tab.
    pub output: Option<String>,
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
        let prepared = prepare_markdown(&text);
        Self {
            id,
            has_activity_status: role == "tool",
            role,
            title,
            text,
            markdown: prepared.markdown,
            accessibility_text: prepared.accessible_text,
            expanded: false,
            activity_expanded: false,
            diff: None,
            output: None,
        }
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

    /// Rows in chronological order.
    pub fn items(&self) -> impl DoubleEndedIterator<Item = &TranscriptItem> {
        self.items.iter()
    }

    /// Rows newest first, for reverse projections such as the inspector.
    pub fn items_newest_first(&self) -> impl Iterator<Item = &TranscriptItem> {
        self.items().rev()
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
            self.list.remeasure();
        }
        let index = self.items.len();
        self.items.push_back(item);
        self.list.splice(index..index, 1);
        if let Some(group) = self.activity_group(index) {
            self.list.remeasure_items(group);
        }
    }

    /// Remove a local unsent echo together with its measured row.
    pub fn remove(&mut self, id: &str) {
        if let Some(index) = self.items.iter().position(|item| item.id == id) {
            let _removed = self.items.remove(index);
            self.list.splice(index..index + 1, 0);
            self.list.remeasure();
        }
    }

    pub fn upsert(&mut self, mut item: TranscriptItem) {
        if let Some(index) = self.items.iter().position(|existing| existing.id == item.id) {
            let role_changed = self.items.get(index).is_some_and(|existing| existing.role != item.role);
            if let Some(existing) = self.items.get_mut(index) {
                item.expanded = existing.expanded;
                item.activity_expanded = existing.activity_expanded;
                *existing = item;
            }
            if role_changed {
                self.list.remeasure();
            } else {
                self.list.remeasure_items(index..index + 1);
            }
        } else {
            self.push(item);
        }
    }

    /// Attach or replace raw command output on an existing row.
    pub fn set_output(&mut self, id: &str, output: Option<String>) {
        if let Some(index) = self.items.iter().position(|item| item.id == id) {
            if let Some(item) = self.items.get_mut(index) {
                item.output = output;
            }
            self.list.remeasure_items(index..index + 1);
        }
    }

    /// Consecutive tool rows form a presentation group without changing event identities or ordering.
    pub fn activity_group(&self, index: usize) -> Option<std::ops::Range<usize>> {
        if self.items.get(index)?.role != "tool" {
            return None;
        }
        let mut start = index;
        while start > 0 && self.items.get(start - 1).is_some_and(|item| item.role == "tool") {
            start -= 1;
        }
        let mut end = index + 1;
        while self.items.get(end).is_some_and(|item| item.role == "tool") {
            end += 1;
        }
        (end - start > 1).then_some(start..end)
    }

    pub fn toggle_activity_group(&mut self, id: &str) {
        if let Some(index) = self.items.iter().position(|item| item.id == id)
            && let Some(range) = self.activity_group(index)
        {
            if let Some(head) = self.items.get_mut(range.start) {
                head.activity_expanded = !head.activity_expanded;
            }
            self.list.pause_following_tail();
            self.list.remeasure_items(range);
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
    fn activity_group_boundaries_follow_removal_role_changes_and_eviction() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "completed"));
        transcript.push(item(2, "tool", "running"));
        transcript.push(item(3, "assistant", "commentary"));
        transcript.push(item(4, "tool", "completed"));
        assert_eq!(transcript.activity_group(1), Some(0..2));
        assert_eq!(transcript.activity_group(3), None);
        transcript.toggle_activity_group("1");
        transcript.upsert(item(1, "tool", "completed with output"));
        assert!(transcript.get(0).unwrap().activity_expanded);
        transcript.remove("1");
        assert_eq!(transcript.activity_group(0), None);
        transcript.upsert(item(3, "tool", "completed"));
        assert_eq!(transcript.activity_group(0), Some(0..3));
        for id in 5..MAX_TRANSCRIPT_ITEMS + 5 {
            transcript.push(item(id, "tool", "completed"));
        }
        assert_eq!(transcript.activity_group(0), Some(0..MAX_TRANSCRIPT_ITEMS));
        assert_eq!(transcript.list_state().item_count(), MAX_TRANSCRIPT_ITEMS);
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
