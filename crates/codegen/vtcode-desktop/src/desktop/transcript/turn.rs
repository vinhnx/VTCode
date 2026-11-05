use super::*;

impl Transcript {
    /// Groups restored archive rows into synthetic turns: each `final_answer`
    /// row closes the turn opened by the nearest preceding user row, and the
    /// rows between them are stamped with an `archive-{id}` turn identity.
    /// Bounds hold by construction (a turn start always trails the current
    /// index); the accessors stay fail-safe regardless.
    pub(in crate::desktop) fn group_archive_turns(
        mut rows: Vec<TranscriptItem>,
        phases: Vec<Option<String>>,
    ) -> Vec<TranscriptItem> {
        let mut start = None;
        let mut ambiguous = false;
        for index in 0..rows.len() {
            let is_user = rows.get(index).is_some_and(TranscriptItem::is_user);
            let is_assistant = rows.get(index).is_some_and(TranscriptItem::is_assistant);
            if is_user {
                start = Some(index);
                ambiguous = false;
            } else if is_assistant {
                match phases.get(index).and_then(|phase| phase.as_deref()) {
                    Some("commentary") => {}
                    Some("final_answer") if start.is_some() && !ambiguous => {
                        if let Some(start_index) = start.take() {
                            let Some(turn_row) = rows.get(start_index) else {
                                continue;
                            };
                            let turn_id = format!("archive-{}", turn_row.id);
                            if let Some(span) = rows.get_mut(start_index + 1..=index) {
                                for row in span {
                                    if row.is_assistant() || row.is_tool() {
                                        row.turn_id = Some(turn_id.clone().into());
                                    }
                                }
                            }
                            if let Some(final_row) = rows.get_mut(index) {
                                final_row.archived_final = true;
                            }
                        }
                    }
                    _ => ambiguous = true,
                }
            }
        }
        rows
    }

    pub(in crate::desktop) fn restore_archived_turn_files(
        &mut self,
        row_id: &str,
        canonical_item_id: &str,
        canonical_turn_id: &str,
        incomplete: bool,
    ) {
        let Some(index) = self.position_by_id(row_id) else {
            return;
        };
        if !self.items.get(index).is_some_and(|item| {
            item.is_assistant()
                && item.archived_phase.as_deref() == Some("final_answer")
                && item.canonical_item_id.as_deref() == Some(canonical_item_id)
        }) {
            return;
        }
        let (files, totals, partial) = summarize_turn_files(
            self.items
                .iter()
                .filter(|item| item.canonical_turn_id.as_deref() == Some(canonical_turn_id)),
            incomplete,
        );
        if let Some(item) = self.items.get_mut(index) {
            item.turn_files = files;
            item.turn_file_totals = totals;
            item.turn_files_incomplete = partial;
        }
        self.bump_revision();
        self.layout_changes
            .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
    }

    pub(in crate::desktop) fn set_archived_completion_time(
        &mut self,
        id: &str,
        canonical_id: &str,
        time: SharedString,
    ) {
        let Some(index) = self.position_by_id(id) else { return };
        let Some(item) = self.items.get_mut(index).filter(|item| {
            item.is_assistant()
                && item.archived_phase.as_deref() == Some("final_answer")
                && item.canonical_item_id.as_deref() == Some(canonical_id)
        }) else {
            return;
        };
        item.completed_at = Some(time);
        self.bump_revision();
        self.layout_changes
            .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
    }

    /// Join only a turn whose user row is in the new page and whose explicitly
    /// phased final row is in the already loaded page.
    pub(in crate::desktop) fn group_archived_boundary(&mut self, inserted: usize) {
        let Some(start) = self.items.iter().take(inserted).rposition(TranscriptItem::is_user) else {
            return;
        };
        let mut final_index = None;
        for (index, item) in self.items().enumerate().skip(start + 1) {
            if item.is_user() || item.turn_id.is_some() {
                return;
            }
            if item.is_assistant() {
                match item.archived_phase.as_deref() {
                    Some("commentary") => {}
                    Some("final_answer") if index >= inserted => {
                        final_index = Some(index);
                        break;
                    }
                    _ => return,
                }
            }
        }
        let Some(end) = final_index else { return };
        let Some(user) = self.items.get(start) else { return };
        let turn_id: SharedString = format!("archive-{}", user.id).into();
        let Some(final_id) = self.items.get(end).map(|item| item.id.clone()) else {
            return;
        };
        for index in start + 1..=end {
            if let Some(item) = self.items.get_mut(index)
                && (item.is_assistant() || item.is_tool())
            {
                item.turn_id = Some(turn_id.clone());
                item.archived_final = index == end;
            }
        }
        self.complete_turn_group(&turn_id, &final_id);
    }

    pub(in crate::desktop) fn group_archived_prefix(&mut self, count: usize) {
        let completed: Vec<_> = self
            .items()
            .take(count)
            .filter(|item| item.archived_final)
            .filter_map(|item| Some((item.turn_id.as_ref()?.to_string(), item.id.clone())))
            .collect();
        for (turn_id, final_id) in completed {
            self.complete_turn_group(&turn_id, &final_id);
        }
    }

