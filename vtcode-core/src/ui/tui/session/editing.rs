use super::Session;
use crate::config::constants::ui;
use crate::ui::tui::session::slash;
/// Text editing and cursor movement operations for Session
///
/// This module handles all text manipulation and cursor navigation including:
/// - Character insertion and deletion
/// - Word and sentence-level editing
/// - Cursor movement (character, word, line boundaries)
/// - Input history navigation
/// - Newline handling with capacity limits
use unicode_segmentation::UnicodeSegmentation;

impl Session {
    /// Insert a character at the current cursor position
    pub(super) fn insert_char(&mut self, ch: char) {
        if ch == '\u{7f}' {
            return;
        }
        if ch == '\n' && !self.can_insert_newline() {
            return;
        }
        self.input_manager.insert_char(ch);
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        slash::update_slash_suggestions(self);
    }

    /// Insert pasted text without enforcing the inline newline cap.
    ///
    /// This preserves the full block (including large multi-line pastes) so the
    /// agent receives the exact content instead of dropping line breaks after
    /// hitting the interactive input's visual limit.
    pub fn insert_paste_text(&mut self, text: &str) {
        let sanitized: String = text
            .chars()
            .filter(|&ch| ch != '\r' && ch != '\u{7f}')
            .collect();

        if sanitized.is_empty() {
            return;
        }

        self.input_manager.insert_text(&sanitized);
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        slash::update_slash_suggestions(self);
    }

    /// Calculate remaining newline capacity in the input field
    pub(super) fn remaining_newline_capacity(&self) -> usize {
        ui::INLINE_INPUT_MAX_LINES
            .saturating_sub(1)
            .saturating_sub(self.input_manager.content().matches('\n').count())
    }

    /// Check if a newline can be inserted
    pub(super) fn can_insert_newline(&self) -> bool {
        self.remaining_newline_capacity() > 0
    }

    /// Delete the character before the cursor (backspace)
    pub(super) fn delete_char(&mut self) {
        self.input_manager.backspace();
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        slash::update_slash_suggestions(self);
    }

    /// Delete the character at the cursor (forward delete)
    pub(super) fn delete_char_forward(&mut self) {
        self.input_manager.delete();
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        slash::update_slash_suggestions(self);
    }

    /// Delete the word before the cursor
    pub(super) fn delete_word_backward(&mut self) {
        if self.input_manager.cursor() == 0 {
            return;
        }

        // Find the start of the current word by moving backward
        let graphemes: Vec<(usize, &str)> = self
            .input_manager
            .content()
            .grapheme_indices(true)
            .take_while(|(idx, _)| *idx < self.input_manager.cursor())
            .collect();

        if graphemes.is_empty() {
            return;
        }

        let mut index = graphemes.len();

        // Skip any trailing whitespace
        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index -= 1;
        }

