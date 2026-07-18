use std::path::Path;

use super::{FilePalette, PickerMode, listing, search::SearchScorer};

impl FilePalette {
    pub(super) fn should_exclude_file(path: &Path) -> bool {
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if vtcode_commons::exclusions::is_sensitive_file(file_name) || file_name == ".git" {
                return true;
            }

            if file_name.starts_with('.') {
                return true;
            }
        }

        if path.components().any(|c| c.as_os_str() == ".git") {
            return true;
        }

        false
    }

    pub fn load_files(&mut self, files: Vec<String>) {
        self.populate_all_files(files);
        self.current_dir = self.workspace_root.clone();
        self.set_filter(String::new());
    }

    pub(super) fn populate_all_files(&mut self, files: Vec<String>) {
        listing::build_entries(self, files, true);
        listing::build_dir_index(self);
    }

    pub fn set_filter(&mut self, query: String) {
        self.filter_query = query;
        if self.filter_query.is_empty() {
            self.mode = PickerMode::Browse;
            listing::rebuild_dir_listing(self);
        } else {
            self.mode = PickerMode::Search;
            self.rebuild_search();
        }
    }

    pub(super) fn rebuild_search(&mut self) {
        let mut scorer = SearchScorer::new();
        scorer.set_query(&self.filter_query);

        if self.all_files.is_empty() {
            self.filtered_files.clear();
            self.select_first();
            return;
        }

        let mut scored: Vec<super::search::ScoredPath> = Vec::with_capacity(self.all_files.len() / 2);

        for (idx, entry) in self.all_files.iter().enumerate() {
            if let Some(mut scored_path) = scorer.score(entry, idx) {
                scored.push(std::mem::replace(
                    &mut scored_path,
                    super::search::ScoredPath {
                        score: 0,
                        index: 0,
                        is_dir: false,
                        path_lower: String::new(),
                    },
                ));
            }
        }

        SearchScorer::sort_results(&mut scored);

        self.filtered_files = scored
            .into_iter()
            .map(|scored_path| self.all_files[scored_path.index].clone())
            .collect();
        self.select_first();
    }

    pub(super) fn select_first(&mut self) {
        if self.filtered_files.is_empty() {
            self.selected = None;
            return;
        }
        let start = if self.filtered_files.first().is_some_and(|e| e.is_parent) {
            1
        } else {
            0
        };
        self.selected = Some(start.min(self.filtered_files.len() - 1));
    }

    #[expect(dead_code)]
    pub(super) fn simple_fuzzy_match(path: &str, query: &str) -> Option<usize> {
        let mut scorer = SearchScorer::new();
        scorer.set_query(query);
        let dummy_entry = super::FileEntry {
            path: path.to_string(),
            display_name: path.to_string(),
            relative_path: path.to_string(),
            is_dir: false,
            is_parent: false,
        };
        scorer.score(&dummy_entry, 0).map(|s| s.score)
    }
}
