//! Transcript group/toggle/follow mutations extracted from the root module.

use super::*;

impl Transcript {
    /// Consecutive tool rows form a presentation group without changing event identities or ordering.
    pub fn activity_group(&self, index: usize) -> Option<Range<usize>> {
        if !self.items.get(index).is_some_and(|item| item.activity_grouped) {
            return None;
        }
        let start = self.activity_group_start_for(index)?;
        let total = self.items.get(start)?.activity_group_summary.total;
        Some(start..start.saturating_add(usize::from(total)).min(self.items.len()))
    }

    pub(super) fn activity_group_start_for(&self, index: usize) -> Option<usize> {
        if !self.items.get(index).is_some_and(|item| item.is_tool()) {
            return None;
        }
        if let Some(start) = self.trailing_tool_group_start
            && index >= start
        {
            return Some(start);
        }
        let mut start = index;
        while start > 0 && self.items.get(start - 1).is_some_and(|item| item.is_tool()) {
            start -= 1;
        }
        Some(start)
    }

    pub(super) fn rebuild_activity_group_flags(&mut self) {
        let mut start = 0;
        while start < self.items.len() {
            if !self.items.get(start).is_some_and(|item| item.is_tool()) {
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
            while self.items.get(end).is_some_and(|item| item.is_tool()) {
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

    pub(super) fn refresh_trailing_tool_group_start(&mut self) {
        if !self.items.back().is_some_and(|item| item.is_tool()) {
            self.trailing_tool_group_start = None;
            return;
        }
        let mut start = self.items.len().saturating_sub(1);
        while start > 0 && self.items.get(start - 1).is_some_and(|item| item.is_tool()) {
            start -= 1;
        }
        self.trailing_tool_group_start = Some(start);
    }

    pub fn toggle_tool(&mut self, id: &str) {
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| self.items.get(*index).is_some_and(|item| item.id == id && item.is_tool()))
            .or_else(|| self.items.iter().rposition(|item| item.id == id && item.is_tool()));
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded.toggle();
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    /// Expand one tool row's inline output tail into a wrapped, vertically
    /// scrollable code view so long single-line output can be read in place.
    pub fn toggle_tool_output(&mut self, id: &str) {
        if let Some(index) = self.items.iter().rposition(|item| item.id == id && item.is_tool()) {
            if let Some(item) = self.items.get_mut(index) {
                item.output_expanded.toggle();
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn toggle_diff(&mut self, id: &str) {
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| self.items.get(*index).is_some_and(|item| item.id == id && item.is_diff()))
            .or_else(|| self.items.iter().rposition(|item| item.id == id && item.is_diff()));
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded.toggle();
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn toggle_notice(&mut self, id: &str) {
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| {
                self.items
                    .get(*index)
                    .is_some_and(|item| item.id == id && item.is_notice() && item.collapsible_details)
            })
            .or_else(|| {
                self.items
                    .iter()
                    .rposition(|item| item.id == id && item.is_notice() && item.collapsible_details)
            });
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded.toggle();
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    /// Expand or collapse the folded notices of a stacked callout. Folding is
    /// identity-free (members are immutable), so the head owns the state.
    pub fn toggle_notice_stack(&mut self, id: &str) {
        let index = self
            .items
            .iter()
            .rposition(|item| item.id == id && item.is_notice() && !item.notice_stack.is_empty());
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.notice_stack_expanded.toggle();
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn follow_latest(&mut self) {
        self.layout_changes.push(TranscriptLayoutChange::FollowLatest);
    }

    pub fn remeasure(&mut self) {
        // Consecutive full remeasures are redundant by construction: nothing
        // changed between them, so measuring twice yields the same layout.
        // Theme browsing, contrast drags, and font stepping enqueue bursts
        // while the transcript is unmounted (routes), and the consumer
        // applies every entry; collapsing the burst here keeps one probe per
        // drain instead of one per click.
        if !matches!(self.layout_changes.last(), Some(TranscriptLayoutChange::Remeasure)) {
            self.layout_changes.push(TranscriptLayoutChange::Remeasure);
        }
    }
}
