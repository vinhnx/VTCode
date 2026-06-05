use super::{FileEntry, FilePalette};

const PAGE_JUMP: usize = 10;

impl FilePalette {
    pub fn move_selection_up(&mut self) {
        self.tree_state.select_prev(&self.tree_groups);
    }

    pub fn move_selection_down(&mut self) {
        self.tree_state.select_next(&self.tree_groups);
    }

    pub fn move_to_first(&mut self) {
        self.tree_state.select(0, None);
    }

    pub fn move_to_last(&mut self) {
        if !self.tree_groups.is_empty() {
            let last = self.tree_groups.len() - 1;
            self.tree_state.select(last, None);
        }
    }

    pub fn page_up(&mut self) {
        for _ in 0..PAGE_JUMP {
            self.tree_state.select_prev(&self.tree_groups);
        }
    }

    pub fn page_down(&mut self) {
        for _ in 0..PAGE_JUMP {
            self.tree_state.select_next(&self.tree_groups);
        }
    }

    pub fn get_selected(&self) -> Option<&FileEntry> {
        let (group_idx, child_idx) = self.tree_state.selected();
        self.group_entries.get(group_idx).and_then(|entries| {
            if let Some(ci) = child_idx {
                entries.get(ci)
            } else {
                entries.first()
            }
        })
    }

    pub fn get_selected_entry(&self) -> Option<&FileEntry> {
        let (group_idx, child_idx) = self.tree_state.selected();
        if let Some(ci) = child_idx {
            self.group_entries
                .get(group_idx)
                .and_then(|entries| entries.get(ci))
        } else {
            None
        }
    }

    pub fn selected_is_group(&self) -> bool {
        self.tree_state.selected().1.is_none()
    }

    pub fn selected_index(&self) -> Option<usize> {
        let (group_idx, child_idx) = self.tree_state.selected();
        let mut flat = 0;
        for (gi, group) in self.tree_groups.iter().enumerate() {
            if gi == group_idx {
                // Group header is at `flat`; first child (if any) is at `flat + 1`.
                return Some(flat + child_idx.map_or(0, |ci| ci + 1));
            }
            flat += 1;
            if self.tree_state.is_expanded(gi) {
                flat += group.children_slice().len();
            }
        }
        None
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        let mut flat = 0;
        for (gi, group) in self.tree_groups.iter().enumerate() {
            if flat == index {
                self.tree_state.select(gi, None);
                return true;
            }
            flat += 1;
            if self.tree_state.is_expanded(gi) {
                let num_children = group.children_slice().len();
                if flat + num_children > index {
                    self.tree_state.select(gi, Some(index - flat));
                    return true;
                }
                flat += num_children;
            }
        }
        false
    }

    pub fn toggle_selected(&mut self) {
        self.tree_state.toggle_selected();
    }

    pub fn expand_selected(&mut self) {
        let (group_idx, _) = self.tree_state.selected();
        self.tree_state.expand(group_idx);
    }

    pub fn collapse_selected(&mut self) {
        let (group_idx, _) = self.tree_state.selected();
        self.tree_state.collapse(group_idx);
    }

    pub fn get_best_match(&self) -> Option<&FileEntry> {
        self.filtered_files.first()
    }

    pub fn select_best_match(&mut self) {
        self.tree_state.select(0, None);
    }

    pub fn current_page_items(&self) -> Vec<(usize, &FileEntry, bool)> {
        let (sel_group, sel_child) = self.tree_state.selected();
        let mut result = Vec::new();
        let mut flat_idx = 0;

        for (gi, _group) in self.tree_groups.iter().enumerate() {
            let entries = &self.group_entries[gi];

            // Group header row (always present).
            if let Some(first_entry) = entries.first() {
                let is_selected = gi == sel_group && sel_child.is_none();
                result.push((flat_idx, first_entry, is_selected));
                flat_idx += 1;
            }

            // Expanded children.
            if self.tree_state.is_expanded(gi) {
                for (ci, entry) in entries.iter().enumerate() {
                    let is_selected = gi == sel_group && sel_child == Some(ci);
                    result.push((flat_idx, entry, is_selected));
                    flat_idx += 1;
                }
            }
        }

        result
    }

    pub fn total_pages(&self) -> usize {
        1
    }

    pub fn current_page_number(&self) -> usize {
        1
    }

    pub fn total_items(&self) -> usize {
        self.filtered_files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.filtered_files.is_empty()
    }

    pub fn filter_query(&self) -> &str {
        &self.filter_query
    }

    pub fn has_files(&self) -> bool {
        !self.all_files.is_empty()
    }

    pub fn has_more_items(&self) -> bool {
        false
    }
}
