use std::collections::VecDeque;
use std::ops::Range;

use gpui_kit::SharedString;

use super::markdown::prepare_markdown;

pub(super) const MAX_TRANSCRIPT_ITEMS: usize = 2_000;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TranscriptLayoutChange {
    Reset(usize),
    Splice { range: Range<usize>, count: usize },
    Remeasure,
    RemeasureItems(Range<usize>),
    PauseFollowingTail,
    FollowLatest,
}

#[derive(Clone, Debug)]
pub(super) struct TranscriptItem {
    pub id: String,
    pub role: String,
    pub title: SharedString,
    pub text: SharedString,
    pub markdown: SharedString,
    pub accessibility_text: SharedString,
    pub expanded: bool,
    pub activity_expanded: bool,
    /// Whether this row carries structured activity status rather than archive text.
    pub has_activity_status: bool,
    pub diff: Option<DiffCard>,
    /// Raw aggregated command/tool output for the Output inspector tab.
    pub output: Option<SharedString>,
}

/// Structured payload for a file-change activity row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DiffCard {
    pub files: Vec<DiffFile>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub incomplete: bool,
    pub failed: bool,
    pub patch: String,
}

/// One file inside a [`DiffCard`], with its change kind.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub fn new(id: String, role: String, title: SharedString, text: String) -> Self {
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

/// Owns transcript rows and queues the minimal layout changes for the view.
#[derive(Default)]
pub(super) struct Transcript {
    items: VecDeque<TranscriptItem>,
    layout_changes: Vec<TranscriptLayoutChange>,
    layout_initialized: bool,
}

impl Transcript {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

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

    pub fn take_layout_changes(&mut self) -> Vec<TranscriptLayoutChange> {
        if !self.layout_initialized {
            self.layout_initialized = true;
            self.layout_changes.clear();
            return vec![TranscriptLayoutChange::Reset(self.items.len())];
        }
        if self
            .layout_changes
            .iter()
            .any(|change| matches!(change, TranscriptLayoutChange::Reset(_)))
        {
            self.layout_changes.clear();
            return vec![TranscriptLayoutChange::Reset(self.items.len())];
        }
        std::mem::take(&mut self.layout_changes)
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.layout_changes.clear();
        self.layout_changes.push(TranscriptLayoutChange::Reset(0));
    }

