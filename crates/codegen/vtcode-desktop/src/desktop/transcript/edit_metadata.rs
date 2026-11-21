use super::*;

impl Transcript {
    /// Record per-call edit counts on an edit row without touching its title.
    /// Carries stamped totals across invocation re-projections (which rebuild
    /// the title from the gist); the projection arm suffixes the fresh title
    /// itself, so this stays a plain field set like the other setters.
    pub fn set_edit_totals(&mut self, id: &str, additions: u64, deletions: u64) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                let totals = EditTotals { additions, deletions };
                if item.edit_totals != Some(totals) {
                    item.edit_totals = Some(totals);
                    self.bump_revision();
                }
            }
        }
    }

    /// Stamp per-call edit counts onto the named tool row, suffixing its
    /// title once. No-ops for unknown ids, non-tool rows, and already-stamped
    /// rows, so duplicate FileChange events and sibling-less calls are safe.
    pub fn stamp_edit_totals(&mut self, id: &str, additions: u64, deletions: u64) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.has_activity_status && item.edit_totals.is_none() {
                    item.edit_totals = Some(EditTotals { additions, deletions });
                    item.title = Self::title_with_edit_stats(item.title.as_str(), additions, deletions).into();
                    self.bump_revision();
                }
            }
        }
    }

    /// Row title with per-call edit counts appended (`Edited main.rs +22 −0`).
    /// Single format site for the stamp path and invocation re-projections;
    /// the minus is U+2212 to match the workspace-review summary. Numerals
    /// only, so no locale key is needed.
    pub(in crate::desktop) fn title_with_edit_stats(base: &str, additions: u64, deletions: u64) -> String {
        format!("{base} +{additions} \u{2212}{deletions}")
    }

    /// Record the raw invoking tool name for search keywords. Plain field set
    /// like the other setters; invocation re-projections repeat the same name.
    pub fn set_tool_name(&mut self, id: &str, tool_name: &str) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.tool_name.as_deref() != Some(tool_name) {
                    item.tool_name = Some(tool_name.into());
                    self.bump_revision();
                }
            }
        }
    }

    pub fn set_command(&mut self, id: &str, command: &str) {
        if let Some(index) = self.position_by_id(id)
            && let Some(item) = self.items.get_mut(index)
            && item.command.as_deref() != Some(command)
        {
            item.command = Some(command.to_owned().into());
            self.bump_revision();
        }
    }

    pub fn set_edit_path(&mut self, id: &str, path: Option<&str>) {
        if let Some(index) = self.position_by_id(id)
            && let Some(item) = self.items.get_mut(index)
            && item.edit_path.as_deref() != path
        {
            item.edit_path = path.map(|value| value.to_owned().into());
            self.bump_revision();
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn set_app_previews(&mut self, id: &str, app_previews: Vec<AppPreviewItem>) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.app_previews != app_previews {
                    item.app_previews = app_previews;
                    self.bump_revision();
                }
            }
        }
    }
}
