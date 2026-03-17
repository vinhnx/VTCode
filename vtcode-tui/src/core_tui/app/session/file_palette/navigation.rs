use super::{FileEntry, FilePalette, PAGE_SIZE};

impl FilePalette {
    pub fn move_selection_up(&mut self) {
        self.navigator.move_up();
    }

    pub fn move_selection_down(&mut self) {
        self.navigator.move_down();
    }

    pub fn move_to_first(&mut self) {
        self.navigator.select_first();
    }

    pub fn move_to_last(&mut self) {
        self.navigator.select_last();
    }

    pub fn page_up(&mut self) {
        let current_page = self.current_page_index();
        if current_page == 0 {
            return;
        }
        let new_index = (current_page - 1) * PAGE_SIZE;
        self.navigator.select_index(new_index);
    }

    pub fn page_down(&mut self) {
        let total_pages = self.total_pages();
        let current_page = self.current_page_index();
        if current_page + 1 >= total_pages {
            return;
        }
        let new_index = (current_page + 1) * PAGE_SIZE;
        self.navigator.select_index(new_index);
    }

    pub fn get_selected(&self) -> Option<&FileEntry> {
        self.navigator
            .selected()
            .and_then(|index| self.filtered_files.get(index))
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.navigator.selected()
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        self.navigator.select_index(index)
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
        self.navigator.select_first();
    }

    pub fn current_page_items(&self) -> Vec<(usize, &FileEntry, bool)> {
        let start = self.current_page_index() * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.filtered_files.len());
        let selected = self.navigator.selected();

        self.filtered_files[start..end]
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let global_idx = start + idx;
                let is_selected = selected == Some(global_idx);
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
        if self.filtered_files.is_empty() {
            1
        } else {
            self.current_page_index() + 1
        }
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
        if self.filtered_files.is_empty() {
            return false;
        }
        let end = ((self.current_page_index() + 1) * PAGE_SIZE).min(self.filtered_files.len());
        end < self.filtered_files.len()
    }

    fn current_page_index(&self) -> usize {
        self.navigator.selected().unwrap_or(0) / PAGE_SIZE
    }
}
