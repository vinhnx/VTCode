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
    /// Shared expansion state copied to each row so virtualized rendering stays constant-time.
    pub activity_expanded: bool,
    /// Whether this tool row is part of a consecutive group of at least two rows.
    pub activity_grouped: bool,
    /// Whether this row owns the visible header for its tool group.
    pub activity_group_head: bool,
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
            activity_grouped: false,
            activity_group_head: false,
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
    /// Head index of the trailing consecutive tool group, including single-row groups.
    trailing_tool_group_start: Option<usize>,
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
        self.trailing_tool_group_start = None;
        self.layout_changes.push(TranscriptLayoutChange::Reset(0));
    }

    pub fn push(&mut self, mut item: TranscriptItem) {
        let item_is_tool = item.role == "tool";
        let extends_tool_group = item_is_tool && self.items.back().is_some_and(|row| row.role == "tool");
        let previous_group_start = self.trailing_tool_group_start;
        let evict_oldest = self.items.len() == MAX_TRANSCRIPT_ITEMS;
        let mut promoted_group_head = false;
        if evict_oldest {
            let removed_group_expanded = self
                .items
                .pop_front()
                .and_then(|item| item.activity_group_head.then_some(item.activity_expanded));
            if let Some(expanded) = removed_group_expanded {
                let next_tool = self.items.get(1).is_some_and(|item| item.role == "tool");
                if let Some(head) = self.items.front_mut().filter(|item| item.role == "tool") {
                    head.activity_grouped = next_tool;
                    head.activity_group_head = next_tool;
                    head.activity_expanded = next_tool && expanded;
                    promoted_group_head = true;
                }
            }
            self.layout_changes
                .push(TranscriptLayoutChange::Splice { range: 0..1, count: 0 });
            if let Some(start) = self.trailing_tool_group_start {
                self.trailing_tool_group_start = Some(start.saturating_sub(1));
            }
        }
        let index = self.items.len();
        debug_assert!(
            !extends_tool_group || previous_group_start.is_some(),
            "the trailing tool-group head stays in sync with transcript rows"
        );
        let group_head = extends_tool_group.then(|| {
            previous_group_start
                .unwrap_or_else(|| index.saturating_sub(1))
                .saturating_sub(usize::from(evict_oldest))
        });
        if let Some(head_index) = group_head
            && let Some(head) = self.items.get_mut(head_index)
        {
            head.activity_grouped = true;
            head.activity_group_head = true;
            item.activity_grouped = true;
            item.activity_expanded = head.activity_expanded;
        }
        self.trailing_tool_group_start = if item_is_tool {
            Some(group_head.unwrap_or(index))
        } else {
            None
        };
        self.items.push_back(item);
        self.layout_changes
            .push(TranscriptLayoutChange::Splice { range: index..index, count: 1 });
        if promoted_group_head {
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(0..1));
        }
        if let Some(head) = group_head {
            // A new tool row changes the shared group header and inserts only
            // one new row. Existing tool rows keep their measured geometry.
            if !promoted_group_head || head != 0 {
                self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(head..head + 1));
            }
        }
    }

    /// Remove a local unsent echo together with its measured row.
    pub fn remove(&mut self, id: &str) {
        if let Some(index) = self.items.iter().rposition(|item| item.id == id) {
            let removed_was_tool = self.items.remove(index).is_some_and(|item| item.role == "tool");
            self.rebuild_activity_group_flags();
            self.refresh_trailing_tool_group_start();
            self.layout_changes
                .push(TranscriptLayoutChange::Splice { range: index..index + 1, count: 0 });
            let affected = index.saturating_sub(1)..(index + 1).min(self.items.len());
            if !affected.is_empty() {
                self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(affected));
            }
            if removed_was_tool
                && let Some(group) = [index.saturating_sub(1), index]
                    .into_iter()
                    .find_map(|probe| self.activity_group(probe))
            {
                self.layout_changes
                    .push(TranscriptLayoutChange::RemeasureItems(group.start..group.start + 1));
            }
        }
    }

    pub fn upsert(&mut self, mut item: TranscriptItem) {
        if let Some(index) = self.items.iter().rposition(|existing| existing.id == item.id) {
            let Some(existing) = self.items.get_mut(index) else {
                return;
            };
            let role_changed = existing.role != item.role;
            let updated_is_tool = item.role == "tool";
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
            item.activity_grouped = existing.activity_grouped;
            item.activity_group_head = existing.activity_group_head;
            if item.output.is_none() {
                item.output.clone_from(&existing.output);
            }
            *existing = item;
            if role_changed {
                self.rebuild_activity_group_flags();
                self.refresh_trailing_tool_group_start();
                self.layout_changes.push(TranscriptLayoutChange::Remeasure);
            } else {
                self.layout_changes
                    .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
                if updated_is_tool
                    && let Some(group) = self.activity_group(index)
                    && index != group.start
                {
                    self.layout_changes
                        .push(TranscriptLayoutChange::RemeasureItems(group.start..group.start + 1));
                }
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
        if !self.items.get(index).is_some_and(|item| item.activity_grouped) {
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
        Some(start..end)
    }

    fn rebuild_activity_group_flags(&mut self) {
        let mut start = 0;
        while start < self.items.len() {
            if !self.items.get(start).is_some_and(|item| item.role == "tool") {
                if let Some(item) = self.items.get_mut(start) {
                    item.activity_grouped = false;
                    item.activity_group_head = false;
                    item.activity_expanded = false;
                }
                start += 1;
                continue;
            }

            let mut end = start + 1;
            while self.items.get(end).is_some_and(|item| item.role == "tool") {
                end += 1;
            }
            let expanded = self.items.get(start).is_some_and(|item| item.activity_expanded);
            for index in start..end {
                if let Some(item) = self.items.get_mut(index) {
                    item.activity_grouped = end - start > 1;
                    item.activity_group_head = end - start > 1 && index == start;
                    item.activity_expanded = end - start > 1 && expanded;
                }
            }
            start = end;
        }
    }

    fn refresh_trailing_tool_group_start(&mut self) {
        if !self.items.back().is_some_and(|item| item.role == "tool") {
            self.trailing_tool_group_start = None;
            return;
        }
        let mut start = self.items.len().saturating_sub(1);
        while start > 0 && self.items.get(start - 1).is_some_and(|item| item.role == "tool") {
            start -= 1;
        }
        self.trailing_tool_group_start = Some(start);
    }

    pub fn toggle_activity_group(&mut self, id: &str) {
        if let Some(index) = self.items.iter().rposition(|item| item.id == id)
            && let Some(range) = self.activity_group(index)
        {
            let expanded = !self.items.get(range.start).is_some_and(|head| head.activity_expanded);
            for index in range.clone() {
                if let Some(item) = self.items.get_mut(index) {
                    item.activity_expanded = expanded;
                }
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

        transcript.toggle_activity_group("5");
        transcript.push(item(MAX_TRANSCRIPT_ITEMS + 5, "tool", "completed"));
        let promoted_head = transcript.get(0).unwrap();
        assert_eq!(promoted_head.id, "6");
        assert!(promoted_head.activity_group_head);
        assert!(promoted_head.activity_expanded, "the promoted group head retains expansion state");
        assert_eq!(transcript.activity_group(0), Some(0..MAX_TRANSCRIPT_ITEMS));
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
    fn appending_tool_rows_remeasures_only_the_shared_group_header() {
        let mut transcript = Transcript::default();
        transcript.push(item(0, "user", "prompt"));
        let _initial_layout = transcript.take_layout_changes();

        for id in 1..=100 {
            transcript.push(item(id, "tool", "completed"));
        }

        let changes = transcript.take_layout_changes();
        let remeasured_ranges = changes
            .iter()
            .filter_map(|change| match change {
                super::TranscriptLayoutChange::RemeasureItems(range) => Some(range.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(remeasured_ranges.len(), 99, "each added row updates the existing group header");
        assert!(remeasured_ranges.iter().all(|range| *range == (1..2)));
    }

    #[test]
    fn tool_status_updates_remeasure_the_group_header_and_changed_row() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "running"));
        transcript.push(item(2, "tool", "running"));
        let _initial_layout = transcript.take_layout_changes();

        transcript.upsert(item(2, "tool", "failed"));

        let changes = transcript.take_layout_changes();
        assert!(transcript.get(1).unwrap().activity_grouped, "status updates preserve group membership");
        assert!(changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (1..2))
        }));
        assert!(changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (0..1))
        }));
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