    pub fn push(&mut self, item: TranscriptItem) {
        if self.items.len() == MAX_TRANSCRIPT_ITEMS {
            let _removed = self.items.pop_front();
            self.layout_changes
                .push(TranscriptLayoutChange::Splice { range: 0..1, count: 0 });
        }
        let index = self.items.len();
        self.items.push_back(item);
        self.layout_changes
            .push(TranscriptLayoutChange::Splice { range: index..index, count: 1 });
        if let Some(group) = self.activity_group(index) {
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(group));
        }
    }

    /// Remove a local unsent echo together with its measured row.
    pub fn remove(&mut self, id: &str) {
        if let Some(index) = self.items.iter().rposition(|item| item.id == id) {
            let _removed = self.items.remove(index);
            self.layout_changes
                .push(TranscriptLayoutChange::Splice { range: index..index + 1, count: 0 });
            let affected = index.saturating_sub(1)..(index + 1).min(self.items.len());
            if !affected.is_empty() {
                self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(affected));
            }
        }
    }

    pub fn upsert(&mut self, mut item: TranscriptItem) {
        if let Some(index) = self.items.iter().rposition(|existing| existing.id == item.id) {
            let Some(existing) = self.items.get_mut(index) else {
                return;
            };
            let role_changed = existing.role != item.role;
            let layout_changed = role_changed
                || existing.role != "tool"
                || existing.title != item.title
                || existing.text != item.text
                || existing.has_activity_status != item.has_activity_status
                || existing.diff != item.diff;
            if !layout_changed {
                if item.output.is_some() {
                    existing.output = item.output;
                }
                return;
            }

            item.expanded = existing.expanded;
            item.activity_expanded = existing.activity_expanded;
            if item.output.is_none() {
                item.output.clone_from(&existing.output);
            }
            *existing = item;
            if role_changed {
                self.layout_changes.push(TranscriptLayoutChange::Remeasure);
            } else {
                self.layout_changes
                    .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
            }
        } else {
            self.push(item);
        }
    }

    /// Attach or replace raw command output, which is presented outside the transcript row.
    pub fn set_output(&mut self, id: &str, output: Option<SharedString>) {
        if let Some(index) = self.items.iter().rposition(|item| item.id == id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.output != output {
                    item.output = output;
                }
            }
        }
    }

    /// Consecutive tool rows form a presentation group without changing event identities or ordering.
    pub fn activity_group(&self, index: usize) -> Option<Range<usize>> {
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
        if let Some(index) = self.items.iter().rposition(|item| item.id == id)
            && let Some(range) = self.activity_group(index)
        {
            if let Some(head) = self.items.get_mut(range.start) {
                head.activity_expanded = !head.activity_expanded;
            }
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(range));
        }
    }

    pub fn toggle_tool(&mut self, id: &str) {
        if let Some(index) = self.items.iter().rposition(|item| item.id == id && item.role == "tool") {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn toggle_diff(&mut self, id: &str) {
        if let Some(index) = self.items.iter().rposition(|item| item.id == id && item.role == "diff") {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn follow_latest(&mut self) {
        self.layout_changes.push(TranscriptLayoutChange::FollowLatest);
    }

    pub fn remeasure(&mut self) {
        self.layout_changes.push(TranscriptLayoutChange::Remeasure);
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_TRANSCRIPT_ITEMS, Transcript, TranscriptItem};

    fn item(id: usize, role: &str, text: &str) -> TranscriptItem {
        TranscriptItem::new(id.to_string(), role.to_string(), "Activity".into(), text.to_string())
    }

    #[test]
    fn activity_group_boundaries_follow_removal_role_changes_and_eviction() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "completed"));
        transcript.push(item(2, "tool", "running"));
        transcript.push(item(3, "assistant", "commentary"));
        transcript.push(item(4, "tool", "completed"));
        assert_eq!(transcript.take_layout_changes(), vec![super::TranscriptLayoutChange::Reset(4)]);
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
        assert_eq!(transcript.len(), MAX_TRANSCRIPT_ITEMS);
        assert_eq!(transcript.get(MAX_TRANSCRIPT_ITEMS - 1).unwrap().text, "completed");
    }

    #[test]
    fn updates_keep_tool_expansion_and_queue_layout_changes() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "before"));
        transcript.push(item(2, "assistant", "reply"));
        let _initial_layout = transcript.take_layout_changes();
        transcript.toggle_tool("1");
        transcript.upsert(item(1, "tool", "after\nwith details"));

        assert_eq!(transcript.len(), 2);
        assert!(transcript.get(0).unwrap().expanded);
        assert_eq!(transcript.get(0).unwrap().text, "after\nwith details");
        assert!(
            transcript
                .take_layout_changes()
                .iter()
                .any(|change| { matches!(change, super::TranscriptLayoutChange::PauseFollowingTail) })
        );
        transcript.clear();
        assert!(transcript.is_empty());
        assert_eq!(transcript.len(), 0);
        assert_eq!(transcript.take_layout_changes(), vec![super::TranscriptLayoutChange::Reset(0)]);
    }

    #[test]
    fn output_only_updates_preserve_transcript_geometry_and_existing_capture() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "running\n\nmetadata"));
        let _initial_layout = transcript.take_layout_changes();

        transcript.set_output("1", Some("first output".into()));
        assert!(transcript.take_layout_changes().is_empty());

        transcript.upsert(item(1, "tool", "running\n\nmetadata"));
        assert!(transcript.take_layout_changes().is_empty());
        let output = transcript.get(0).and_then(|row| row.output.as_ref()).map(ToString::to_string);
        assert_eq!(output.as_deref(), Some("first output"));

        transcript.set_output("1", Some("updated output".into()));
        assert!(transcript.take_layout_changes().is_empty());
        let output = transcript.get(0).and_then(|row| row.output.as_ref()).map(ToString::to_string);
        assert_eq!(output.as_deref(), Some("updated output"));
    }
}
