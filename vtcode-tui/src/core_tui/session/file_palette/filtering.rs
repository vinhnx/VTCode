use std::path::Path;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::{FileEntry, FilePalette};

impl FilePalette {
    /// SECURITY: Check if a file should be excluded from the file browser
    /// Always excludes .env files, .git directories, and other sensitive data
    pub(super) fn should_exclude_file(path: &Path) -> bool {
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

    pub(super) fn apply_filter(&mut self) {
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
    pub(super) fn simple_fuzzy_match(path: &str, query: &str) -> Option<usize> {
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

    /// Get the appropriate style for a file entry based on its path and type
    #[allow(dead_code)]
    pub fn style_for_entry(&self, entry: &FileEntry) -> Option<anstyle::Style> {
        let path = Path::new(&entry.path);
        self.file_colorizer.style_for_path(path)
    }
}
