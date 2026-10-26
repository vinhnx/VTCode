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
    /// Status totals kept on the group head so streamed updates do not rescan every tool row.
    pub activity_group_summary: ActivityGroupSummary,
    /// Whether this row carries structured activity status rather than archive text.
    pub has_activity_status: bool,
    pub diff: Option<DiffCard>,
    /// Raw aggregated command/tool output for the Output inspector tab.
    pub output: Option<SharedString>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ActivityGroupSummary {
    pub total: u16,
    pub running: u16,
    pub failed: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ToolActivityStatus {
    running: bool,
    failed: bool,
}

impl ActivityGroupSummary {
    fn for_status(status: ToolActivityStatus) -> Self {
        Self {
            total: 1,
            running: u16::from(status.running),
            failed: u16::from(status.failed),
        }
    }

    fn add_status(&mut self, status: ToolActivityStatus) {
        self.total += 1;
        self.running += u16::from(status.running);
        self.failed += u16::from(status.failed);
    }

    fn remove_status(&mut self, status: ToolActivityStatus) {
        debug_assert!(self.total > 0, "a group summary cannot remove a missing tool row");
        debug_assert!(!status.running || self.running > 0, "running group count must include the removed row");
        debug_assert!(!status.failed || self.failed > 0, "failed group count must include the removed row");
        self.total -= 1;
        self.running -= u16::from(status.running);
        self.failed -= u16::from(status.failed);
    }

    fn replace_status(&mut self, previous: ToolActivityStatus, next: ToolActivityStatus) {
        debug_assert!(!previous.running || self.running > 0, "running group count must include the replaced row");
        debug_assert!(!previous.failed || self.failed > 0, "failed group count must include the replaced row");
        self.running = self.running - u16::from(previous.running) + u16::from(next.running);
        self.failed = self.failed - u16::from(previous.failed) + u16::from(next.failed);
    }
}

impl ToolActivityStatus {
    fn from_text(text: &str) -> Self {
        let status = text.lines().next().unwrap_or_default();
        Self {
            running: matches!(status, "running" | "in progress" | "started"),
            failed: status == "failed" || status.starts_with("failed (exit "),
        }
    }
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
        let activity_group_summary = if role == "tool" {
            ActivityGroupSummary::for_status(ToolActivityStatus::from_text(&text))
        } else {
            ActivityGroupSummary::default()
        };
        Self {
            id,
            has_activity_status: role == "tool",
            role,
            title,
            text,
            markdown: SharedString::default(),
            accessibility_text: SharedString::default(),
            expanded: false,
            activity_expanded: false,
            activity_grouped: false,
            activity_group_head: false,
            activity_group_summary,
            diff: None,
            output: None,
        }
    }

    pub(super) fn prepare_render_content(&mut self) {
        if matches!(self.role.as_str(), "tool" | "diff") {
            self.markdown = SharedString::default();
            self.accessibility_text = SharedString::default();
            return;
        }

        let prepared = prepare_markdown(&self.text);
        self.markdown = prepared.markdown;
        self.accessibility_text = prepared.accessible_text;
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

    fn position_by_id(&self, id: &str) -> Option<usize> {
        let last_index = self.items.len().checked_sub(1)?;
        if self.items.back().is_some_and(|item| item.id == id) {
            return Some(last_index);
        }
        self.items.iter().rposition(|item| item.id == id)
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

    pub fn push(&mut self, item: TranscriptItem) {
        self.push_inner(item, true);
    }

    /// Replace the transcript with items whose render projections were prepared off the UI thread.
    pub(super) fn replace_prepared(&mut self, items: Vec<TranscriptItem>) {
        self.clear();
        for item in items {
            self.push_inner(item, false);
        }
    }

    fn push_inner(&mut self, mut item: TranscriptItem, prepare_render_content: bool) {
        if prepare_render_content {
            item.prepare_render_content();
        }
        let item_is_tool = item.role == "tool";
        let item_status = ToolActivityStatus::from_text(&item.text);
        if item_is_tool {
            item.activity_group_summary = ActivityGroupSummary::for_status(item_status);
        } else {
            item.activity_group_summary = ActivityGroupSummary::default();
        }
        let extends_tool_group = item_is_tool && self.items.back().is_some_and(|row| row.role == "tool");
        let previous_group_start = self.trailing_tool_group_start;
        let evict_oldest = self.items.len() == MAX_TRANSCRIPT_ITEMS;
        let mut promoted_group_head = false;
        if evict_oldest {
            if let Some(removed) = self.items.pop_front()
                && removed.activity_group_head
            {
                let mut summary = removed.activity_group_summary;
                summary.remove_status(ToolActivityStatus::from_text(&removed.text));
                if let Some(head) = self.items.front_mut().filter(|item| item.role == "tool") {
                    let grouped = summary.total > 1;
                    head.activity_group_summary = summary;
                    head.activity_grouped = grouped;
                    head.activity_group_head = grouped;
                    head.activity_expanded = grouped && removed.activity_expanded;
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
            head.activity_group_summary.add_status(item_status);
            head.activity_grouped = true;
            head.activity_group_head = true;
            item.activity_grouped = true;
            item.activity_expanded = head.activity_expanded;
            item.activity_group_summary = ActivityGroupSummary::default();
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
        if let Some(index) = self.position_by_id(id) {
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
        if let Some(index) = self.position_by_id(&item.id) {
            let Some(existing) = self.items.get(index) else {
                return;
            };
            let role_changed = existing.role != item.role;
            let updated_is_tool = item.role == "tool";
            let title_changed = existing.title != item.title;
            let text_changed = existing.text != item.text;
            let is_tool_update = updated_is_tool && existing.role == "tool";
            let activity_status_changed = is_tool_update && existing.text.lines().next() != item.text.lines().next();
            let previous_status = if is_tool_update {
                ToolActivityStatus::from_text(&existing.text)
            } else {
                ToolActivityStatus::default()
            };
            let next_status = if is_tool_update {
                ToolActivityStatus::from_text(&item.text)
            } else {
                ToolActivityStatus::default()
            };
            let group_status_changed = is_tool_update && previous_status != next_status;
            let has_activity_status_changed = existing.has_activity_status != item.has_activity_status;
            let diff_changed = existing.diff != item.diff;
            let content_changed =
                role_changed || title_changed || text_changed || has_activity_status_changed || diff_changed;
            if !content_changed {
                let Some(existing) = self.items.get_mut(index) else {
                    return;
                };
                if item.output.is_some() {
                    existing.output = item.output;
                }
                return;
            }

            let group_head = group_status_changed.then(|| self.activity_group_start_for(index)).flatten();
            if text_changed || role_changed {
                item.prepare_render_content();
            } else if !role_changed {
                item.markdown.clone_from(&existing.markdown);
                item.accessibility_text.clone_from(&existing.accessibility_text);
            }
            item.expanded = existing.expanded;
            item.activity_expanded = existing.activity_expanded;
            item.activity_grouped = existing.activity_grouped;
            item.activity_group_head = existing.activity_group_head;
            item.activity_group_summary = existing.activity_group_summary;
            if item.output.is_none() {
                item.output.clone_from(&existing.output);
            }
            let remeasure_row = role_changed
                || title_changed
                || has_activity_status_changed
                || diff_changed
                || if updated_is_tool {
                    activity_status_changed || (existing.expanded && text_changed)
                } else {
                    text_changed
                };
            if let Some(existing) = self.items.get_mut(index) {
                *existing = item;
            }
            if group_status_changed
                && let Some(head_index) = group_head
                && let Some(head) = self.items.get_mut(head_index)
            {
                head.activity_group_summary.replace_status(previous_status, next_status);
            }
            if role_changed {
                self.rebuild_activity_group_flags();
                self.refresh_trailing_tool_group_start();
                self.layout_changes.push(TranscriptLayoutChange::Remeasure);
            } else {
                if remeasure_row {
                    self.layout_changes
                        .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
                }
                if group_status_changed
                    && let Some(head_index) = group_head
                    && head_index != index
                    && self.items.get(head_index).is_some_and(|head| head.activity_grouped)
                {
                    self.layout_changes
                        .push(TranscriptLayoutChange::RemeasureItems(head_index..head_index + 1));
                }
            }
        } else {
            self.push(item);
        }
    }

    /// Attach or replace raw command output, which is presented outside the transcript row.
    pub fn set_output(&mut self, id: &str, output: Option<SharedString>) {
        if let Some(index) = self.position_by_id(id) {
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
        let start = self.activity_group_start_for(index)?;
        let total = self.items.get(start)?.activity_group_summary.total;
        Some(start..start.saturating_add(usize::from(total)).min(self.items.len()))
    }

    fn activity_group_start_for(&self, index: usize) -> Option<usize> {
        if !self.items.get(index).is_some_and(|item| item.role == "tool") {
            return None;
        }
        if let Some(start) = self.trailing_tool_group_start
            && index >= start
        {
            return Some(start);
        }
        let mut start = index;
        while start > 0 && self.items.get(start - 1).is_some_and(|item| item.role == "tool") {
            start -= 1;
        }
        Some(start)
    }

    fn rebuild_activity_group_flags(&mut self) {
        let mut start = 0;
        while start < self.items.len() {
            if !self.items.get(start).is_some_and(|item| item.role == "tool") {
                if let Some(item) = self.items.get_mut(start) {
                    item.activity_grouped = false;
                    item.activity_group_head = false;
                    item.activity_expanded = false;
                    item.activity_group_summary = ActivityGroupSummary::default();
                }
                start += 1;
                continue;
            }

            let mut end = start + 1;
            while self.items.get(end).is_some_and(|item| item.role == "tool") {
                end += 1;
            }
            let expanded = self.items.get(start).is_some_and(|item| item.activity_expanded);
            let mut summary = ActivityGroupSummary::default();
            for item in self.items.range(start..end) {
                summary.add_status(ToolActivityStatus::from_text(&item.text));
            }
            for index in start..end {
                if let Some(item) = self.items.get_mut(index) {
                    item.activity_grouped = end - start > 1;
                    item.activity_group_head = end - start > 1 && index == start;
                    item.activity_expanded = end - start > 1 && expanded;
                    item.activity_group_summary = if index == start {
                        summary
                    } else {
                        ActivityGroupSummary::default()
                    };
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
        if let Some(index) = self.position_by_id(id)
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
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| self.items.get(*index).is_some_and(|item| item.id == id && item.role == "tool"))
            .or_else(|| self.items.iter().rposition(|item| item.id == id && item.role == "tool"));
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn toggle_diff(&mut self, id: &str) {
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| self.items.get(*index).is_some_and(|item| item.id == id && item.role == "diff"))
            .or_else(|| self.items.iter().rposition(|item| item.id == id && item.role == "diff"));
        if let Some(index) = index {
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
    fn tool_and_diff_rows_skip_unused_markdown_projections() {
        let mut transcript = Transcript::default();
        for role in ["tool", "diff"] {
            let row = item(1, role, "status\n\nmetadata");
            assert!(row.markdown.is_empty());
            assert!(row.accessibility_text.is_empty());
            transcript.push(row);
            let prepared = transcript.get(transcript.len() - 1).unwrap();
            assert!(prepared.markdown.is_empty());
            assert!(prepared.accessibility_text.is_empty());
        }

        transcript.push(item(2, "assistant", "A **readable** response."));
        let message = transcript.get(2).unwrap();
        assert_eq!(message.markdown.as_str(), "A **readable** response.");
        assert!(message.accessibility_text.contains("readable"));
    }

    #[test]
    fn unchanged_message_snapshots_skip_projection_and_layout_work() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "assistant", "A **stable** response."));
        let _initial_layout = transcript.take_layout_changes();
        let original_markdown = transcript.get(0).unwrap().markdown.clone();
        let original_accessibility = transcript.get(0).unwrap().accessibility_text.clone();

        let mut duplicate = item(1, "assistant", "A **stable** response.");
        duplicate.markdown = "unexpected projection".into();
        duplicate.accessibility_text = "unexpected accessibility text".into();
        transcript.upsert(duplicate);

        assert!(transcript.take_layout_changes().is_empty());
        assert_eq!(transcript.get(0).unwrap().markdown, original_markdown);
        assert_eq!(transcript.get(0).unwrap().accessibility_text, original_accessibility);

        transcript.upsert(item(1, "assistant", "A **changed** response."));
        assert_eq!(transcript.get(0).unwrap().markdown.as_str(), "A **changed** response.");
    }

    #[test]
    fn activity_group_summary_tracks_appends_status_changes_and_removal() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "running"));
        transcript.push(item(2, "tool", "running"));
        let _initial_layout = transcript.take_layout_changes();

        assert_eq!(
            transcript.get(0).unwrap().activity_group_summary,
            super::ActivityGroupSummary { total: 2, running: 2, failed: 0 }
        );

        transcript.upsert(item(2, "tool", "failed"));
        assert_eq!(
            transcript.get(0).unwrap().activity_group_summary,
            super::ActivityGroupSummary { total: 2, running: 1, failed: 1 }
        );

        transcript.push(item(3, "tool", "completed"));
        assert_eq!(transcript.get(0).unwrap().activity_group_summary.total, 3);
        transcript.remove("2");
        assert_eq!(
            transcript.get(0).unwrap().activity_group_summary,
            super::ActivityGroupSummary { total: 2, running: 1, failed: 0 }
        );
    }

    #[test]
    fn collapsed_tool_output_updates_skip_geometry_until_expanded() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "running\n\nfirst"));
        transcript.push(item(2, "tool", "running\n\nfirst"));
        let _initial_layout = transcript.take_layout_changes();

        transcript.upsert(item(2, "tool", "running\n\nstreamed output grew"));
        assert!(transcript.take_layout_changes().is_empty());
        assert_eq!(transcript.get(1).unwrap().text.as_str(), "running\n\nstreamed output grew");

        transcript.toggle_tool("2");
        let _expanded_layout = transcript.take_layout_changes();
        transcript.upsert(item(2, "tool", "running\n\nexpanded output grew again"));
        let changes = transcript.take_layout_changes();
        assert!(changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (1..2))
        }));
        assert!(!changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (0..1))
        }));
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
        assert_eq!(usize::from(promoted_head.activity_group_summary.total), MAX_TRANSCRIPT_ITEMS);
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
        assert_eq!(transcript.get(1).unwrap().activity_group_summary.total, 100);
        assert_eq!(transcript.get(1).unwrap().activity_group_summary.running, 0);
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
    fn row_lookup_keeps_newest_duplicate_id_semantics() {
        let mut transcript = Transcript::default();
        transcript.push(item(1, "tool", "older"));
        transcript.push(item(2, "assistant", "middle"));
        transcript.push(item(1, "assistant", "newest"));

        assert_eq!(transcript.position_by_id("1"), Some(2));
        assert_eq!(transcript.position_by_id("2"), Some(1));
        assert_eq!(transcript.position_by_id("missing"), None);
        transcript.toggle_tool("1");
        assert!(transcript.get(0).unwrap().expanded, "role-specific lookup still finds the latest matching tool row");
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
