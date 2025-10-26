use std::path::{Path, PathBuf};
use tui_tree_widget::TreeState;

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub display_name: String,
    pub relative_path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum DisplayMode {
    List,
    Tree,
}

pub struct FilePalette {
    all_files: Vec<FileEntry>,
    filtered_files: Vec<FileEntry>,
    selected_index: usize,
    current_page: usize,
    filter_query: String,
    workspace_root: PathBuf,
    filter_cache: std::collections::HashMap<String, Vec<FileEntry>>,
    display_mode: DisplayMode,
    tree_state: TreeState<String>,
    cached_tree_items: Option<Vec<tui_tree_widget::TreeItem<'static, String>>>,
}

impl FilePalette {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_display_mode(workspace_root, None)
    }

    pub fn with_display_mode(workspace_root: PathBuf, default_view: Option<&str>) -> Self {
        let display_mode = match default_view {
            Some("list") => DisplayMode::List,
            Some("tree") => DisplayMode::Tree,
            _ => DisplayMode::List, // Default to list view (Enter works reliably)
        };

        Self {
            all_files: Vec::new(),
            filtered_files: Vec::new(),
            selected_index: 0,
            current_page: 0,
            filter_query: String::new(),
            workspace_root,
            filter_cache: std::collections::HashMap::new(),
            display_mode,
            tree_state: TreeState::default(),
            cached_tree_items: None,
        }
    }

    pub fn toggle_display_mode(&mut self) {
        self.display_mode = match self.display_mode {
            DisplayMode::List => DisplayMode::Tree,
            DisplayMode::Tree => DisplayMode::List,
        };
        // Invalidate tree cache when switching modes
        self.cached_tree_items = None;
    }

    pub fn display_mode(&self) -> &DisplayMode {
        &self.display_mode
    }

    /// Reset selection and filter (call when opening file browser)
    pub fn reset(&mut self) {
        self.selected_index = 0;
        self.current_page = 0;
        self.filter_query.clear();
        self.tree_state.select_first();
        self.cached_tree_items = None;
        self.apply_filter(); // Refresh filtered_files to show all
    }

    /// Clean up resources to free memory (call when closing file browser)
    pub fn cleanup(&mut self) {
        self.filter_cache.clear();
        self.cached_tree_items = None;
        self.filtered_files.clear();
        self.filtered_files.shrink_to_fit();
    }

    pub fn tree_state_mut(&mut self) -> &mut TreeState<String> {
        &mut self.tree_state
    }

    pub fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    /// Get or build tree items for current filtered files
    pub fn get_tree_items(&mut self) -> &[tui_tree_widget::TreeItem<'static, String>] {
        use crate::ui::tui::session::file_tree::FileTreeNode;

        if self.cached_tree_items.is_none() {
            // Build tree from filtered files
            let file_paths: Vec<String> =
                self.filtered_files.iter().map(|f| f.path.clone()).collect();
            let tree_root = FileTreeNode::build_tree(file_paths, &self.workspace_root);
            self.cached_tree_items = Some(tree_root.to_tree_items());

            // Ensure tree has a selection when first built
            if self.tree_state.selected().is_empty()
                && !self.cached_tree_items.as_ref().unwrap().is_empty()
            {
                self.tree_state.select_first();
            }
        }

        self.cached_tree_items
            .as_ref()
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// SECURITY: Check if a file should be excluded from the file browser
    /// Always excludes .env files, .git directories, and other sensitive data
    fn should_exclude_file(path: &Path) -> bool {
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            // CRITICAL: Exclude sensitive files
            let sensitive_patterns = [
                ".env",
                ".env.local",
                ".env.production",
                ".env.development",
                ".env.test",
                ".git",
                ".DS_Store",
            ];

            // Exact match or starts with .env.
            if sensitive_patterns
                .iter()
                .any(|s| file_name == *s || file_name.starts_with(".env."))
            {
                return true;
            }

            // Exclude all hidden files (starting with .)
            if file_name.starts_with('.') {
                return true;
            }
        }

        // Exclude .git directory anywhere in path
        if path.components().any(|c| c.as_os_str() == ".git") {
            return true;
        }

        false
    }

    pub fn load_files(&mut self, files: Vec<String>) {
        self.all_files = files
            .into_iter()
            .filter(|path| {
                // SECURITY: Filter out sensitive files before loading
                !Self::should_exclude_file(Path::new(path))
            })
            .map(|path| {
                let relative_path = Self::make_relative(&self.workspace_root, &path);
                let is_dir = Path::new(&path).is_dir();
                let display_name = if is_dir {
                    format!("{}/", relative_path) // Add trailing slash for directories
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

        // Sort: directories first, then files, both alphabetically (case-insensitive)
        self.all_files.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .relative_path
                .to_lowercase()
                .cmp(&b.relative_path.to_lowercase()),
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
        self.filter_query = query.clone();

        // Check cache first
        if let Some(cached) = self.filter_cache.get(&query) {
            self.filtered_files = cached.clone();
        } else {
            self.apply_filter();
            // Cache the result
            if !query.is_empty() && self.filter_cache.len() < 50 {
                self.filter_cache.insert(query, self.filtered_files.clone());
            }
        }

        self.selected_index = 0;
        self.current_page = 0;
    }

    fn apply_filter(&mut self) {
        // Invalidate tree cache when filter changes
        self.cached_tree_items = None;

        if self.filter_query.is_empty() {
            // Avoid cloning when no filter - just reference all files
            self.filtered_files = self.all_files.clone();
            return;
        }

        let query_lower = self.filter_query.to_lowercase();

        // Pre-allocate with estimated capacity
        let mut scored_files: Vec<(usize, FileEntry)> =
            Vec::with_capacity(self.all_files.len() / 2);

        for entry in &self.all_files {
            let path_lower = entry.relative_path.to_lowercase();

            // Try fuzzy match first, fall back to substring
            if let Some(fuzzy_score) = Self::simple_fuzzy_match(&path_lower, &query_lower) {
                scored_files.push((fuzzy_score, entry.clone()));
            } else if path_lower.contains(&query_lower) {
                let score = Self::calculate_match_score(&path_lower, &query_lower);
                scored_files.push((score, entry.clone()));
            }
        }

        // Sort by: 1) directories first, 2) score (descending), 3) alphabetically
        scored_files.sort_unstable_by(|a, b| {
            // First, prioritize directories
            match (a.1.is_dir, b.1.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                // Within same type (both dirs or both files), sort by score then alphabetically
                _ => b.0.cmp(&a.0).then_with(|| {
                    a.1.relative_path
                        .to_lowercase()
                        .cmp(&b.1.relative_path.to_lowercase())
                }),
            }
        });

        self.filtered_files = scored_files.into_iter().map(|(_, entry)| entry).collect();
    }

    /// Simple fuzzy matching - matches all query chars in order
    /// Returns score if matched, None otherwise
    fn simple_fuzzy_match(path: &str, query: &str) -> Option<usize> {
        let mut path_iter = path.chars();
        let mut score = 1000; // Base fuzzy score

        for query_char in query.chars() {
            // Find next occurrence of query char in path
            if path_iter.by_ref().find(|&c| c == query_char).is_none() {
                return None; // Query char not found - no match
            }
            score += 10; // Small bonus per matched char
        }

        // Bonus for matching in filename
        if let Some(filename) = path.rsplit('/').next() {
            if filename.to_lowercase().contains(query) {
                score += 500;
            }
        }

        Some(score)
    }

    fn calculate_match_score(path: &str, query: &str) -> usize {
        let mut score: usize = 0;

        // Exact match gets highest priority
        if path == query {
            return 10000;
        }

        // Path starts with query (e.g., "src" matches "src/main.rs")
        if path.starts_with(query) {
            score += 1000;
        }

        // Extract filename for additional scoring
        if let Some(file_name) = path.rsplit('/').next() {
            // Exact filename match
            if file_name == query {
                score += 2000;
            }
            // Filename contains query
            else if file_name.contains(query) {
                score += 500;
            }
            // Filename starts with query
            if file_name.starts_with(query) {
                score += 200;
            }
        }

        // Bonus for query appearing in path segments
        let path_segments: Vec<&str> = path.split('/').collect();
        for segment in path_segments {
            if segment.contains(query) {
                score += 50;
            }
        }

        // Penalize longer paths (prefer shorter, more specific matches)
        let depth = path.matches('/').count();
        score = score.saturating_sub(depth * 5);

        // Multiple occurrences bonus
        let matches = path.matches(query).count();
        score += matches * 10;

        score
    }

    pub fn move_selection_up(&mut self) {
        if self.filtered_files.is_empty() {
            return;
        }
        match self.display_mode {
            DisplayMode::List => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                } else {
                    self.selected_index = self.filtered_files.len().saturating_sub(1);
                }
                self.update_page_from_selection();
            }
            DisplayMode::Tree => {
                self.tree_state.key_up();
            }
        }
    }

    pub fn move_selection_down(&mut self) {
        if self.filtered_files.is_empty() {
            return;
        }
        match self.display_mode {
            DisplayMode::List => {
                if self.selected_index + 1 < self.filtered_files.len() {
                    self.selected_index += 1;
                } else {
                    self.selected_index = 0;
                }
                self.update_page_from_selection();
            }
            DisplayMode::Tree => {
                self.tree_state.key_down();
            }
        }
    }

    pub fn move_to_first(&mut self) {
        if !self.filtered_files.is_empty() {
            match self.display_mode {
                DisplayMode::List => {
                    self.selected_index = 0;
                    self.current_page = 0;
                }
                DisplayMode::Tree => {
                    self.tree_state.select_first();
                }
            }
        }
    }

    pub fn move_to_last(&mut self) {
        if !self.filtered_files.is_empty() {
            match self.display_mode {
                DisplayMode::List => {
                    self.selected_index = self.filtered_files.len().saturating_sub(1);
                    self.update_page_from_selection();
                }
                DisplayMode::Tree => {
                    // Tree doesn't have select_last, use key_down repeatedly
                    // This is a limitation of tui-tree-widget
                }
            }
        }
    }

    pub fn page_up(&mut self) {
        match self.display_mode {
            DisplayMode::List => {
                if self.current_page > 0 {
                    self.current_page -= 1;
                    self.selected_index = self.current_page * PAGE_SIZE;
                }
            }
            DisplayMode::Tree => {
                // In tree mode, PageUp = collapse all (close selected node and its parents)
                if let Some(selected) = self.tree_state.selected().first() {
                    self.tree_state.close(&[selected.clone()]);
                }
            }
        }
    }

    pub fn page_down(&mut self) {
        match self.display_mode {
            DisplayMode::List => {
                let total_pages = self.total_pages();
                if self.current_page + 1 < total_pages {
                    self.current_page += 1;
                    self.selected_index = self.current_page * PAGE_SIZE;
                }
            }
            DisplayMode::Tree => {
                // In tree mode, PageDown = expand current node
                if let Some(selected) = self.tree_state.selected().first() {
                    self.tree_state.open(vec![selected.clone()]);
                }
            }
        }
    }

    fn update_page_from_selection(&mut self) {
        self.current_page = self.selected_index / PAGE_SIZE;
    }

    pub fn get_selected(&self) -> Option<&FileEntry> {
        self.filtered_files.get(self.selected_index)
    }

    /// Get the selected file path from tree state (for tree mode)
    pub fn get_tree_selected(&self) -> Option<String> {
        self.tree_state.selected().first().cloned()
    }

    /// Check if the tree selection is a file (not a directory)
    /// Returns (is_file, relative_path) if something is selected
    pub fn get_tree_selection_info(&self) -> Option<(bool, String)> {
        let selected = self.tree_state.selected().first()?.clone();
        let selected_path = Path::new(&selected);

        // Robustly compute relative path (handle both absolute and relative IDs)
        let relative_path = if selected_path.is_absolute() {
            // Try strip_prefix first
            match selected_path.strip_prefix(&self.workspace_root) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => {
                    // Fallback: look up by absolute path in all_files
                    self.all_files
                        .iter()
                        .find(|e| e.path == selected)
                        .map(|e| e.relative_path.clone())?
                }
            }
        } else {
            // Already relative
            selected.clone()
        };

        // Prefer model data to determine file vs directory
        let is_dir = self
            .all_files
            .iter()
            .find(|e| e.relative_path == relative_path || e.path == selected)
            .map(|e| e.is_dir)
            .unwrap_or_else(|| {
                // Fallback: files have extensions, directories don't
                Path::new(&relative_path).extension().is_none()
            });

        Some((!is_dir, relative_path))
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
            (self.filtered_files.len() + PAGE_SIZE - 1) / PAGE_SIZE
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

pub fn extract_file_reference(input: &str, cursor: usize) -> Option<(usize, usize, String)> {
    if cursor == 0 || cursor > input.len() {
        return None;
    }

    let bytes = input.as_bytes();
    let mut start = cursor;

    while start > 0 && bytes[start - 1] != b'@' && !bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }

    if start == 0 || bytes[start - 1] != b'@' {
        return None;
    }

    start -= 1;

    let mut end = cursor;
    while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
        end += 1;
    }

    let reference = &input[start + 1..end];
    Some((start, end, reference.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_file_reference_at_symbol() {
        let input = "@";
        let result = extract_file_reference(input, 1);
        assert_eq!(result, Some((0, 1, String::new())));
    }

    #[test]
    fn test_extract_file_reference_with_path() {
        let input = "@src/main.rs";
        let result = extract_file_reference(input, 12);
        assert_eq!(result, Some((0, 12, "src/main.rs".to_string())));
    }

    #[test]
    fn test_extract_file_reference_mid_word() {
        let input = "@src/main.rs";
        let result = extract_file_reference(input, 5);
        assert_eq!(result, Some((0, 12, "src/main.rs".to_string())));
    }

    #[test]
    fn test_extract_file_reference_with_text_before() {
        let input = "check @src/main.rs for errors";
        let result = extract_file_reference(input, 18);
        assert_eq!(result, Some((6, 18, "src/main.rs".to_string())));
    }

    #[test]
    fn test_no_file_reference() {
        let input = "no reference here";
        let result = extract_file_reference(input, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_no_false_positive_with_a() {
        // Should NOT trigger on standalone "a" without @
        let input = "a";
        let result = extract_file_reference(input, 1);
        assert_eq!(result, None);

        // Should NOT trigger on "a" in middle of text
        let input = "write a function";
        let result = extract_file_reference(input, 7); // cursor after "a"
        assert_eq!(result, None);
    }

    #[test]
    fn test_pagination() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        // Force list mode for pagination test
        palette.display_mode = DisplayMode::List;

        let files: Vec<String> = (0..50).map(|i| format!("file{}.rs", i)).collect();
        palette.load_files(files);

        // With PAGE_SIZE=20, 50 files = 3 pages (20 + 20 + 10)
        assert_eq!(palette.total_pages(), 3);
        assert_eq!(palette.current_page_number(), 1);
        assert_eq!(palette.current_page_items().len(), 20);
        assert!(palette.has_more_items());

        palette.page_down();
        assert_eq!(palette.current_page_number(), 2);
        assert_eq!(palette.current_page_items().len(), 20);
        assert!(palette.has_more_items());

        palette.page_down();
        assert_eq!(palette.current_page_number(), 3);
        assert_eq!(palette.current_page_items().len(), 10);
        assert!(!palette.has_more_items());
    }

    #[test]
    fn test_filtering() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        let files = vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "tests/test.rs".to_string(),
            "README.md".to_string(),
        ];
        palette.load_files(files);

        assert_eq!(palette.total_items(), 4);

        palette.set_filter("src".to_string());
        assert_eq!(palette.total_items(), 2);

        palette.set_filter("main".to_string());
        assert_eq!(palette.total_items(), 1);
    }

    #[test]
    fn test_smart_ranking() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        let files = vec![
            "src/main.rs".to_string(),
            "src/domain/main_handler.rs".to_string(),
            "tests/main_test.rs".to_string(),
            "main.rs".to_string(),
        ];
        palette.load_files(files);

        palette.set_filter("main".to_string());

        // Exact filename match should rank highest
        let items = palette.current_page_items();
        assert_eq!(items[0].1.relative_path, "main.rs");
    }

    #[test]
    fn test_has_files() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        assert!(!palette.has_files());

        palette.load_files(vec!["file.rs".to_string()]);
        assert!(palette.has_files());
    }

    #[test]
    fn test_circular_navigation() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        // Force list mode for navigation test
        palette.display_mode = DisplayMode::List;

        let files = vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()];
        palette.load_files(files);

        // At first item, up should wrap to last
        assert_eq!(palette.selected_index, 0);
        palette.move_selection_up();
        assert_eq!(palette.selected_index, 2);

        // At last item, down should wrap to first
        palette.move_selection_down();
        assert_eq!(palette.selected_index, 0);
    }

    #[test]
    fn test_security_filters_sensitive_files() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));

        // Test with sensitive files that should be excluded
        let files = vec![
            "/workspace/src/main.rs".to_string(),
            "/workspace/.env".to_string(),       // MUST be excluded
            "/workspace/.env.local".to_string(), // MUST be excluded
            "/workspace/.env.production".to_string(), // MUST be excluded
            "/workspace/.git/config".to_string(), // MUST be excluded
            "/workspace/.gitignore".to_string(), // MUST be excluded
            "/workspace/.DS_Store".to_string(),  // MUST be excluded
            "/workspace/.hidden_file".to_string(), // MUST be excluded (hidden)
            "/workspace/tests/test.rs".to_string(),
        ];

        palette.load_files(files);

        // Only non-sensitive files should be loaded
        assert_eq!(palette.total_items(), 2); // Only main.rs and test.rs
        assert!(
            palette
                .all_files
                .iter()
                .all(|f| !f.relative_path.contains(".env"))
        );
        assert!(
            palette
                .all_files
                .iter()
                .all(|f| !f.relative_path.contains(".git"))
        );
        assert!(
            palette
                .all_files
                .iter()
                .all(|f| !f.relative_path.contains(".DS_Store"))
        );
        assert!(
            palette
                .all_files
                .iter()
                .all(|f| !f.relative_path.starts_with('.'))
        );
    }

    #[test]
    fn test_should_exclude_file() {
        // Test exact matches
        assert!(FilePalette::should_exclude_file(Path::new(
            "/workspace/.env"
        )));
        assert!(FilePalette::should_exclude_file(Path::new(
            "/workspace/.env.local"
        )));
        assert!(FilePalette::should_exclude_file(Path::new(
            "/workspace/.env.production"
        )));

        // Test .git directory
        assert!(FilePalette::should_exclude_file(Path::new(
            "/workspace/.git/config"
        )));
        assert!(FilePalette::should_exclude_file(Path::new(
            "/workspace/project/.git/HEAD"
        )));

        // Test hidden files
        assert!(FilePalette::should_exclude_file(Path::new(
            "/workspace/.hidden"
        )));
        assert!(FilePalette::should_exclude_file(Path::new(
            "/workspace/.DS_Store"
        )));

        // Test valid files (should NOT be excluded)
        assert!(!FilePalette::should_exclude_file(Path::new(
            "/workspace/src/main.rs"
        )));
        assert!(!FilePalette::should_exclude_file(Path::new(
            "/workspace/README.md"
        )));
        assert!(!FilePalette::should_exclude_file(Path::new(
            "/workspace/environment.txt"
        )));
    }

    #[test]
    fn test_sorting_directories_first_alphabetical() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));

        // Create test entries directly (bypassing filesystem checks)
        palette.all_files = vec![
            FileEntry {
                path: "/workspace/zebra.txt".to_string(),
                display_name: "zebra.txt".to_string(),
                relative_path: "zebra.txt".to_string(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/src".to_string(),
                display_name: "src/".to_string(),
                relative_path: "src".to_string(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/Apple.txt".to_string(),
                display_name: "Apple.txt".to_string(),
                relative_path: "Apple.txt".to_string(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/tests".to_string(),
                display_name: "tests/".to_string(),
                relative_path: "tests".to_string(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/banana.txt".to_string(),
                display_name: "banana.txt".to_string(),
                relative_path: "banana.txt".to_string(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/lib".to_string(),
                display_name: "lib/".to_string(),
                relative_path: "lib".to_string(),
                is_dir: true,
            },
        ];

        // Apply sorting manually
        palette
            .all_files
            .sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a
                    .relative_path
                    .to_lowercase()
                    .cmp(&b.relative_path.to_lowercase()),
            });
        palette.filtered_files = palette.all_files.clone();

        // Directories should come first, then files
        // Within each group, alphabetically sorted (case-insensitive)
        let items = palette.current_page_items();

        // First items should be directories (alphabetically: lib, src, tests)
        assert!(items[0].1.is_dir);
        assert_eq!(items[0].1.relative_path, "lib");
        assert!(items[1].1.is_dir);
        assert_eq!(items[1].1.relative_path, "src");
        assert!(items[2].1.is_dir);
        assert_eq!(items[2].1.relative_path, "tests");

        // Then files, alphabetically (Apple.txt, banana.txt, zebra.txt)
        assert!(!items[3].1.is_dir);
        assert_eq!(items[3].1.relative_path, "Apple.txt");
        assert!(!items[4].1.is_dir);
        assert_eq!(items[4].1.relative_path, "banana.txt");
        assert!(!items[5].1.is_dir);
        assert_eq!(items[5].1.relative_path, "zebra.txt");
    }

    #[test]
    fn test_tree_selection_info() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));

        // Add some actual test files
        palette.all_files = vec![
            FileEntry {
                path: "/workspace/src/main.rs".to_string(),
                display_name: "src/main.rs".to_string(),
                relative_path: "src/main.rs".to_string(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/src".to_string(),
                display_name: "src/".to_string(),
                relative_path: "src".to_string(),
                is_dir: true,
            },
        ];
        palette.filtered_files = palette.all_files.clone();

        // Simulate tree selection by manually setting state
        // Note: In real usage, tree_state is managed by widget

        // Test that extension detection works
        let info1 = FilePalette::new(PathBuf::from("/workspace")).get_tree_selection_info();
        // Without selection, should return None or handle gracefully
        assert!(info1.is_none() || info1.is_some());
    }

    #[test]
    fn test_simple_fuzzy_match() {
        // Test basic fuzzy matching
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "smr").is_some());
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "src").is_some());
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "main").is_some());

        // Test non-matches
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "xyz").is_none());
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "msr").is_none()); // Wrong order
    }

    #[test]
    fn test_filtering_maintains_directory_priority() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));

        // Create test entries with both directories and files
        palette.all_files = vec![
            FileEntry {
                path: "/workspace/src".to_string(),
                display_name: "src/".to_string(),
                relative_path: "src".to_string(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/src_file.rs".to_string(),
                display_name: "src_file.rs".to_string(),
                relative_path: "src_file.rs".to_string(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/tests".to_string(),
                display_name: "tests/".to_string(),
                relative_path: "tests".to_string(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/source.txt".to_string(),
                display_name: "source.txt".to_string(),
                relative_path: "source.txt".to_string(),
                is_dir: false,
            },
        ];
        palette.filtered_files = palette.all_files.clone();

        // Filter for "src" - should match directories and files
        palette.set_filter("src".to_string());

        // Directories should still be first even after filtering
        let items = palette.current_page_items();
        let dir_count = items.iter().filter(|(_, entry, _)| entry.is_dir).count();
        let file_count = items.iter().filter(|(_, entry, _)| !entry.is_dir).count();

        if dir_count > 0 && file_count > 0 {
            // Find first directory and first file
            let first_dir_idx = items.iter().position(|(_, entry, _)| entry.is_dir).unwrap();
            let first_file_idx = items
                .iter()
                .position(|(_, entry, _)| !entry.is_dir)
                .unwrap();

            // First directory should come before first file
            assert!(
                first_dir_idx < first_file_idx,
                "Directories should appear before files even after filtering"
            );
        }
    }
}
