/// Input management for terminal sessions
///
/// Encapsulates user input state, including text editing, cursor movement,
/// and command history navigation.

use std::time::Instant;

/// Manages user input state including text, cursor, and history
#[derive(Clone, Debug)]
pub struct InputManager {
    /// The input text content
    content: String,
    /// Current cursor position in the input
    cursor: usize,
    /// Command history entries
    history: Vec<String>,
    /// Current position in history (None = viewing current input)
    history_index: Option<usize>,
    /// Unsaved draft when navigating history
    history_draft: Option<String>,
    /// Time of last Escape key press for double-tap detection
    last_escape_time: Option<Instant>,
    /// Whether input is enabled
    enabled: bool,
}

impl InputManager {
    /// Creates a new input manager
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            history_draft: None,
            last_escape_time: None,
            enabled: true,
        }
    }

    /// Returns the current input content
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns mutable reference to content (for direct manipulation if needed)
    pub fn content_mut(&mut self) -> &mut String {
        &mut self.content
    }

    /// Sets the input content and resets cursor to end
    pub fn set_content(&mut self, content: String) {
        self.content = content.clone();
        self.cursor = content.len();
        self.reset_history_navigation();
    }

    /// Returns the current cursor position
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Sets the cursor position (clamped to valid range)
    pub fn set_cursor(&mut self, pos: usize) {
        self.cursor = pos.min(self.content.len());
    }

    /// Moves cursor left by one character (UTF-8 aware)
    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            let mut pos = self.cursor - 1;
            while pos > 0 && !self.content.is_char_boundary(pos) {
                pos -= 1;
            }
            self.cursor = pos;
        }
    }

    /// Moves cursor right by one character (UTF-8 aware)
    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.content.len() {
            let mut pos = self.cursor + 1;
            while pos < self.content.len() && !self.content.is_char_boundary(pos) {
                pos += 1;
            }
            self.cursor = pos;
        }
    }

    /// Moves cursor to the beginning
    pub fn move_cursor_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Moves cursor to the end
    pub fn move_cursor_to_end(&mut self) {
        self.cursor = self.content.len();
    }

    /// Inserts text at the current cursor position
    pub fn insert_text(&mut self, text: &str) {
        self.content.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Deletes the character before the cursor
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let mut pos = self.cursor - 1;
            while pos > 0 && !self.content.is_char_boundary(pos) {
                pos -= 1;
            }
            self.content.drain(pos..self.cursor);
            self.cursor = pos;
        }
    }

    /// Deletes the character at the cursor
    pub fn delete(&mut self) {
        if self.cursor < self.content.len() {
            let mut end = self.cursor + 1;
            while end < self.content.len() && !self.content.is_char_boundary(end) {
                end += 1;
            }
            self.content.drain(self.cursor..end);
        }
    }

    /// Clears all input
    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
        self.reset_history_navigation();
    }

    /// Adds an entry to history and resets navigation
    pub fn add_to_history(&mut self, entry: String) {
        if !entry.trim().is_empty() {
            self.history.push(entry);
        }
        self.reset_history_navigation();
    }

    /// Navigates to the next history entry
    pub fn go_to_next_history(&mut self) -> Option<String> {
        match self.history_index {
            None => None,
            Some(0) => {
                self.history_index = None;
                self.history_draft.take()
            }
            Some(i) => {
                self.history_index = Some(i - 1);
                self.history.get(i - 1).cloned()
            }
        }
    }

    /// Navigates to the previous history entry
    pub fn go_to_previous_history(&mut self) -> Option<String> {
        let current_index = match self.history_index {
            None => {
                // Save current input as draft when starting history navigation
                self.history_draft = Some(self.content.clone());
                self.history.len().saturating_sub(1)
            }
            Some(i) => {
                if i == 0 {
                    return None;
                }
                i - 1
            }
        };

        if current_index < self.history.len() {
            self.history_index = Some(current_index);
            self.history.get(current_index).cloned()
        } else {
            None
        }
    }

    /// Resets history navigation to viewing current input
    pub fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    /// Gets the history draft (unsaved input from before history navigation)
    pub fn history_draft(&self) -> Option<&str> {
        self.history_draft.as_deref()
    }

    /// Checks if currently viewing history
    pub fn is_in_history(&self) -> bool {
        self.history_index.is_some()
    }

    /// Sets whether input is enabled
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns whether input is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Updates last escape time and returns true if double-tap (within 300ms)
    pub fn check_escape_double_tap(&mut self) -> bool {
        let now = Instant::now();
        let is_double_tap = if let Some(last_time) = self.last_escape_time {
            now.duration_since(last_time).as_millis() < 300
        } else {
            false
        };

        self.last_escape_time = Some(now);
        is_double_tap
    }

    /// Clears escape time (useful when exiting input mode)
    pub fn reset_escape_time(&mut self) {
        self.last_escape_time = None;
    }

    /// Returns the history entries (for debugging/testing)
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Returns the current history index
    pub fn history_index(&self) -> Option<usize> {
        self.history_index
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_input_manager_is_empty() {
        let manager = InputManager::new();
        assert_eq!(manager.content(), "");
        assert_eq!(manager.cursor(), 0);
        assert!(manager.is_enabled());
    }

    #[test]
    fn insert_text_updates_content_and_cursor() {
        let mut manager = InputManager::new();
        manager.insert_text("hello");
        assert_eq!(manager.content(), "hello");
        assert_eq!(manager.cursor(), 5);
    }

    #[test]
    fn backspace_removes_character_before_cursor() {
        let mut manager = InputManager::new();
        manager.insert_text("hello");
        manager.backspace();
        assert_eq!(manager.content(), "hell");
        assert_eq!(manager.cursor(), 4);
    }

    #[test]
    fn delete_removes_character_at_cursor() {
        let mut manager = InputManager::new();
        manager.insert_text("hello");
        manager.set_cursor(1);
        manager.delete();
        assert_eq!(manager.content(), "hllo");
    }

    #[test]
    fn move_cursor_left_and_right() {
        let mut manager = InputManager::new();
        manager.insert_text("hello");
        manager.move_cursor_left();
        assert_eq!(manager.cursor(), 4);
        manager.move_cursor_right();
        assert_eq!(manager.cursor(), 5);
    }

    #[test]
    fn clear_resets_state() {
        let mut manager = InputManager::new();
        manager.insert_text("hello");
        manager.clear();
        assert_eq!(manager.content(), "");
        assert_eq!(manager.cursor(), 0);
    }

    #[test]
    fn history_navigation() {
        let mut manager = InputManager::new();
        manager.add_to_history("first".to_string());
        manager.add_to_history("second".to_string());

        assert_eq!(manager.go_to_previous_history(), Some("second".to_string()));
        assert_eq!(manager.go_to_previous_history(), Some("first".to_string()));
        assert_eq!(manager.go_to_previous_history(), None);
    }

    #[test]
    fn history_navigation_saves_draft() {
        let mut manager = InputManager::new();
        manager.set_content("current".to_string());
        manager.add_to_history("previous".to_string());

        manager.go_to_previous_history();
        assert_eq!(manager.go_to_next_history(), Some("current".to_string()));
    }

    #[test]
    fn escape_double_tap_detection() {
        let mut manager = InputManager::new();
        assert!(!manager.check_escape_double_tap());
        // Would need to wait or mock time for real double-tap test
    }

    #[test]
    fn utf8_cursor_movement() {
        let mut manager = InputManager::new();
        manager.insert_text("你好");
        assert_eq!(manager.cursor(), 6); // 2 chars * 3 bytes

        manager.move_cursor_left();
        assert_eq!(manager.cursor(), 3);

        manager.move_cursor_right();
        assert_eq!(manager.cursor(), 6);
    }
}
