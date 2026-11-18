use super::*;

impl Transcript {
    pub(in crate::desktop) fn set_output_identity(&mut self, id: &str, output_id: Option<&str>) {
        if let Some(index) = self.position_by_id(id)
            && let Some(item) = self.items.get_mut(index)
            && item.output_item_id.as_deref() != output_id
        {
            item.output_item_id = output_id.map(str::to_owned);
            self.bump_output_revision();
        }
    }
    /// Attach or replace human-projected output and remeasure an expanded transcript row.
    pub fn set_output(&mut self, id: &str, output: Option<SharedString>) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.output != output {
                    // Activity rows read `has_output`, so only the none↔some
                    // boundary matters to them. Streaming text deltas keep
                    // the Activity projection cache warm.
                    let activity_affecting = item.output.is_none() != output.is_none();
                    item.output = output;
                    let expanded = item.expanded;
                    if activity_affecting {
                        self.bump_revision();
                    } else {
                        self.bump_output_revision();
                    }
                    if expanded {
                        self.layout_changes
                            .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
                    }
                }
            }
        }
    }

    pub fn set_output_truncated(&mut self, id: &str, truncated: bool) {
        if let Some(index) = self.position_by_id(id)
            && let Some(item) = self.items.get_mut(index)
            && item.output_truncated != truncated
        {
            item.output_truncated = truncated;
            let expanded = item.expanded;
            self.bump_output_revision();
            if expanded {
                self.layout_changes
                    .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
            }
        }
    }
}
