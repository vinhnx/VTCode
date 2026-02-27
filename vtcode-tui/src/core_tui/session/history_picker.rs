/// History Picker - Fuzzy search for command history (Ctrl+R)
///
/// Provides a visual palette for searching and selecting from command history
/// using nucleo fuzzy matching, similar to the slash command palette.
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::llm::provider::ContentPart;
use crate::ui::search::fuzzy_score;

use super::input_manager::InputManager;

/// A single history entry with fuzzy match score
#[derive(Debug, Clone)]
pub struct HistoryMatch {
    /// Index in the original history
    pub history_index: usize,
    /// The command text
    pub content: String,
    /// Fuzzy match score (higher is better)
    pub score: u32,
    /// Associated attachments
    pub attachments: Vec<ContentPart>,
}

/// State for the history picker overlay
#[derive(Debug)]
pub struct HistoryPickerState {
    /// Whether the picker is currently active
    pub active: bool,
    /// Current search/filter query
    pub search_query: String,
    /// Filtered and sorted matches
    pub matches: Vec<HistoryMatch>,
    /// List state for selection tracking
    pub list_state: ListState,
    /// Number of visible rows in the picker
    pub visible_rows: usize,
    /// Original content before picker was opened (for cancel restoration)
    original_content: String,
    /// Original cursor position before picker was opened
    original_cursor: usize,
    /// Original attachments before picker was opened
    original_attachments: Vec<ContentPart>,
}

impl Default for HistoryPickerState {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryPickerState {
    /// Create a new history picker state
    pub fn new() -> Self {
        Self {
            active: false,
            search_query: String::new(),
            matches: Vec::new(),
            list_state: ListState::default(),
            visible_rows: 10,
            original_content: String::new(),
            original_cursor: 0,
            original_attachments: Vec::new(),
        }
    }

    /// Open the history picker
    pub fn open(&mut self, input_manager: &InputManager) {
        self.active = true;
        self.search_query.clear();
        self.original_content = input_manager.content().to_string();
        self.original_cursor = input_manager.cursor();
        self.original_attachments = input_manager.attachments().to_vec();
        self.list_state.select(Some(0));
    }

    /// Close the picker and restore original input
    pub fn cancel(&mut self, input_manager: &mut InputManager) {
        self.active = false;
        self.search_query.clear();
        self.matches.clear();
        input_manager.set_content(self.original_content.clone());
        input_manager.set_cursor(self.original_cursor);
        input_manager.set_attachments(self.original_attachments.clone());
    }

    /// Accept the current selection and close the picker
    pub fn accept(&mut self, input_manager: &mut InputManager) {
        if let Some(selected) = self.selected_match() {
            input_manager.set_content(selected.content.clone());
            input_manager.set_attachments(selected.attachments.clone());
        }
        self.active = false;
        self.search_query.clear();
        self.matches.clear();
    }

    /// Get the currently selected match
    pub fn selected_match(&self) -> Option<&HistoryMatch> {
        self.list_state
            .selected()
            .and_then(|idx| self.matches.get(idx))
    }

