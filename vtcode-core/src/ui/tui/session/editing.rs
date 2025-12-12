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

use super::Session;

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
        slash::update_slash_suggestions(self);
    }

    /// Insert text at the current cursor position
    ///
    /// Sanitizes the text to respect newline capacity limits
    pub(super) fn insert_text(&mut self, text: &str) {
        let mut remaining_newlines = self.remaining_newline_capacity();
        let sanitized: String = text
            .chars()
            .filter(|&ch| {
                if ch == '\n' {
                    if remaining_newlines > 0 {
                        remaining_newlines -= 1;
                        true
                    } else {
                        false
                    }
                } else {
                    ch != '\r' && ch != '\u{7f}'
                }
            })
            .collect();
        if sanitized.is_empty() {
            return;
        }
        self.input_manager.insert_text(&sanitized);
        slash::update_slash_suggestions(self);
    }

    /// Insert pasted text without enforcing the inline newline cap.
    ///
    /// This preserves the full block (including large multi-line pastes) so the
    /// agent receives the exact content instead of dropping line breaks after
    /// hitting the interactive input's visual limit.
    pub(super) fn insert_paste_text(&mut self, text: &str) {
        let sanitized: String = text
            .chars()
            .filter(|&ch| ch != '\r' && ch != '\u{7f}')
            .collect();

        if sanitized.is_empty() {
            return;
        }

        self.input_manager.insert_text(&sanitized);
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
        slash::update_slash_suggestions(self);
    }

    /// Delete the character at the cursor (forward delete)
    pub(super) fn delete_char_forward(&mut self) {
        self.input_manager.delete();
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
            slash::update_slash_suggestions(self);
        }
    }

    /// Delete from the beginning of the line to the cursor (kill sentence backward)
    pub(super) fn delete_sentence_backward(&mut self) {
        if self.input_manager.cursor() == 0 {
            return;
        }

        let input_before_cursor = &self.input_manager.content()[..self.input_manager.cursor()];
        let chars: Vec<(usize, char)> = input_before_cursor.char_indices().collect();

        if chars.is_empty() {
            return;
        }

        // Look backwards from cursor for the most recent sentence ending followed by whitespace
        let mut delete_start = 0;

        // Search backwards to find the most recent sentence boundary
        for i in (0..chars.len()).rev() {
            let (idx, ch) = chars[i];

            // Check if this is a sentence-ending punctuation
            if matches!(ch, '.' | '!' | '?') {
                // Look ahead to see if followed by whitespace or end
                if i + 1 < chars.len() {
                    let (_next_idx, next_ch) = chars[i + 1];
                    if next_ch.is_whitespace() {
                        // Found a sentence boundary - delete from after the whitespace
                        if i + 2 < chars.len() {
                            delete_start = chars[i + 2].0;
                        } else {
                            // At the end, delete from after whitespace to cursor
                            delete_start = input_before_cursor.len();
                        }
                        break;
                    }
                } else {
                    // Punctuation at end - delete from after it
                    delete_start = idx + ch.len_utf8();
                    break;
                }
            }

            // Check for newline as sentence boundary
            if ch == '\n' {
                delete_start = idx + 1;
                break;
            }
        }

        // Delete from delete_start to cursor
        if delete_start < self.input_manager.cursor() {
            let before = &self.input_manager.content()[..delete_start];
            let after = &self.input_manager.content()[self.input_manager.cursor()..];
            let new_content = format!("{}{}", before, after);

            self.input_manager.set_content(new_content);
            self.input_manager.set_cursor(delete_start);
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
    pub(super) fn remember_submitted_input(&mut self, submitted: &str) {
        self.input_manager.add_to_history(submitted.to_owned());
    }

    /// Navigate to previous history entry (disabled to prevent cursor flickering)
    #[allow(dead_code)]
    pub(super) fn navigate_history_previous(&mut self) -> bool {
        // History navigation disabled to prevent cursor flickering
        false
    }

    /// Navigate to next history entry (disabled to prevent cursor flickering)
    #[allow(dead_code)]
    pub(super) fn navigate_history_next(&mut self) -> bool {
        // History navigation disabled to prevent cursor flickering
        false
    }
}