        // Move backwards until we find whitespace (start of the word)
        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index -= 1;
        }

        // Calculate the position to delete from
        let delete_start = if index < graphemes.len() {
            graphemes[index].0
        } else {
            self.input_manager.cursor()
        };

        // Delete from delete_start to cursor
        if delete_start < self.input_manager.cursor() {
            let before = &self.input_manager.content()[..delete_start];
            let after = &self.input_manager.content()[self.input_manager.cursor()..];
            let new_content = format!("{}{}", before, after);

            self.input_manager.set_content(new_content);
            self.input_manager.set_cursor(delete_start);
            self.input_compact_mode = self.input_compact_placeholder().is_some();
            slash::update_slash_suggestions(self);
        }
    }

    #[allow(dead_code)]
    pub(super) fn delete_word_forward(&mut self) {
        self.input_manager.delete_word_forward();
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        slash::update_slash_suggestions(self);
    }
    /// Delete from cursor to start of current line (Command+Backspace on macOS)
    pub(super) fn delete_to_start_of_line(&mut self) {
        let content = self.input_manager.content();
        let cursor = self.input_manager.cursor();

        // Find the previous newline or start of string
        let before = &content[..cursor];
        let delete_start = if let Some(newline_pos) = before.rfind('\n') {
            newline_pos + 1 // Delete after the newline
        } else {
            0 // Delete from start
        };

        if delete_start < cursor {
            let new_content = format!("{}{}", &content[..delete_start], &content[cursor..]);
            self.input_manager.set_content(new_content);
            self.input_manager.set_cursor(delete_start);
            self.input_compact_mode = self.input_compact_placeholder().is_some();
            slash::update_slash_suggestions(self);
        }
    }

    /// Delete from cursor to end of current line (Command+Delete on macOS)
    pub(super) fn delete_to_end_of_line(&mut self) {
        let content = self.input_manager.content();
        let cursor = self.input_manager.cursor();

        // Find the next newline or end of string
        let rest = &content[cursor..];
        let delete_len = if let Some(newline_pos) = rest.find('\n') {
            newline_pos
        } else {
            rest.len()
        };

        if delete_len > 0 {
            let new_content = format!("{}{}", &content[..cursor], &content[cursor + delete_len..]);
            self.input_manager.set_content(new_content);
            self.input_compact_mode = self.input_compact_placeholder().is_some();
            slash::update_slash_suggestions(self);
        }
    }

    /// Move cursor left by one character
    pub(super) fn move_left(&mut self) {
        self.input_manager.move_cursor_left();
        slash::update_slash_suggestions(self);
    }

    /// Move cursor right by one character
    pub(super) fn move_right(&mut self) {
        self.input_manager.move_cursor_right();
        slash::update_slash_suggestions(self);
    }

    /// Move cursor left to the start of the previous word
    pub(super) fn move_left_word(&mut self) {
        if self.input_manager.cursor() == 0 {
            return;
        }

        let graphemes: Vec<(usize, &str)> = self
            .input_manager
            .content()
            .grapheme_indices(true)
            .take_while(|(idx, _)| *idx < self.input_manager.cursor())
            .collect();

        if graphemes.is_empty() {
            return;
        }

        let mut index = graphemes.len();

        // Skip trailing whitespace
        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index -= 1;
        }

        // Move to start of word
        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index -= 1;
        }

        if index < graphemes.len() {
            self.input_manager.set_cursor(graphemes[index].0);
        } else {
            self.input_manager.set_cursor(0);
        }
    }
    /// Move cursor right to the start of the next word
    pub(super) fn move_right_word(&mut self) {
        if self.input_manager.cursor() >= self.input_manager.content().len() {
            return;
        }

        let graphemes: Vec<(usize, &str)> = self
            .input_manager
            .content()
            .grapheme_indices(true)
            .skip_while(|(idx, _)| *idx < self.input_manager.cursor())
            .collect();

        if graphemes.is_empty() {
            self.input_manager.move_cursor_to_end();
            return;
        }

        let mut index = 0;
        let mut skipped_whitespace = false;

        // Skip current whitespace
        while index < graphemes.len() {
            let (_, grapheme) = graphemes[index];
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            skipped_whitespace = true;
            index += 1;
        }

        if index >= graphemes.len() {
            self.input_manager.move_cursor_to_end();
            return;
        }

        // If we skipped whitespace, we're at the start of a word
        if skipped_whitespace {
            self.input_manager.set_cursor(graphemes[index].0);
            return;
        }

        // Otherwise, skip to end of current word
        while index < graphemes.len() {
            let (_, grapheme) = graphemes[index];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index += 1;
        }

        if index < graphemes.len() {
            self.input_manager.set_cursor(graphemes[index].0);
        } else {
            self.input_manager.move_cursor_to_end();
        }
    }
    /// Move cursor to the start of the line
    pub(super) fn move_to_start(&mut self) {
        self.input_manager.move_cursor_to_start();
    }

    /// Move cursor to the end of the line
    pub(super) fn move_to_end(&mut self) {
        self.input_manager.move_cursor_to_end();
    }

    /// Remember submitted input in history
    pub(super) fn remember_submitted_input(
        &mut self,
        submitted: super::input_manager::InputHistoryEntry,
    ) {
        self.input_manager.add_to_history(submitted);
    }

    /// Navigate to previous history entry (disabled to prevent cursor flickering)
    #[allow(dead_code)]
    pub(super) fn navigate_history_previous(&mut self) -> bool {
        if let Some(previous) = self.input_manager.go_to_previous_history() {
            self.input_manager.apply_history_entry(previous);
            true
        } else {
            false
        }
    }

    /// Navigate to next history entry (disabled to prevent cursor flickering)
    #[allow(dead_code)]
    pub(super) fn navigate_history_next(&mut self) -> bool {
        if let Some(next) = self.input_manager.go_to_next_history() {
            self.input_manager.apply_history_entry(next);
            true
        } else {
            false
        }
    }

    /// Returns the current history position for status bar display
    /// Returns (current_index, total_entries) or None if not navigating history
    pub fn history_position(&self) -> Option<(usize, usize)> {
        self.input_manager.history_index().map(|idx| {
            let total = self.input_manager.history().len();
            (total - idx, total)
        })
    }
}
