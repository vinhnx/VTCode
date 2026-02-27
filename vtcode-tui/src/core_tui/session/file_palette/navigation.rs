use super::{FileEntry, FilePalette, PAGE_SIZE};

impl FilePalette {
    pub fn move_selection_up(&mut self) {
        if self.filtered_files.is_empty() {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = self.filtered_files.len().saturating_sub(1);
        }
        self.update_page_from_selection();
    }

    pub fn move_selection_down(&mut self) {
        if self.filtered_files.is_empty() {
            return;
        }
        if self.selected_index + 1 < self.filtered_files.len() {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
        self.update_page_from_selection();
    }

    pub fn move_to_first(&mut self) {
        if !self.filtered_files.is_empty() {
            self.selected_index = 0;
            self.current_page = 0;
        }
    }

    pub fn move_to_last(&mut self) {
        if !self.filtered_files.is_empty() {
            self.selected_index = self.filtered_files.len().saturating_sub(1);
            self.update_page_from_selection();
        }
    }

    pub fn page_up(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
            self.selected_index = self.current_page * PAGE_SIZE;
        }
    }

    pub fn page_down(&mut self) {
        let total_pages = self.total_pages();
        if self.current_page + 1 < total_pages {
            self.current_page += 1;
            self.selected_index = self.current_page * PAGE_SIZE;
        }
    }

    fn update_page_from_selection(&mut self) {
        self.current_page = self.selected_index / PAGE_SIZE;
    }

    pub fn get_selected(&self) -> Option<&FileEntry> {
        self.filtered_files.get(self.selected_index)
    }

    /// Get the best matching file entry based on current filter query
    /// Used for Tab autocomplete - returns the first filtered file if any exist
    #[allow(dead_code)]
    pub fn get_best_match(&self) -> Option<&FileEntry> {
        // Return the first file in filtered results (already sorted by score)
        self.filtered_files.first()
    }

    /// Set selection to the best match
    pub fn select_best_match(&mut self) {
        if !self.filtered_files.is_empty() {
            self.selected_index = 0;
            self.current_page = 0;
        }
    }

    pub fn current_page_items(&self) -> Vec<(usize, &FileEntry, bool)> {
        let start = self.current_page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.filtered_files.len());

        self.filtered_files[start..end]
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let global_idx = start + idx;
                let is_selected = global_idx == self.selected_index;
                (global_idx, entry, is_selected)
            })
            .collect()
    }

    pub fn total_pages(&self) -> usize {
        if self.filtered_files.is_empty() {
            1
        } else {
            self.filtered_files.len().div_ceil(PAGE_SIZE)
        }
    }

    pub fn current_page_number(&self) -> usize {
        self.current_page + 1
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
        let end = ((self.current_page + 1) * PAGE_SIZE).min(self.filtered_files.len());
        end < self.filtered_files.len()
    }
}
