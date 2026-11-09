use super::*;

impl Transcript {
    pub(in crate::desktop) fn set_output_identity(&mut self, id: &str, output_id: &str) {
        if let Some(index) = self.position_by_id(id)
            && let Some(item) = self.items.get_mut(index)
            && item.output_item_id.as_deref() != Some(output_id)
        {
            item.output_item_id = Some(output_id.to_owned());
            self.bump_revision();
        }
    }
    /// Attach or replace raw output and remeasure an expanded transcript row.
    pub fn set_output(&mut self, id: &str, output: Option<SharedString>) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.output != output {
                    item.output = output;
                    let expanded = item.expanded;
                    self.bump_revision();
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
            self.bump_revision();
            if expanded {
                self.layout_changes
                    .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
            }
        }
    }
}
