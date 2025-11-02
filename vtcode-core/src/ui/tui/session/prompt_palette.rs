use crate::prompts::CustomPrompt;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::path::Path;

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone)]
pub struct PromptEntry {
    pub name: String,
    pub description: String,
}

pub struct PromptPalette {
    all_prompts: Vec<PromptEntry>,
    filtered_prompts: Vec<PromptEntry>,
    selected_index: usize,
    current_page: usize,
    filter_query: String,
    filter_cache: std::collections::HashMap<String, Vec<PromptEntry>>,
}

impl PromptPalette {
    pub fn new() -> Self {
        Self {
            all_prompts: Vec::new(),
            filtered_prompts: Vec::new(),
            selected_index: 0,
            current_page: 0,
            filter_query: String::new(),
            filter_cache: std::collections::HashMap::new(),
        }
    }

    /// Reset selection and filter (call when opening prompt browser)
    pub fn reset(&mut self) {
        self.selected_index = 0;
        self.current_page = 0;
        self.filter_query.clear();
        self.apply_filter(); // Refresh filtered_prompts to show all
    }

    /// Clean up resources to free memory (call when closing prompt browser)
    pub fn cleanup(&mut self) {
        self.filter_cache.clear();
        self.filtered_prompts.clear();
        self.filtered_prompts.shrink_to_fit();
    }

