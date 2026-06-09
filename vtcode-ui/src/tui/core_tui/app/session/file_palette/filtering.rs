use std::collections::BTreeMap;
use std::path::Path;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui_cheese::tree::{TreeGroup, TreeItem, TreeState};

use super::{FileEntry, FilePalette};

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
        self.all_files = files
            .into_iter()
            .filter(|path| !Self::should_exclude_file(Path::new(path)))
            .map(|path| {
                let relative_path = Self::make_relative(&self.workspace_root, &path);
                let is_dir = Path::new(&path).is_dir();
                let display_name = if is_dir {
                    format!("{}/", relative_path)
                } else {
                    relative_path.clone()
                };
                FileEntry {
                    path,
                    display_name,
                    relative_path,
                    is_dir,
                }
            })
            .collect();

        self.all_files.sort_by(|a, b| {
            a.relative_path
                .to_lowercase()
                .cmp(&b.relative_path.to_lowercase())
        });
        self.apply_filter();
    }

    fn make_relative(workspace: &Path, file_path: &str) -> String {
        let path = Path::new(file_path);
        path.strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    pub fn set_filter(&mut self, query: String) {
        self.filter_query.clone_from(&query);

        if let Some(cached) = self.filter_cache.get(&query) {
            self.filtered_files.clone_from(cached);
            self.rebuild_tree();
        } else {
            self.apply_filter();
            if !query.is_empty() && self.filter_cache.len() < 50 {
                self.filter_cache.insert(query, self.filtered_files.clone());
            }
        }
    }

    pub(super) fn apply_filter(&mut self) {
        if self.filter_query.is_empty() {
            self.filtered_files.clone_from(&self.all_files);
            self.rebuild_tree();
            return;
        }

        let query_lower = self.filter_query.to_lowercase();
        let mut scored_indices: Vec<(usize, usize)> = Vec::with_capacity(self.all_files.len() / 2);
        let mut buffer = Vec::new();

        for (idx, entry) in self.all_files.iter().enumerate() {
            let path_lower = entry.relative_path.to_lowercase();

            if let Some(fuzzy_score) =
                Self::simple_fuzzy_match_with_buffer(&path_lower, &query_lower, &mut buffer)
            {
                let mut score = fuzzy_score;
                if !path_lower.contains('/') {
                    score += 1000;
                }
                if path_lower == query_lower {
                    score += 10000;
                } else if let Some(file_name) = path_lower.rsplit('/').next() {
                    if file_name == query_lower {
                        score += 5000;
                    } else if file_name.starts_with(&query_lower) {
                        score += 2000;
                    }
                }
                scored_indices.push((score, idx));
            } else if path_lower.contains(&query_lower) {
                let mut score = Self::calculate_match_score(&path_lower, &query_lower);
                if !path_lower.contains('/') {
                    score += 1000;
                }
                scored_indices.push((score, idx));
            }
        }

        scored_indices.sort_unstable_by(|a, b| {
            let entry_a = &self.all_files[a.1];
            let entry_b = &self.all_files[b.1];
            match (entry_a.is_dir, entry_b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.0.cmp(&a.0).then_with(|| {
                    entry_a
                        .relative_path
                        .to_lowercase()
                        .cmp(&entry_b.relative_path.to_lowercase())
                }),
            }
        });

        self.filtered_files = scored_indices
            .into_iter()
            .map(|(_, idx)| self.all_files[idx].clone())
            .collect();
        self.rebuild_tree();
    }

    fn rebuild_tree(&mut self) {
        // Save expanded state before rebuilding.
        let old_expanded: Vec<bool> = (0..self.tree_groups.len())
            .map(|i| self.tree_state.is_expanded(i))
            .collect();

        let (groups, group_entries) = Self::build_tree_groups(&self.filtered_files);
        let num_groups = groups.len();
        self.tree_groups = groups;
        self.group_entries = group_entries;
        self.tree_state = TreeState::new(num_groups);

        // Restore expanded state for groups that still exist.
        for (i, &was_expanded) in old_expanded.iter().enumerate() {
            if i < num_groups && was_expanded {
                self.tree_state.expand(i);
            }
        }
    }

    pub(super) fn build_tree_groups(files: &[FileEntry]) -> (Vec<TreeGroup>, Vec<Vec<FileEntry>>) {
        let mut top_level_dirs: Vec<&FileEntry> = Vec::new();
        let mut top_level_files: Vec<&FileEntry> = Vec::new();
        let mut dir_children: BTreeMap<String, Vec<&FileEntry>> = BTreeMap::new();

        for entry in files {
            let relative = &entry.relative_path;
            if let Some(slash_pos) = relative.find('/') {
                let top_dir = &relative[..slash_pos];
                dir_children
                    .entry(top_dir.to_owned())
                    .or_default()
                    .push(entry);
            } else if entry.is_dir {
                top_level_dirs.push(entry);
            } else {
                top_level_files.push(entry);
            }
        }

        top_level_dirs.retain(|entry| !dir_children.contains_key(entry.relative_path.as_str()));

        let mut groups = Vec::new();
        let mut group_entries = Vec::new();

        let mut dir_iter: Vec<_> = dir_children.into_iter().collect();
        dir_iter.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        for (dir_name, mut children) in dir_iter {
            children.sort_by(|a, b| {
                a.relative_path
                    .to_lowercase()
                    .cmp(&b.relative_path.to_lowercase())
            });

            let prefix = format!("{dir_name}/");
            let child_items: Vec<TreeItem> = children
                .iter()
                .map(|entry| {
                    let name = entry
                        .relative_path
                        .strip_prefix(&prefix)
                        .unwrap_or(&entry.relative_path)
                        .to_owned();
                    TreeItem::new(if entry.is_dir {
                        format!("{name}/")
                    } else {
                        name
                    })
                })
                .collect();
            let child_entries: Vec<FileEntry> = children.iter().map(|e| (*e).clone()).collect();

            groups
                .push(TreeGroup::new(TreeItem::new(format!("{dir_name}/"))).children(child_items));
            group_entries.push(child_entries);
        }

        top_level_dirs.sort_by(|a, b| {
            a.relative_path
                .to_lowercase()
                .cmp(&b.relative_path.to_lowercase())
        });
        for entry in top_level_dirs {
            groups.push(TreeGroup::new(TreeItem::new(format!(
                "{}/",
                entry.relative_path
            ))));
            group_entries.push(vec![(*entry).clone()]);
        }

        top_level_files.sort_by(|a, b| {
            a.relative_path
                .to_lowercase()
                .cmp(&b.relative_path.to_lowercase())
        });
        for entry in top_level_files {
            groups.push(TreeGroup::new(TreeItem::new(entry.display_name.clone())));
            group_entries.push(vec![(*entry).clone()]);
        }

        (groups, group_entries)
    }

    #[expect(dead_code)]
    pub(super) fn simple_fuzzy_match(path: &str, query: &str) -> Option<usize> {
        let mut buffer = Vec::new();
        Self::simple_fuzzy_match_with_buffer(path, query, &mut buffer)
    }

    fn simple_fuzzy_match_with_buffer(
        path: &str,
        query: &str,
        buffer: &mut Vec<char>,
    ) -> Option<usize> {
        if query.is_empty() {
            return Some(1000);
        }

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let utf32_path = Utf32Str::new(path, buffer);
        let score = pattern.score(utf32_path, &mut matcher)?;

        let mut adjusted_score = score as usize;
        let query_lower = query.to_lowercase();

        if let Some(filename) = path.rsplit('/').next()
            && filename.to_lowercase().contains(&query_lower)
        {
            adjusted_score += 500;
        }

        Some(adjusted_score)
    }

    fn calculate_match_score(path: &str, query: &str) -> usize {
        let mut score: usize = 0;

        if path == query {
            return 10000;
        }

        if path.starts_with(query) {
            score += 1000;
        }

        if let Some(file_name) = path.rsplit('/').next() {
            if file_name == query {
                score += 2000;
            } else if file_name.contains(query) {
                score += 500;
            }
            if file_name.starts_with(query) {
                score += 200;
            }
        }

        for segment in path.split('/') {
            if segment.contains(query) {
                score += 50;
            }
        }

        let depth = path.matches('/').count();
        score = score.saturating_sub(depth * 5);

        let matches = path.matches(query).count();
        score += matches * 10;

        score
    }

    #[expect(dead_code)]
    pub fn style_for_entry(&self, entry: &FileEntry) -> Option<anstyle::Style> {
        let path = Path::new(&entry.path);
        self.file_colorizer.style_for_path(path)
    }
}