    pub(in crate::desktop) fn set_completion_time(&mut self, turn_id: &str, id: &str, completed_at: SharedString) {
        let Some(index) = self.position_by_id(id) else { return };
        let Some(item) = self
            .items
            .get_mut(index)
            .filter(|item| item.is_assistant() && item.turn_id.as_deref() == Some(turn_id))
        else {
            return;
        };
        item.completed_at = Some(completed_at);
        self.bump_revision();
        self.layout_changes
            .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
    }

    pub(in crate::desktop) fn set_item_turn(&mut self, id: &str, turn_id: &str) {
        let Some(index) = self.position_by_id(id) else { return };
        let Some(item) = self.items.get_mut(index) else { return };
        if item.turn_id.is_none() {
            item.turn_id = Some(turn_id.to_string().into());
            self.bump_revision();
        }
    }

    pub(in crate::desktop) fn set_item_canonical_turn(&mut self, id: &str, turn_id: &str) {
        let Some(index) = self.position_by_id(id) else { return };
        let Some(item) = self.items.get_mut(index) else { return };
        if item.canonical_turn_id.is_none() {
            item.canonical_turn_id = Some(turn_id.to_owned().into());
            self.bump_revision();
        }
    }

    pub(in crate::desktop) fn accepts_item_turn(&self, id: &str, turn_id: Option<&str>) -> bool {
        let existing = self
            .position_by_id(id)
            .and_then(|index| self.get(index))
            .and_then(|item| item.turn_id.as_deref());
        !matches!((existing, turn_id), (Some(previous), Some(incoming)) if previous != incoming)
    }

    pub(in crate::desktop) fn complete_turn_group(&mut self, turn_id: &str, final_id: &str) {
        let Some(final_index) = self.position_by_id(final_id) else {
            return;
        };
        if !self
            .items
            .get(final_index)
            .is_some_and(|item| item.is_assistant() && item.turn_id.as_deref() == Some(turn_id))
        {
            return;
        }
        let (files, totals, files_incomplete) =
            summarize_turn_files(self.items.iter().filter(|item| item.turn_id.as_deref() == Some(turn_id)), false);
        if let Some(final_item) = self.items.get_mut(final_index) {
            final_item.turn_files = files;
            final_item.turn_file_totals = totals;
            final_item.turn_files_incomplete = files_incomplete;
        }
        let was_expanded = self
            .items
            .iter()
            .find(|item| item.turn_group_head && item.turn_id.as_deref() == Some(turn_id))
            .is_some_and(|item| item.turn_group_expanded);
        let mut first = None;
        let mut end = final_index + 1;
        for (index, item) in self.items.iter_mut().enumerate() {
            if index != final_index && item.turn_id.as_deref() == Some(turn_id) && !item.is_user() {
                item.turn_grouped = true;
                item.turn_group_head = first.is_none();
                item.turn_group_expanded = was_expanded;
                let _first = first.get_or_insert(index);
                end = end.max(index + 1);
            }
        }
        if let Some(first) = first {
            self.bump_revision();
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(first.min(final_index)..end));
        }
    }

    pub(in crate::desktop) fn set_turn_group_expanded(&mut self, turn_id: &str, expanded: bool) {
        let mut first = None;
        let mut end = 0;
        for (index, item) in self.items.iter_mut().enumerate() {
            if item.turn_grouped && item.turn_id.as_deref() == Some(turn_id) {
                item.turn_group_expanded = expanded;
                let _first = first.get_or_insert(index);
                end = index + 1;
            }
        }
        if let Some(first) = first {
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(first..end));
        }
    }

    pub(in crate::desktop) fn toggle_turn_files(&mut self, id: &str) {
        let Some(index) = self.position_by_id(id) else { return };
        let Some(item) = self.items.get_mut(index) else { return };
        let end = if item.turn_files_offset == 0 {
            3
        } else {
            item.turn_files_offset.saturating_add(50)
        };
        item.turn_files_offset = if end >= item.turn_files.len() { 0 } else { end };
        self.bump_revision();
        self.layout_changes
            .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
    }
}

fn summarize_turn_files<'a>(
    items: impl Iterator<Item = &'a TranscriptItem>,
    incomplete: bool,
) -> (Vec<TurnFile>, Option<(u64, u64)>, bool) {
    let mut files: Vec<TurnFile> = Vec::new();
    let mut files_incomplete = incomplete;
    for item in items {
        if let Some(diff) = item.diff.as_ref().filter(|diff| !diff.failed) {
            files_incomplete |= diff.incomplete;
            let previews = gpui_kit::component::diff::DiffFile::parse(&diff.patch).unwrap_or_default();
            for file in &diff.files {
                if let Some(existing) = files.iter_mut().find(|existing| existing.file.path == file.path) {
                    existing.counts = None;
                } else {
                    let preview = previews.iter().find(|preview| preview.path().as_ref() == file.path.as_str());
                    let (additions, deletions) = crate::desktop::inspector::preview_counts(preview, diff.incomplete);
                    files.push(TurnFile {
                        file: file.clone(),
                        counts: additions.zip(deletions),
                    });
                }
            }
        }
    }
    let totals = (!files.is_empty() && !files_incomplete)
        .then(|| {
            files.iter().try_fold((0_u64, 0_u64), |(added, deleted), file| {
                let (file_added, file_deleted) = file.counts?;
                Some((added.checked_add(file_added)?, deleted.checked_add(file_deleted)?))
            })
        })
        .flatten();
    (files, totals, files_incomplete)
}