    pub fn load_prompts<'a>(&mut self, prompts: impl Iterator<Item = &'a CustomPrompt>) {
        self.all_prompts.clear();
        self.filtered_prompts.clear();
        self.filter_cache.clear();
        self.selected_index = 0;
        self.current_page = 0;

        self.append_custom_prompts(prompts);

        if self.all_prompts.is_empty() {
            self.apply_filter();
        }
    }

    pub fn append_custom_prompts<'a>(&mut self, prompts: impl Iterator<Item = &'a CustomPrompt>) {
        let entries = prompts.map(|prompt| PromptEntry {
            name: prompt.name.clone(),
            description: prompt.description.clone().unwrap_or_default(),
        });
        self.append_entries(entries);
    }

    pub fn append_entries<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = PromptEntry>,
    {
        let mut added = false;

        for entry in entries {
            added |= self.insert_entry(entry);
        }

        if added {
            self.finalize_entries();
        }
    }

    /// Load prompts directly from filesystem (fallback if CustomPromptRegistry not available)
    pub fn load_from_directory(&mut self, prompts_dir: &Path) {
        if !prompts_dir.exists() || !prompts_dir.is_dir() {
            return;
        }

        let mut entries = Vec::new();

        if let Ok(dir_entries) = std::fs::read_dir(prompts_dir) {
            for entry in dir_entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }

                if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                    entries.push(PromptEntry {
                        name: name.to_string(),
                        description: String::new(), // No description from direct load
                    });
                }
            }
        }

        self.append_entries(entries);
    }

    fn insert_entry(&mut self, entry: PromptEntry) -> bool {
        if entry.name.trim().is_empty() {
            return false;
        }

        if self
            .all_prompts
            .iter()
            .any(|existing| existing.name.eq_ignore_ascii_case(&entry.name))
        {
            return false;
        }

        self.all_prompts.push(entry);
        true
    }

    fn finalize_entries(&mut self) {
        self.all_prompts
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.filter_cache.clear();
        self.apply_filter();

        if self.filtered_prompts.is_empty() {
            self.selected_index = 0;
            self.current_page = 0;
        } else {
            self.selected_index = self
                .selected_index
                .min(self.filtered_prompts.len().saturating_sub(1));
            let last_page = self.total_pages().saturating_sub(1);
            self.current_page = self.current_page.min(last_page);
        }
    }

    pub fn set_filter(&mut self, query: String) {
        self.filter_query = query.clone();

        // Check cache first
        if let Some(cached) = self.filter_cache.get(&query) {
            self.filtered_prompts = cached.clone();
        } else {
            self.apply_filter();
            // Cache the result
            if !query.is_empty() && self.filter_cache.len() < 50 {
                self.filter_cache
                    .insert(query, self.filtered_prompts.clone());
            }
        }

        self.selected_index = 0;
        self.current_page = 0;
    }

    fn apply_filter(&mut self) {
        if self.filter_query.is_empty() {
            self.filtered_prompts = self.all_prompts.clone();
            return;
        }

        let query_lower = self.filter_query.to_lowercase();

        // Pre-allocate with estimated capacity
        let mut scored_prompts: Vec<(usize, PromptEntry)> =
            Vec::with_capacity(self.all_prompts.len() / 2);

        // Use a reusable buffer for efficiency
        let mut buffer = Vec::new();

        for entry in &self.all_prompts {
            let name_lower = entry.name.to_lowercase();
            let desc_lower = entry.description.to_lowercase();

            // Try fuzzy match first, fall back to substring
            if let Some(fuzzy_score) =
                Self::simple_fuzzy_match_with_buffer(&name_lower, &query_lower, &mut buffer)
            {
                scored_prompts.push((fuzzy_score, entry.clone()));
            } else if name_lower.contains(&query_lower) || desc_lower.contains(&query_lower) {
                let score = Self::calculate_match_score(&name_lower, &desc_lower, &query_lower);
                scored_prompts.push((score, entry.clone()));
            }
        }

        // Sort by score (descending), then alphabetically
        scored_prompts.sort_unstable_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
        });

        self.filtered_prompts = scored_prompts.into_iter().map(|(_, entry)| entry).collect();
    }

    /// Simple fuzzy matching using nucleo-matcher
    /// Returns score if matched, None otherwise
    #[allow(dead_code)]
    fn simple_fuzzy_match(text: &str, query: &str) -> Option<usize> {
        let mut buffer = Vec::new();
        Self::simple_fuzzy_match_with_buffer(text, query, &mut buffer)
    }

    /// Simple fuzzy matching using nucleo-matcher with a reusable buffer
    /// Returns score if matched, None otherwise
    fn simple_fuzzy_match_with_buffer(
        text: &str,
        query: &str,
        buffer: &mut Vec<char>,
    ) -> Option<usize> {
        if query.is_empty() {
            return Some(1000); // Default score for empty query
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let utf32_text = Utf32Str::new(text, buffer);
        let score = pattern.score(utf32_text, &mut matcher)?;

        Some(score as usize)
    }

    fn calculate_match_score(name: &str, description: &str, query: &str) -> usize {
        let mut score: usize = 0;

        // Exact match gets highest priority
        if name == query {
            return 10000;
        }

        // Name starts with query
        if name.starts_with(query) {
            score += 2000;
        }

        // Name contains query
        if name.contains(query) {
            score += 500;
        }

        // Description contains query
        if description.contains(query) {
            score += 100;
        }

        // Multiple occurrences bonus
        let matches = name.matches(query).count() + description.matches(query).count();
        score += matches * 10;

        score
    }

    pub fn move_selection_up(&mut self) {
        if self.filtered_prompts.is_empty() {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = self.filtered_prompts.len().saturating_sub(1);
        }
        self.update_page_from_selection();
    }

    pub fn move_selection_down(&mut self) {
        if self.filtered_prompts.is_empty() {
            return;
        }
        if self.selected_index + 1 < self.filtered_prompts.len() {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
        self.update_page_from_selection();
    }

    pub fn move_to_first(&mut self) {
        if !self.filtered_prompts.is_empty() {
            self.selected_index = 0;
            self.current_page = 0;
        }
    }

    pub fn move_to_last(&mut self) {
        if !self.filtered_prompts.is_empty() {
            self.selected_index = self.filtered_prompts.len().saturating_sub(1);
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

    pub fn get_selected(&self) -> Option<&PromptEntry> {
        self.filtered_prompts.get(self.selected_index)
    }

    pub fn current_page_items(&self) -> Vec<(usize, &PromptEntry, bool)> {
        let start = self.current_page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.filtered_prompts.len());

        self.filtered_prompts[start..end]
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
        if self.filtered_prompts.is_empty() {
            1
        } else {
            (self.filtered_prompts.len() + PAGE_SIZE - 1) / PAGE_SIZE
        }
    }

    pub fn current_page_number(&self) -> usize {
        self.current_page + 1
    }

    pub fn total_items(&self) -> usize {
        self.filtered_prompts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.filtered_prompts.is_empty()
    }

    pub fn filter_query(&self) -> &str {
        &self.filter_query
    }

    pub fn has_prompts(&self) -> bool {
        !self.all_prompts.is_empty()
    }

    pub fn has_more_items(&self) -> bool {
        let end = ((self.current_page + 1) * PAGE_SIZE).min(self.filtered_prompts.len());
        end < self.filtered_prompts.len()
    }
}

pub fn extract_prompt_reference(input: &str, cursor: usize) -> Option<(usize, usize, String)> {
    if cursor == 0 || cursor > input.len() {
        return None;
    }

    let bytes = input.as_bytes();
    let mut start = cursor;

    while start > 0 && bytes[start - 1] != b'#' && !bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }

    if start == 0 || bytes[start - 1] != b'#' {
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
    fn test_extract_prompt_reference_at_symbol() {
        let input = "#";
        let result = extract_prompt_reference(input, 1);
        assert_eq!(result, Some((0, 1, String::new())));
    }

    #[test]
    fn test_extract_prompt_reference_with_name() {
        let input = "#vtcode";
        let result = extract_prompt_reference(input, 7);
        assert_eq!(result, Some((0, 7, "vtcode".to_string())));
    }

    #[test]
    fn test_extract_prompt_reference_mid_word() {
        let input = "#vtcode";
        let result = extract_prompt_reference(input, 4);
        assert_eq!(result, Some((0, 7, "vtcode".to_string())));
    }

    #[test]
    fn test_extract_prompt_reference_with_text_before() {
        let input = "use #vtcode for help";
        let result = extract_prompt_reference(input, 11);
        assert_eq!(result, Some((4, 11, "vtcode".to_string())));
    }

    #[test]
    fn test_no_prompt_reference() {
        let input = "no reference here";
        let result = extract_prompt_reference(input, 5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_pagination() {
        let mut palette = PromptPalette::new();

        // Create 50 test entries directly
        palette.all_prompts = (0..50)
            .map(|i| PromptEntry {
                name: format!("prompt{}", i),
                description: format!("Description {}", i),
            })
            .collect();
        palette.filtered_prompts = palette.all_prompts.clone();

        // With PAGE_SIZE=20, 50 prompts = 3 pages (20 + 20 + 10)
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
        let mut palette = PromptPalette::new();
        palette.all_prompts = vec![
            PromptEntry {
                name: "vtcode".to_string(),
                description: "VTCode helper".to_string(),
            },
            PromptEntry {
                name: "rust".to_string(),
                description: "Rust coding assistant".to_string(),
            },
            PromptEntry {
                name: "test".to_string(),
                description: "Test helper".to_string(),
            },
        ];
        palette.filtered_prompts = palette.all_prompts.clone();

        assert_eq!(palette.total_items(), 3);

        palette.set_filter("rust".to_string());
        assert_eq!(palette.total_items(), 1);

        palette.set_filter("vt".to_string());
        assert_eq!(palette.total_items(), 1);
    }

    #[test]
    fn test_circular_navigation() {
        let mut palette = PromptPalette::new();
        palette.all_prompts = vec![
            PromptEntry {
                name: "a".to_string(),
                description: String::new(),
            },
            PromptEntry {
                name: "b".to_string(),
                description: String::new(),
            },
            PromptEntry {
                name: "c".to_string(),
                description: String::new(),
            },
        ];
        palette.filtered_prompts = palette.all_prompts.clone();

        // At first item, up should wrap to last
        assert_eq!(palette.selected_index, 0);
        palette.move_selection_up();
        assert_eq!(palette.selected_index, 2);

        // At last item, down should wrap to first
        palette.move_selection_down();
        assert_eq!(palette.selected_index, 0);
    }

    #[test]
    fn test_alphabetical_sorting() {
        let mut palette = PromptPalette::new();
        let test_entries = vec![
            PromptEntry {
                name: "zebra".to_string(),
                description: String::new(),
            },
            PromptEntry {
                name: "apple".to_string(),
                description: String::new(),
            },
            PromptEntry {
                name: "Banana".to_string(),
                description: String::new(),
            },
        ];

        palette.all_prompts = test_entries;
        palette
            .all_prompts
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        palette.filtered_prompts = palette.all_prompts.clone();

        let items = palette.current_page_items();
        assert_eq!(items[0].1.name, "apple");
        assert_eq!(items[1].1.name, "Banana");
        assert_eq!(items[2].1.name, "zebra");
    }
}
