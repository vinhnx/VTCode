use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::path::{Path, PathBuf};

use crate::ui::FileColorizer;

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    #[allow(dead_code)]
    pub display_name: String,
    pub relative_path: String,
    pub is_dir: bool,
}

pub struct FilePalette {
    all_files: Vec<FileEntry>,
    filtered_files: Vec<FileEntry>,
    selected_index: usize,
    current_page: usize,
    filter_query: String,
    workspace_root: PathBuf,
    filter_cache: std::collections::HashMap<String, Vec<FileEntry>>,
    #[allow(dead_code)]
    file_colorizer: FileColorizer,
}

impl FilePalette {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            all_files: Vec::new(),
            filtered_files: Vec::new(),
            selected_index: 0,
            current_page: 0,
            filter_query: String::new(),
            workspace_root,
            filter_cache: std::collections::HashMap::new(),
            file_colorizer: FileColorizer::new(),
        }
    }

    /// Reset selection and filter (call when opening file browser)
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.selected_index = 0;
        self.current_page = 0;
        self.filter_query.clear();
        self.apply_filter(); // Refresh filtered_files to show all
    }

    /// Clean up resources to free memory (call when closing file browser)
    pub fn cleanup(&mut self) {
        self.filter_cache.clear();
        self.filtered_files.clear();
        self.filtered_files.shrink_to_fit();
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
        if self.filter_query.is_empty() {
            // Avoid cloning when no filter - just reference all files
            self.filtered_files = self.all_files.clone();
            return;
        }

        let query_lower = self.filter_query.to_lowercase();

        // Pre-allocate with estimated capacity
        let mut scored_files: Vec<(usize, FileEntry)> =
            Vec::with_capacity(self.all_files.len() / 2);

        // Use a reusable buffer for efficiency
        let mut buffer = Vec::new();

        for entry in &self.all_files {
            let path_lower = entry.relative_path.to_lowercase();

            // Try fuzzy match first, fall back to substring
            if let Some(fuzzy_score) =
                Self::simple_fuzzy_match_with_buffer(&path_lower, &query_lower, &mut buffer)
            {
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

    /// Simple fuzzy matching using nucleo-matcher
    /// Returns score if matched, None otherwise
    #[allow(dead_code)]
    fn simple_fuzzy_match(path: &str, query: &str) -> Option<usize> {
        let mut buffer = Vec::new();
        Self::simple_fuzzy_match_with_buffer(path, query, &mut buffer)
    }

    /// Simple fuzzy matching using nucleo-matcher with a reusable buffer
    /// Returns score if matched, None otherwise
    fn simple_fuzzy_match_with_buffer(
        path: &str,
        query: &str,
        buffer: &mut Vec<char>,
    ) -> Option<usize> {
        if query.is_empty() {
            return Some(1000); // Default score for empty query
        }

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let utf32_path = Utf32Str::new(path, buffer);
        let score = pattern.score(utf32_path, &mut matcher)?;

        // Convert nucleo score to our scoring system, with bonuses for filename matches
        let mut adjusted_score = score as usize;
        let query_lower = query.to_lowercase();

        // Bonus for matching in filename (last path segment)
        if let Some(filename) = path.rsplit('/').next()
            && filename.to_lowercase().contains(&query_lower)
        {
            adjusted_score += 500;
        }

        Some(adjusted_score)
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

    /// Get the appropriate style for a file entry based on its path and type
    #[allow(dead_code)]
    pub fn style_for_entry(&self, entry: &FileEntry) -> Option<anstyle::Style> {
        let path = Path::new(&entry.path);
        self.file_colorizer.style_for_path(path)
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

    // Check context: if @ is preceded by command-like text, skip it
    let is_npm_context = is_npm_command_context(input, start);

    let mut end = cursor;
    while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
        end += 1;
    }

    let reference = &input[start + 1..end];

    // Ensure the extracted reference looks like a file path, not a package specifier
    if !looks_like_file_path(reference, is_npm_context) {
        return None;
    }

    Some((start, end, reference.to_owned()))
}

/// Check if @ is used in npm command context (e.g., @scope/package)
fn is_npm_command_context(input: &str, at_pos: usize) -> bool {
    // Check if preceded by package manager commands: npm, npx, yarn, pnpm, bun
    let before_at = input[..at_pos].trim_end();
    let cmd_names = ["npm", "npx", "yarn", "pnpm", "bun"];

    // Check if any command appears in the command line
    for cmd in &cmd_names {
        // Look for the command at word boundaries
        // e.g., "npm install @scope/pkg" or "npm i @scope/pkg"
        let bytes = before_at.as_bytes();
        let cmd_bytes = cmd.as_bytes();

        // Check if it starts with the command
        if bytes.len() >= cmd_bytes.len() {
            // Check beginning
            if &bytes[..cmd_bytes.len()] == cmd_bytes {
                // Verify it's a word boundary (followed by space or end of string)
                if cmd_bytes.len() == bytes.len() || bytes[cmd_bytes.len()].is_ascii_whitespace() {
                    return true;
                }
            }
        }

        // Also check after spaces (in case of leading whitespace)
        if let Some(pos) = before_at.find(cmd) {
            // Check if preceded by space or start
            let is_word_start = pos == 0 || before_at.as_bytes()[pos - 1].is_ascii_whitespace();
            // Check if followed by space or end
            let is_word_end = pos + cmd.len() == before_at.len()
                || before_at.as_bytes()[pos + cmd.len()].is_ascii_whitespace();

            if is_word_start && is_word_end {
                return true;
            }
        }
    }

    false
}

/// Check if the reference looks like a file path vs package specifier
/// `is_npm_context`: whether @ appears in npm command context (affects bare identifier handling)
fn looks_like_file_path(reference: &str, is_npm_context: bool) -> bool {
    // Allow empty (bare @) to show file picker with all files
    if reference.is_empty() {
        return true;
    }

    // Reject anything that contains @ (scoped packages or version specs like @scope/pkg@1.0.0)
    if reference.contains('@') {
        return false;
    }

    let has_separator = reference.contains('/') || reference.contains('\\');
    let has_extension = reference.contains('.');

    // Relative paths with dot prefix: ./path, ../path
    if reference.starts_with("./") || reference.starts_with("../") {
        return true;
    }

    // Absolute paths: /path, ~/path
    if reference.starts_with('/') || reference.starts_with("~/") {
        return true;
    }

    // Windows absolute paths: C:\path, C:/path
    if reference.len() > 2 && reference.as_bytes()[1] == b':' {
        let sep = reference.as_bytes()[2];
        if sep == b'\\' || sep == b'/' {
            return true;
        }
    }

    // Paths with separators AND extensions: src/main.rs, foo/bar/file.ts
    // This distinguishes from packages like @scope/package (no extension)
    if has_separator && has_extension {
        return true;
    }

    // Simple filename with extension: main.rs, index.ts, image.png
    if !has_separator && has_extension {
        return true;
    }

    // In npm command context, reject bare identifiers (likely package names)
    // e.g., "npm i @types" where "types" is a package scope
    if is_npm_context {
        return false;
    }

    // In normal conversation context, allow bare identifiers for file picker
    // e.g., "choose @files" or "edit @config"
    if !has_separator && !has_extension {
        return true;
    }

    false
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
        assert_eq!(result, Some((0, 12, "src/main.rs".to_owned())));
    }

    #[test]
    fn test_extract_file_reference_mid_word() {
        let input = "@src/main.rs";
        let result = extract_file_reference(input, 5);
        assert_eq!(result, Some((0, 12, "src/main.rs".to_owned())));
    }

    #[test]
    fn test_extract_file_reference_with_text_before() {
        let input = "check @src/main.rs for errors";
        let result = extract_file_reference(input, 18);
        assert_eq!(result, Some((6, 18, "src/main.rs".to_owned())));
    }

    #[test]
    fn test_no_file_reference() {
        let input = "no reference here";
        let result = extract_file_reference(input, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_npm_scoped_package_no_trigger() {
        // Should NOT trigger file picker on npm scoped packages
        let input = "npx -y @openai/codex@latest";
        let result = extract_file_reference(input, 17); // cursor at @openai
        assert_eq!(result, None);
    }

    #[test]
    fn test_npm_scoped_package_variant() {
        // npm install @scope/package@version
        let input = "npm install @scope/package@1.0.0";
        let result = extract_file_reference(input, 22); // cursor at @scope
        assert_eq!(result, None);
    }

    #[test]
    fn test_npm_install_scoped() {
        // npm i @types/node
        let input = "npm i @types/node";
        let result = extract_file_reference(input, 11); // cursor after @
        assert_eq!(result, None);
    }

    #[test]
    fn test_yarn_scoped_package() {
        // yarn add @babel/core
        let input = "yarn add @babel/core";
        let result = extract_file_reference(input, 14); // cursor at @babel
        assert_eq!(result, None);
    }

    #[test]
    fn test_pnpm_scoped_package() {
        // pnpm install @vitejs/plugin-vue
        let input = "pnpm install @vitejs/plugin-vue";
        let result = extract_file_reference(input, 23); // cursor at @vitejs
        assert_eq!(result, None);
    }

    #[test]
    fn test_valid_file_path_with_at() {
        // Valid file path like @src/main.rs should work
        let input = "@src/main.rs";
        let result = extract_file_reference(input, 12);
        assert_eq!(result, Some((0, 12, "src/main.rs".to_owned())));
    }

    #[test]
    fn test_valid_at_path_in_text() {
        // @./relative/path should work
        let input = "check @./src/components/Button.tsx";
        let result = extract_file_reference(input, 34);
        assert_eq!(
            result,
            Some((6, 34, "./src/components/Button.tsx".to_owned()))
        );
    }

    #[test]
    fn test_absolute_at_path() {
        // @/absolute/path should work
        let input = "see @/etc/config.txt";
        let result = extract_file_reference(input, 20);
        assert_eq!(result, Some((4, 20, "/etc/config.txt".to_owned())));
    }

    #[test]
    fn test_bare_identifier_in_conversation() {
        // In normal conversation, @files should trigger picker
        let input = "choose @files and do something";
        let result = extract_file_reference(input, 13); // cursor after "files"
        assert_eq!(result, Some((7, 13, "files".to_owned())));
    }

    #[test]
    fn test_bare_identifier_config() {
        // @config in conversation context should work
        let input = "edit @config";
        let result = extract_file_reference(input, 12); // cursor at end
        assert_eq!(result, Some((5, 12, "config".to_owned())));
    }

    #[test]
    fn test_bare_identifier_rejected_in_npm() {
        // But in npm context, bare identifier is rejected
        let input = "npm i @types";
        let result = extract_file_reference(input, 12); // cursor at end
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
            "src/main.rs".to_owned(),
            "src/lib.rs".to_owned(),
            "tests/test.rs".to_owned(),
            "README.md".to_owned(),
        ];
        palette.load_files(files);

        assert_eq!(palette.total_items(), 4);

        palette.set_filter("src".to_owned());
        assert_eq!(palette.total_items(), 2);

        palette.set_filter("main".to_owned());
        assert_eq!(palette.total_items(), 1);
    }

    #[test]
    fn test_smart_ranking() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        let files = vec![
            "src/main.rs".to_owned(),
            "src/domain/main_handler.rs".to_owned(),
            "tests/main_test.rs".to_owned(),
            "main.rs".to_owned(),
        ];
        palette.load_files(files);

        palette.set_filter("main".to_owned());

        // Exact filename match should rank highest
        let items = palette.current_page_items();
        assert_eq!(items[0].1.relative_path, "main.rs");
    }

    #[test]
    fn test_has_files() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        assert!(!palette.has_files());

        palette.load_files(vec!["file.rs".to_owned()]);
        assert!(palette.has_files());
    }

    #[test]
    fn test_circular_navigation() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));
        let files = vec!["a.rs".to_owned(), "b.rs".to_owned(), "c.rs".to_owned()];
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
            "/workspace/src/main.rs".to_owned(),
            "/workspace/.env".to_owned(),            // MUST be excluded
            "/workspace/.env.local".to_owned(),      // MUST be excluded
            "/workspace/.env.production".to_owned(), // MUST be excluded
            "/workspace/.git/config".to_owned(),     // MUST be excluded
            "/workspace/.gitignore".to_owned(),      // MUST be excluded
            "/workspace/.DS_Store".to_owned(),       // MUST be excluded
            "/workspace/.hidden_file".to_owned(),    // MUST be excluded (hidden)
            "/workspace/tests/test.rs".to_owned(),
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
                path: "/workspace/zebra.txt".to_owned(),
                display_name: "zebra.txt".to_owned(),
                relative_path: "zebra.txt".to_owned(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/src".to_owned(),
                display_name: "src/".to_owned(),
                relative_path: "src".to_owned(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/Apple.txt".to_owned(),
                display_name: "Apple.txt".to_owned(),
                relative_path: "Apple.txt".to_owned(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/tests".to_owned(),
                display_name: "tests/".to_owned(),
                relative_path: "tests".to_owned(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/banana.txt".to_owned(),
                display_name: "banana.txt".to_owned(),
                relative_path: "banana.txt".to_owned(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/lib".to_owned(),
                display_name: "lib/".to_owned(),
                relative_path: "lib".to_owned(),
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
    fn test_simple_fuzzy_match() {
        // Test basic fuzzy matching
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "smr").is_some());
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "src").is_some());
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "main").is_some());

        // Test non-matches
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "xyz").is_none());
        assert!(FilePalette::simple_fuzzy_match("src/main.rs", "msr").is_none());
        // Wrong order
    }

    #[test]
    fn test_filtering_maintains_directory_priority() {
        let mut palette = FilePalette::new(PathBuf::from("/workspace"));

        // Create test entries with both directories and files
        palette.all_files = vec![
            FileEntry {
                path: "/workspace/src".to_owned(),
                display_name: "src/".to_owned(),
                relative_path: "src".to_owned(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/src_file.rs".to_owned(),
                display_name: "src_file.rs".to_owned(),
                relative_path: "src_file.rs".to_owned(),
                is_dir: false,
            },
            FileEntry {
                path: "/workspace/tests".to_owned(),
                display_name: "tests/".to_owned(),
                relative_path: "tests".to_owned(),
                is_dir: true,
            },
            FileEntry {
                path: "/workspace/source.txt".to_owned(),
                display_name: "source.txt".to_owned(),
                relative_path: "source.txt".to_owned(),
                is_dir: false,
            },
        ];
        palette.filtered_files = palette.all_files.clone();

        // Filter for "src" - should match directories and files
        palette.set_filter("src".to_owned());

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

/// Implement PaletteItem trait for FileEntry to support generic PaletteRenderer
impl super::palette_renderer::PaletteItem for FileEntry {
    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn display_icon(&self) -> Option<String> {
        if self.is_dir {
            Some("↳  ".to_owned())
        } else {
            Some("  · ".to_owned())
        }
    }

    fn is_directory(&self) -> bool {
        self.is_dir
    }
}
