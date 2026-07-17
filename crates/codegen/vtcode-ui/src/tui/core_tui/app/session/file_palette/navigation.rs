use std::path::PathBuf;

use super::{FileEntry, FilePalette};

const PAGE_JUMP: usize = 10;

impl FilePalette {
    pub fn move_selection_up(&mut self) {
        match self.selected {
            Some(sel) => self.selected = Some(sel.saturating_sub(1)),
            None if !self.filtered_files.is_empty() => self.selected = Some(0),
            None => {}
        }
    }

    pub fn move_selection_down(&mut self) {
        if self.filtered_files.is_empty() {
            self.selected = None;
            return;
        }
        let max = self.filtered_files.len() - 1;
        self.selected = Some(match self.selected {
            Some(sel) => sel.min(max).saturating_add(1).min(max),
            None => 0,
        });
    }

    pub fn move_to_first(&mut self) {
        self.select_first();
    }

    pub fn move_to_last(&mut self) {
        self.selected = if self.filtered_files.is_empty() {
            None
        } else {
            Some(self.filtered_files.len() - 1)
        };
    }

    pub fn page_up(&mut self) {
        for _ in 0..PAGE_JUMP {
            self.move_selection_up();
        }
    }

    pub fn page_down(&mut self) {
        for _ in 0..PAGE_JUMP {
            self.move_selection_down();
        }
    }

    pub fn get_selected(&self) -> Option<&FileEntry> {
        self.selected.and_then(|i| self.filtered_files.get(i))
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        if index < self.filtered_files.len() {
            self.selected = Some(index);
            true
        } else {
            false
        }
    }

    pub fn select_best_match(&mut self) {
        self.select_first();
    }

    /// Descend into the selected directory, or ascend when the `..` entry is
    /// selected. No-op on a file or when nothing is selected.
    pub fn enter_selected_dir(&mut self) {
        let Some(entry) = self.get_selected().cloned() else {
            return;
        };
        if !entry.is_dir {
            return;
        }
        if entry.is_parent {
            self.go_up();
            return;
        }
        self.last_entered = Some(entry.display_name.trim_end_matches('/').to_string());
        self.current_dir = PathBuf::from(entry.path);
        self.rebuild_dir_listing();
    }

    /// Ascend one directory level, reselecting the directory just left.
    pub fn go_up(&mut self) {
        if self.current_dir == self.workspace_root {
            return;
        }
        let child = self.last_entered.clone();
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
        }
        self.rebuild_dir_listing();
        if let Some(name) = child
            && let Some(pos) = self
                .filtered_files
                .iter()
                .position(|e| e.is_dir && !e.is_parent && e.display_name == format!("{name}/"))
        {
            self.selected = Some(pos);
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

    /// Whether the palette has anything to show. True when there are visible
    /// entries (`filtered_files`, populated immediately by Browse mode via the
    /// directory lister) *or* a search corpus (`all_files`, populated lazily by
    /// the background discovery task). Basing this solely on `all_files` would
    /// collapse the panel layout and swallow mouse input during Browse mode
    /// before the index finishes; basing it solely on `filtered_files` would
    /// make an empty search-listing with a loaded corpus report `false`.
    pub fn has_files(&self) -> bool {
        !self.filtered_files.is_empty() || !self.all_files.is_empty()
    }

    pub fn has_more_items(&self) -> bool {
        false
    }
}