    /// Update the search query and filter matches
    pub fn update_search(&mut self, history: &[(String, Vec<ContentPart>)]) {
        self.matches.clear();

        // Score and collect all matching entries
        let query = self.search_query.to_lowercase();
        for (idx, (content, attachments)) in history.iter().enumerate().rev() {
            // Skip empty entries
            if content.trim().is_empty() {
                continue;
            }

            // Calculate fuzzy score or use substring match as fallback
            let score = if query.is_empty() {
                // No query - include all with recency score
                Some((history.len() - idx) as u32)
            } else {
                fuzzy_score(&query, content)
            };

            if let Some(score) = score {
                self.matches.push(HistoryMatch {
                    history_index: idx,
                    content: content.clone(),
                    score,
                    attachments: attachments.clone(),
                });
            }
        }

        // Sort by score (descending) - higher scores first
        self.matches.sort_by(|a, b| b.score.cmp(&a.score));

        // Deduplicate by content (keep highest scored entry for each unique command)
        let mut seen = std::collections::HashSet::new();
        self.matches.retain(|m| seen.insert(m.content.clone()));

        // Limit to reasonable number
        self.matches.truncate(100);

        // Reset selection to first item if available
        if self.matches.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    /// Add a character to the search query
    pub fn add_char(&mut self, ch: char, history: &[(String, Vec<ContentPart>)]) {
        self.search_query.push(ch);
        self.update_search(history);
    }

    /// Remove the last character from the search query
    pub fn backspace(&mut self, history: &[(String, Vec<ContentPart>)]) {
        self.search_query.pop();
        self.update_search(history);
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if self.matches.is_empty() {
            return;
        }

        let current = self.list_state.selected().unwrap_or(0);
        let new_index = if current == 0 {
            self.matches.len() - 1
        } else {
            current - 1
        };
        self.list_state.select(Some(new_index));
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        if self.matches.is_empty() {
            return;
        }

        let current = self.list_state.selected().unwrap_or(0);
        let new_index = (current + 1) % self.matches.len();
        self.list_state.select(Some(new_index));
    }

    /// Check if the picker is empty (no matches)
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Get number of matches
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
}

/// Handle keyboard input for the history picker
/// Returns true if the key was handled
pub fn handle_history_picker_key(
    key: &KeyEvent,
    picker: &mut HistoryPickerState,
    input_manager: &mut InputManager,
    history: &[(String, Vec<ContentPart>)],
) -> bool {
    if !picker.active {
        return false;
    }

    match key.code {
        KeyCode::Esc => {
            picker.cancel(input_manager);
            true
        }
        KeyCode::Enter => {
            picker.accept(input_manager);
            true
        }
        // Plain Up/Down arrows for navigation
        KeyCode::Up => {
            picker.move_up();
            true
        }
        KeyCode::Down => {
            picker.move_down();
            true
        }
        // Ctrl+K/J for vim-style navigation
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            picker.move_up();
            true
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            picker.move_down();
            true
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            picker.move_up();
            true
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            picker.move_down();
            true
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+R while in picker - cycle through matches
            picker.move_down();
            true
        }
        KeyCode::Tab => {
            // Tab cycles forward through matches
            picker.move_down();
            true
        }
        KeyCode::BackTab => {
            // Shift+Tab cycles backward through matches
            picker.move_up();
            true
        }
        KeyCode::Char(ch) => {
            picker.add_char(ch, history);
            true
        }
        KeyCode::Backspace => {
            picker.backspace(history);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_history() -> Vec<(String, Vec<ContentPart>)> {
        vec![
            ("cargo build".to_string(), vec![]),
            ("cargo test".to_string(), vec![]),
            ("git status".to_string(), vec![]),
            ("cargo clippy".to_string(), vec![]),
            ("git diff".to_string(), vec![]),
        ]
    }

    #[test]
    fn test_open_picker() {
        let mut picker = HistoryPickerState::new();
        let manager = InputManager::new();

        assert!(!picker.active);
        picker.open(&manager);
        assert!(picker.active);
    }

    #[test]
    fn test_filter_matches() {
        let mut picker = HistoryPickerState::new();
        let manager = InputManager::new();
        let history = make_history();

        picker.open(&manager);
        picker.update_search(&history);

        // All entries should match with empty query
        assert_eq!(picker.match_count(), 5);

        // Filter to "cargo"
        picker.search_query = "cargo".to_string();
        picker.update_search(&history);
        assert_eq!(picker.match_count(), 3);

        // Filter to "git"
        picker.search_query = "git".to_string();
        picker.update_search(&history);
        assert_eq!(picker.match_count(), 2);
    }

    #[test]
    fn test_navigation() {
        let mut picker = HistoryPickerState::new();
        let manager = InputManager::new();
        let history = make_history();

        picker.open(&manager);
        picker.update_search(&history);

        assert_eq!(picker.list_state.selected(), Some(0));

        picker.move_down();
        assert_eq!(picker.list_state.selected(), Some(1));

        picker.move_up();
        assert_eq!(picker.list_state.selected(), Some(0));

        // Wrap around
        picker.move_up();
        assert_eq!(picker.list_state.selected(), Some(4));
    }

    #[test]
    fn test_accept_selection() {
        let mut picker = HistoryPickerState::new();
        let mut manager = InputManager::new();
        let history = make_history();

        picker.open(&manager);
        picker.update_search(&history);
        picker.move_down(); // Select second item

        let selected_content = picker.selected_match().map(|m| m.content.clone());
        picker.accept(&mut manager);

        assert!(!picker.active);
        assert_eq!(Some(manager.content().to_string()), selected_content);
    }

    #[test]
    fn test_cancel_restores_original() {
        let mut picker = HistoryPickerState::new();
        let mut manager = InputManager::new();
        manager.set_content("original content".to_string());
        let history = make_history();

        picker.open(&manager);
        picker.update_search(&history);
        picker.cancel(&mut manager);

        assert!(!picker.active);
        assert_eq!(manager.content(), "original content");
    }

    #[test]
    fn test_deduplication() {
        let mut picker = HistoryPickerState::new();
        let manager = InputManager::new();
        let history = vec![
            ("cargo build".to_string(), vec![]),
            ("cargo test".to_string(), vec![]),
            ("cargo build".to_string(), vec![]), // Duplicate
            ("cargo build".to_string(), vec![]), // Another duplicate
        ];

        picker.open(&manager);
        picker.update_search(&history);

        // Should deduplicate to 2 unique entries
        assert_eq!(picker.match_count(), 2);
    }
}
