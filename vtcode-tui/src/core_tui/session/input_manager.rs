/// Input management for terminal sessions
///
/// Encapsulates user input state, including text editing, cursor movement,
/// and command history navigation.
use std::time::Instant;

use super::super::types::ContentPart;

#[derive(Clone, Debug)]
pub struct InputHistoryEntry {
    content: String,
    elements: Vec<ContentPart>,
}

impl InputHistoryEntry {
    pub fn from_content_and_attachments(content: String, attachments: Vec<ContentPart>) -> Self {
        let mut elements = Vec::new();
        if !content.is_empty() {
            elements.push(ContentPart::text(content.clone()));
        }
        elements.extend(attachments.into_iter().filter(ContentPart::is_image));
        Self { content, elements }
    }

    /// Returns the text content of this history entry
    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn has_attachments(&self) -> bool {
        self.elements.iter().any(|part| part.is_image())
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty() && !self.has_attachments()
    }

    pub fn attachment_elements(&self) -> Vec<ContentPart> {
        self.elements
            .iter()
            .filter(|part| part.is_image())
            .cloned()
            .collect()
    }
}

/// Manages user input state including text, cursor, and history
#[derive(Clone, Debug)]
pub struct InputManager {
    /// The input text content
    content: String,
    /// Current cursor position in the input
    cursor: usize,
    /// Non-text input elements (e.g. image attachments)
    attachments: Vec<ContentPart>,
    /// Command history entries
    history: Vec<InputHistoryEntry>,
    /// Current position in history (None = viewing current input)
    history_index: Option<usize>,
    /// Unsaved draft when navigating history
    history_draft: Option<InputHistoryEntry>,
    /// Time of last Escape key press for double-tap detection
    last_escape_time: Option<Instant>,
}

#[allow(dead_code)]
impl InputManager {
    /// Creates a new input manager
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor: 0,
            attachments: Vec::new(),
            history: Vec::new(),
            history_index: None,
            history_draft: None,
            last_escape_time: None,
        }
    }

    /// Returns the current input content
    pub fn content(&self) -> &str {
        &self.content
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

    /// Inserts a single character at the current cursor position
    pub fn insert_char(&mut self, ch: char) {
        self.content.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
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

    /// Deletes the word ahead of the cursor
    pub fn delete_word_forward(&mut self) {
        if self.cursor >= self.content.len() {
            return;
        }
        let rest = &self.content[self.cursor..];
        let end_offset = rest
            .char_indices()
            .skip_while(|(_, c)| !c.is_alphanumeric())
            .skip_while(|(_, c)| c.is_alphanumeric())
            .map(|(i, _)| i)
            .next()
            .unwrap_or(rest.len());
        self.content.drain(self.cursor..self.cursor + end_offset);
    }

    /// Clears all input
    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
        self.attachments.clear();
        self.reset_history_navigation();
    }

    /// Adds an entry to history and resets navigation
    pub fn add_to_history(&mut self, entry: InputHistoryEntry) {
        if !entry.is_empty() {
            // Avoid duplicates
            if let Some(last) = self.history.last()
                && last.content == entry.content
                && last.elements == entry.elements
            {
                self.reset_history_navigation();
                return;
            }
            self.history.push(entry);
        }
        self.reset_history_navigation();
    }

    /// Navigates to the next history entry
    pub fn go_to_next_history(&mut self) -> Option<InputHistoryEntry> {
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
    pub fn go_to_previous_history(&mut self) -> Option<InputHistoryEntry> {
        let current_index = match self.history_index {
            None => {
                // Save current input as draft when starting history navigation
                self.history_draft = Some(self.current_history_entry());
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

    /// Returns the history entries (for debugging/testing)
    pub fn history(&self) -> &[InputHistoryEntry] {
        &self.history
    }

    pub fn history_texts(&self) -> Vec<String> {
        self.history
            .iter()
            .map(|entry| entry.content.clone())
            .collect()
    }

    /// Returns the current history index
    pub fn history_index(&self) -> Option<usize> {
        self.history_index
    }

    pub fn attachments(&self) -> &[ContentPart] {
        &self.attachments
    }

    pub fn set_attachments(&mut self, attachments: Vec<ContentPart>) {
        self.attachments = attachments
            .into_iter()
            .filter(ContentPart::is_image)
            .collect();
    }

    pub fn current_history_entry(&self) -> InputHistoryEntry {
        InputHistoryEntry::from_content_and_attachments(
            self.content.clone(),
            self.attachments.clone(),
        )
    }

    pub fn apply_history_entry(&mut self, entry: InputHistoryEntry) {
        self.content = entry.content.clone();
        self.cursor = self.content.len();
        self.attachments = entry.attachment_elements();
    }

    pub fn apply_history_index(&mut self, index: usize) -> bool {
        let Some(entry) = self.history.get(index).cloned() else {
            return false;
        };
        self.apply_history_entry(entry);
        true
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
        manager.add_to_history(InputHistoryEntry::from_content_and_attachments(
            "first".to_owned(),
            Vec::new(),
        ));
        manager.add_to_history(InputHistoryEntry::from_content_and_attachments(
            "second".to_owned(),
            Vec::new(),
        ));

        assert_eq!(
            manager
                .go_to_previous_history()
                .map(|entry| entry.content.clone()),
            Some("second".to_owned())
        );
        assert_eq!(
            manager
                .go_to_previous_history()
                .map(|entry| entry.content.clone()),
            Some("first".to_owned())
        );
        assert_eq!(
            manager
                .go_to_previous_history()
                .map(|entry| entry.content.clone()),
            None
        );
    }

    #[test]
    fn history_navigation_saves_draft() {
        let mut manager = InputManager::new();
        manager.set_content("current".to_owned());
        manager.add_to_history(InputHistoryEntry::from_content_and_attachments(
            "previous".to_owned(),
            Vec::new(),
        ));

        manager.go_to_previous_history();
        assert_eq!(
            manager
                .go_to_next_history()
                .map(|entry| entry.content.clone()),
            Some("current".to_owned())
        );
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

    #[test]
    fn history_navigation_restores_attachments() {
        let mut manager = InputManager::new();
        manager.set_content("check this".to_owned());
        manager.set_attachments(vec![ContentPart::image(
            "encoded".to_owned(),
            "image/png".to_owned(),
        )]);
        manager.add_to_history(manager.current_history_entry());
        manager.clear();

        let entry = manager.go_to_previous_history().expect("history entry");
        manager.apply_history_entry(entry);

        assert_eq!(manager.content(), "check this");
        assert_eq!(manager.attachments().len(), 1);
    }
}
