use super::super::types::ContentPart;
use crate::tui::core_tui::session::list_navigator::ListNavigator;
use crate::tui::ui::search::fuzzy_score;
/// History Picker - Fuzzy search for command history (Ctrl+R)
///
/// Provides a visual palette for searching and selecting from command history
/// using nucleo fuzzy matching, similar to the slash command palette.
use chrono::{DateTime, Duration, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::core_tui::session::input_manager::InputManager;

/// A prompt from a previous session archive.
#[derive(Debug, Clone)]
pub struct ArchivedPrompt {
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub session_label: String,
}

/// A single history entry with fuzzy match score
#[derive(Debug, Clone)]
pub struct HistoryMatch {
    /// Index in the original in-memory history. `None` for archived entries
    /// that come from a previous session rather than the current one.
    pub history_index: Option<usize>,
    /// The command text
    pub content: String,
    /// Fuzzy match score (higher is better)
    pub score: u32,
    /// Associated attachments
    pub attachments: Vec<ContentPart>,
    /// When this prompt was submitted (None for legacy entries without timestamps)
    pub created_at: Option<DateTime<Utc>>,
    /// Short relative time label (e.g. "3h ago", "just now")
    pub time_label: String,
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
    /// Shared list navigation state
    pub(crate) navigator: ListNavigator,
    /// Original content before picker was opened (for cancel restoration)
    original_content: String,
    /// Original cursor position before picker was opened
    original_cursor: usize,
    /// Original attachments before picker was opened
    original_attachments: Vec<ContentPart>,
    /// Prompts loaded from archived sessions (injected externally)
    archived_prompts: Vec<ArchivedPrompt>,
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
            navigator: ListNavigator::new(),
            original_content: String::new(),
            original_cursor: 0,
            original_attachments: Vec::new(),
            archived_prompts: Vec::new(),
        }
    }

    /// Set archived prompts loaded from previous sessions.
    pub fn set_archived_prompts(&mut self, entries: Vec<ArchivedPrompt>) {
        self.archived_prompts = entries;
    }

    /// Open the history picker
    pub fn open(&mut self, input_manager: &InputManager) {
        self.active = true;
        self.search_query.clear();
        self.original_content = input_manager.content().to_string();
        self.original_cursor = input_manager.cursor();
        self.original_attachments = input_manager.attachments().to_vec();
        self.navigator.select_first();
    }

    /// Close the picker and restore original input
    pub fn cancel(&mut self, input_manager: &mut InputManager) {
        self.active = false;
        self.search_query.clear();
        self.matches.clear();
        self.navigator.set_item_count(0);
        input_manager.set_content(std::mem::take(&mut self.original_content));
        input_manager.set_cursor(self.original_cursor);
        input_manager.set_attachments(std::mem::take(&mut self.original_attachments));
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
        self.navigator.set_item_count(0);
    }

    /// Get the currently selected match
    pub fn selected_match(&self) -> Option<&HistoryMatch> {
        self.navigator
            .selected()
            .and_then(|index| self.matches.get(index))
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.navigator.selected()
    }

    pub fn scroll_offset(&self) -> usize {
        self.navigator.scroll_offset()
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        self.navigator.select_index(index)
    }

    /// Update the search query and filter matches.
    ///
    /// Merges in-memory session history with archived prompts from previous
    /// sessions.  Archived entries are included so the user can search across
    /// all recent prompts, not just the current conversation.
    pub fn update_search(&mut self, history: &[(String, Vec<ContentPart>, DateTime<Utc>)]) {
        self.matches.clear();
        let now = Utc::now();

        // Score and collect in-memory history entries
        let query = self.search_query.to_lowercase();
        for (idx, (content, attachments, created_at)) in history.iter().enumerate().rev() {
            if content.trim().is_empty() {
                continue;
            }

            let score = if query.is_empty() {
                Some(history.len().saturating_sub(idx) as u32)
            } else {
                fuzzy_score(&query, content)
            };

            if let Some(score) = score {
                let time_label = format_time_ago(now - *created_at);
                self.matches.push(HistoryMatch {
                    history_index: Some(idx),
                    content: content.clone(),
                    score,
                    attachments: attachments.clone(),
                    created_at: Some(*created_at),
                    time_label,
                });
            }
        }

        // Merge archived prompts from previous sessions
        for archived in &self.archived_prompts {
            if archived.content.trim().is_empty() {
                continue;
            }

            let score = if query.is_empty() {
                // Recency-based score: archived prompts get lower base score
                // than current-session entries so they sort below them.
                let age_hours = (now - archived.created_at).num_hours().max(0) as u32;
                1000u32.saturating_sub(age_hours)
            } else {
                match fuzzy_score(&query, &archived.content) {
                    Some(s) => s,
                    None => continue,
                }
            };

            let time_label = format_time_ago(now - archived.created_at);
            self.matches.push(HistoryMatch {
                history_index: None,
                content: archived.content.clone(),
                score,
                attachments: Vec::new(),
                created_at: Some(archived.created_at),
                time_label,
            });
        }

        // Sort by score (descending) - higher scores first
        self.matches
            .sort_by_key(|history_match| std::cmp::Reverse(history_match.score));

        // Deduplicate by content (keep highest scored entry)
        let mut seen = hashbrown::HashSet::new();
        self.matches.retain(|m| seen.insert(m.content.clone()));

        // Limit to reasonable number
        self.matches.truncate(200);

        self.navigator.set_item_count(self.matches.len());
        if self.matches.is_empty() {
            self.navigator.set_selected(None);
        } else {
            self.navigator.select_first();
        }
    }

    /// Add a character to the search query
    pub fn add_char(&mut self, ch: char, history: &[(String, Vec<ContentPart>, DateTime<Utc>)]) {
        self.search_query.push(ch);
        self.update_search(history);
    }

    /// Remove the last character from the search query
    pub fn backspace(&mut self, history: &[(String, Vec<ContentPart>, DateTime<Utc>)]) {
        self.search_query.pop();
        self.update_search(history);
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        self.navigator.move_up();
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        self.navigator.move_down();
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
    history: &[(String, Vec<ContentPart>, DateTime<Utc>)],
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

/// Format a `chrono::Duration` as a compact human-readable relative time label.
///
/// Returns strings like `"just now"`, `"5m ago"`, `"3h ago"`, `"2d ago"`.
pub fn format_time_ago(elapsed: Duration) -> String {
    if elapsed < Duration::zero() {
        return "just now".to_string();
    }
    let seconds = elapsed.num_seconds();
    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_history() -> Vec<(String, Vec<ContentPart>, DateTime<Utc>)> {
        let now = Utc::now();
        vec![
            (
                "cargo build".to_string(),
                vec![],
                now - Duration::minutes(4),
            ),
            ("cargo test".to_string(), vec![], now - Duration::minutes(3)),
            ("git status".to_string(), vec![], now - Duration::minutes(2)),
            (
                "cargo clippy".to_string(),
                vec![],
                now - Duration::minutes(1),
            ),
            ("git diff".to_string(), vec![], now),
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

        assert_eq!(picker.selected_index(), Some(0));

        picker.move_down();
        assert_eq!(picker.selected_index(), Some(1));

        picker.move_up();
        assert_eq!(picker.selected_index(), Some(0));

        // Wrap around
        picker.move_up();
        assert_eq!(picker.selected_index(), Some(4));
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
        let now = Utc::now();
        let history = vec![
            (
                "cargo build".to_string(),
                vec![],
                now - Duration::minutes(3),
            ),
            ("cargo test".to_string(), vec![], now - Duration::minutes(2)),
            (
                "cargo build".to_string(),
                vec![],
                now - Duration::minutes(1),
            ), // Duplicate
            ("cargo build".to_string(), vec![], now), // Another duplicate
        ];

        picker.open(&manager);
        picker.update_search(&history);

        // Should deduplicate to 2 unique entries
        assert_eq!(picker.match_count(), 2);
    }

    #[test]
    fn test_archived_prompts_merged() {
        let mut picker = HistoryPickerState::new();
        let manager = InputManager::new();
        let now = Utc::now();
        let history = vec![("cargo build".to_string(), vec![], now)];

        picker.set_archived_prompts(vec![
            ArchivedPrompt {
                content: "implement auth".to_string(),
                created_at: now - Duration::hours(3),
                session_label: "session-1".to_string(),
            },
            ArchivedPrompt {
                content: "fix login bug".to_string(),
                created_at: now - Duration::hours(6),
                session_label: "session-2".to_string(),
            },
        ]);

        picker.open(&manager);
        picker.update_search(&history);

        // 1 current + 2 archived = 3
        assert_eq!(picker.match_count(), 3);
    }

    #[test]
    fn test_archived_dedup_with_current() {
        let mut picker = HistoryPickerState::new();
        let manager = InputManager::new();
        let now = Utc::now();
        let history = vec![("cargo build".to_string(), vec![], now)];

        picker.set_archived_prompts(vec![ArchivedPrompt {
            content: "cargo build".to_string(), // same as current
            created_at: now - Duration::hours(3),
            session_label: "session-1".to_string(),
        }]);

        picker.open(&manager);
        picker.update_search(&history);

        // Should dedup to 1 entry
        assert_eq!(picker.match_count(), 1);
    }

    #[test]
    fn test_format_time_ago() {
        assert_eq!(format_time_ago(Duration::seconds(30)), "just now");
        assert_eq!(format_time_ago(Duration::seconds(90)), "1m ago");
        assert_eq!(format_time_ago(Duration::minutes(45)), "45m ago");
        assert_eq!(format_time_ago(Duration::hours(3)), "3h ago");
        assert_eq!(format_time_ago(Duration::hours(25)), "1d ago");
        assert_eq!(format_time_ago(Duration::zero()), "just now");
        // Negative duration (clock skew)
        assert_eq!(format_time_ago(Duration::seconds(-10)), "just now");
    }
}
