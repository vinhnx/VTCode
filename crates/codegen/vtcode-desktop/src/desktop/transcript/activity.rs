use super::*;

impl Transcript {
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
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(range));
        }
    }

    /// Expand a tool row and its containing activity group so a search result is visible.
    ///
    /// Queues remeasurement but deliberately no tail pause: the sole caller
    /// navigates to the row immediately, and `scroll_to_item` disengages
    /// tail-following itself. A pause here would write its stale anchor over
    /// the jump in the same sync pass.
    pub(in crate::desktop) fn reveal_tool_item(&mut self, id: &str) -> Option<usize> {
        let index = self.position_by_id(id)?;
        if !self.items.get(index).is_some_and(|item| item.is_tool()) {
            return None;
        }

        let group = self.activity_group(index);
        let expand_group = group
            .as_ref()
            .is_some_and(|range| !self.items.get(range.start).is_some_and(|item| item.activity_expanded));
        let expand_item = self.items.get(index).is_some_and(|item| !item.expanded);
        if !expand_group && !expand_item {
            return Some(index);
        }

        self.bump_revision();
        if expand_group && let Some(range) = group {
            for group_index in range.clone() {
                if let Some(item) = self.items.get_mut(group_index) {
                    item.activity_expanded = true;
                }
            }
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(range));
        }
        if expand_item {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = true;
            }
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
        Some(index)
    }
}
