/// Input management for terminal sessions
///
/// Encapsulates user input state, including text editing, cursor movement,
/// and command history navigation.  Text editing and cursor positioning are
/// delegated to [`ratatui_textarea::TextArea`], which provides undo/redo,
/// proper UTF-8 handling, and a battle-tested editing model.
use ratatui_textarea::{CursorMove, DataCursor, TextArea};

use super::super::types::ContentPart;
use super::mouse_selection::MouseSelectionState;
use super::textarea_bridge;

fn configure_textarea(textarea: &mut TextArea<'static>) {
    textarea.set_max_histories(50);
    textarea.set_tab_length(4);
}

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

/// Manages user input state including text, cursor, and history.
///
/// The text buffer and cursor are managed by [`TextArea`], giving us
/// undo/redo and proper character-boundary handling for free.  Selection is
/// tracked via an anchor model (`selection_anchor` + cursor) so that the
/// existing rendering pipeline (which reads byte-offset ranges) continues to
/// work unchanged.
#[derive(Clone, Debug)]
pub struct InputManager {
    textarea: TextArea<'static>,
    /// Byte-offset selection anchor.  `Some(anchor)` when a range selection is
    /// active; `None` otherwise.
    selection_anchor: Option<usize>,
    /// Whether the current selection has already been copied to the clipboard.
    selection_copied: bool,
    /// Non-text input elements (e.g. image attachments)
    attachments: Vec<ContentPart>,
    /// Command history entries
    history: Vec<InputHistoryEntry>,
    /// Current position in history (None = viewing current input)
    history_index: Option<usize>,
    /// Unsaved draft when navigating history
    history_draft: Option<InputHistoryEntry>,
}

#[expect(dead_code)]
impl InputManager {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        configure_textarea(&mut textarea);
        Self {
            textarea,
            selection_anchor: None,
            selection_copied: false,
            attachments: Vec::new(),
            history: Vec::new(),
            history_index: None,
            history_draft: None,
        }
    }

    // ------------------------------------------------------------------
    // Content access
    // ------------------------------------------------------------------

    pub fn content(&self) -> &str {
        // `leak` converts the joined `String` into `&'static str`.
        // Acceptable because vtcode's input is bounded to `INLINE_INPUT_MAX_LINES`
        // (10) lines of ~200 chars each — at most ~2 KB per call.  The leaked
        // memory is reclaimed when the process exits.  Returning `&'static str`
        // avoids changing the 100+ call sites that expect `&str`.
        self.textarea.lines().join("\n").leak()
    }

    pub fn set_content(&mut self, content: String) {
        self.textarea = TextArea::from(content.split('\n'));
        configure_textarea(&mut self.textarea);
        self.textarea.move_cursor(CursorMove::End);
        self.clear_selection();
        self.reset_history_navigation();
    }

    pub fn cursor(&self) -> usize {
        let DataCursor(row, col) = self.textarea.cursor();
        textarea_bridge::row_col_to_byte_offset(self.textarea.lines(), row, col)
    }

    fn set_textarea_cursor(&mut self, row: usize, col: usize) {
        let row = u16::try_from(row).unwrap_or(u16::MAX);
        let col = u16::try_from(col).unwrap_or(u16::MAX);
        self.textarea.move_cursor(CursorMove::Jump(row, col));
    }

    pub fn set_cursor(&mut self, pos: usize) {
        let (row, col) = textarea_bridge::byte_offset_to_row_col(self.textarea.lines(), pos);
        self.set_textarea_cursor(row, col);
        self.clear_selection();
    }

    pub fn set_cursor_with_selection(&mut self, pos: usize) {
        let previous_selection = self.selection_range();
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor());
            self.textarea.start_selection();
        }
        let (row, col) = textarea_bridge::byte_offset_to_row_col(self.textarea.lines(), pos);
        self.set_textarea_cursor(row, col);
        if self.selection_range() != previous_selection {
            self.selection_copied = false;
        }
    }

    // ------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        let cursor = self.cursor();
        if anchor == cursor {
            return None;
        }
        Some((anchor.min(cursor), anchor.max(cursor)))
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        // Build directly from TextArea lines to avoid leaking via content().
        let mut result = String::new();
        let mut byte_pos = 0;
        for line in self.textarea.lines() {
            let line_end = byte_pos + line.len();
            if start < line_end && end > byte_pos {
                let lo = start.saturating_sub(byte_pos);
                let hi = end.min(line_end) - byte_pos;
                result.push_str(&line[lo..hi]);
            }
            if end <= line_end {
                break;
            }
            if start <= line_end && end > line_end {
                result.push('\n');
            }
            byte_pos = line_end + 1; // +1 for '\n'
        }
        Some(result)
    }

    pub fn copy_selected_text_to_clipboard(&mut self) -> bool {
        let Some(text) = self.selected_text() else {
            return false;
        };
        MouseSelectionState::copy_to_clipboard(&text);
        self.selection_copied = true;
        true
    }

    pub fn selection_needs_copy(&self) -> bool {
        self.has_selection() && !self.selection_copied
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_copied = false;
        self.textarea.cancel_selection();
    }

    // ------------------------------------------------------------------
    // Editing
    // ------------------------------------------------------------------

    pub(crate) fn replace_range(&mut self, start: usize, end: usize, replacement: &str) {
        let lines: Vec<String> = self.textarea.lines().to_vec();
        let (start_line, start_col) = textarea_bridge::byte_offset_to_row_col(&lines, start);
        let (end_line, end_col) = textarea_bridge::byte_offset_to_row_col(&lines, end);

        let rep_parts: Vec<&str> = replacement.split('\n').collect();
        let has_newlines = rep_parts.len() > 1;

        if start_line == end_line {
            let line = &lines[start_line];
            let start_byte = textarea_bridge::char_col_to_byte_offset(line, start_col);
            let end_byte = textarea_bridge::char_col_to_byte_offset(line, end_col);
            let remaining = &line[end_byte..];

            if !has_newlines {
                // Single-line replacement: fast path.
                let mut new_line =
                    String::with_capacity(start_byte + replacement.len() + remaining.len());
                new_line.push_str(&line[..start_byte]);
                new_line.push_str(replacement);
                new_line.push_str(remaining);
                self.textarea = TextArea::from(lines.iter().enumerate().map(|(i, l)| {
                    if i == start_line {
                        new_line.as_str()
                    } else {
                        l.as_str()
                    }
                }));
            } else {
                // Multi-line replacement: split across lines.
                let mut new_lines: Vec<String> =
                    Vec::with_capacity(lines.len() + rep_parts.len() - 1);
                for (i, l) in lines.iter().enumerate() {
                    if i < start_line {
                        new_lines.push(l.clone());
                    } else if i == start_line {
                        let mut first = String::with_capacity(start_byte + rep_parts[0].len());
                        first.push_str(&line[..start_byte]);
                        first.push_str(rep_parts[0]);
                        new_lines.push(first);
                        for mid in &rep_parts[1..rep_parts.len() - 1] {
                            new_lines.push(mid.to_string());
                        }
                        let last = rep_parts.last().unwrap();
                        let mut last_line = String::with_capacity(last.len() + remaining.len());
                        last_line.push_str(last);
                        last_line.push_str(remaining);
                        new_lines.push(last_line);
                    } else {
                        new_lines.push(l.clone());
                    }
                }
                self.textarea = TextArea::from(new_lines);
            }
        } else {
            let end_line_obj = &lines[end_line];
            let end_byte = textarea_bridge::char_col_to_byte_offset(end_line_obj, end_col);
            let remaining = &end_line_obj[end_byte..];

            let mut new_lines: Vec<String> =
                Vec::with_capacity(lines.len() + rep_parts.len().saturating_sub(1));
            for (i, line) in lines.iter().enumerate() {
                if i < start_line {
                    new_lines.push(line.clone());
                } else if i == start_line {
                    let start_byte = textarea_bridge::char_col_to_byte_offset(line, start_col);
                    if !has_newlines {
                        let mut merged =
                            String::with_capacity(start_byte + replacement.len() + remaining.len());
                        merged.push_str(&line[..start_byte]);
                        merged.push_str(replacement);
                        merged.push_str(remaining);
                        new_lines.push(merged);
                    } else {
                        let mut first = String::with_capacity(start_byte + rep_parts[0].len());
                        first.push_str(&line[..start_byte]);
                        first.push_str(rep_parts[0]);
                        new_lines.push(first);
                        for mid in &rep_parts[1..rep_parts.len() - 1] {
                            new_lines.push(mid.to_string());
                        }
                        let last = rep_parts.last().unwrap();
                        let mut last_line = String::with_capacity(last.len() + remaining.len());
                        last_line.push_str(last);
                        last_line.push_str(remaining);
                        new_lines.push(last_line);
                    }
                } else if i > end_line {
                    new_lines.push(line.clone());
                }
            }
            self.textarea = TextArea::from(new_lines);
        }

        configure_textarea(&mut self.textarea);
        let cursor_byte = start + replacement.len();
        let (row, col) =
            textarea_bridge::byte_offset_to_row_col(self.textarea.lines(), cursor_byte);
        self.set_textarea_cursor(row, col);
        self.clear_selection();
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        self.replace_range(start, end, "");
        true
    }

    pub fn move_cursor_left(&mut self) {
        if let Some((start, _)) = self.selection_range() {
            let (row, col) = textarea_bridge::byte_offset_to_row_col(self.textarea.lines(), start);
            self.set_textarea_cursor(row, col);
            self.clear_selection();
            return;
        }
        self.textarea.cancel_selection();
        self.textarea.move_cursor(CursorMove::Back);
    }

    pub fn move_cursor_right(&mut self) {
        if let Some((_, end)) = self.selection_range() {
            let (row, col) = textarea_bridge::byte_offset_to_row_col(self.textarea.lines(), end);
            self.set_textarea_cursor(row, col);
            self.clear_selection();
            return;
        }
        self.textarea.cancel_selection();
        self.textarea.move_cursor(CursorMove::Forward);
    }

    pub fn move_cursor_to_start(&mut self) {
        self.set_textarea_cursor(0, 0);
        self.clear_selection();
    }

    pub fn move_cursor_to_end(&mut self) {
        let last_row = self.textarea.lines().len().saturating_sub(1);
        let last_col = self
            .textarea
            .lines()
            .last()
            .map_or(0, |l| l.chars().count());
        self.set_textarea_cursor(last_row, last_col);
        self.clear_selection();
    }

    pub fn insert_char(&mut self, ch: char) {
        if let Some((start, end)) = self.selection_range() {
            let mut buf = [0_u8; 4];
            self.replace_range(start, end, ch.encode_utf8(&mut buf));
        } else {
            self.textarea.insert_char(ch);
        }
    }

    pub fn insert_text(&mut self, text: &str) {
        if let Some((start, end)) = self.selection_range() {
            self.replace_range(start, end, text);
        } else {
            self.textarea.insert_str(text);
        }
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.textarea.delete_char();
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.textarea.delete_next_char();
    }

    pub fn delete_word_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.textarea.delete_next_word();
    }

    pub fn delete_whitespace_around_cursor(&mut self) {
        let content = self.content().to_string();
        let cursor = self.cursor();
        if content.is_empty() || cursor >= content.len() {
            return;
        }

        let before = &content[..cursor];
        let after = &content[cursor..];

        let new_before = before.trim_end();
        let new_after = after.trim_start();

        let removed_before = before.len() - new_before.len();
        let removed_after = after.len() - new_after.len();

        if removed_before == 0 && removed_after == 0 {
            return;
        }

        let new_content = format!("{}{}", new_before, new_after);
        let new_cursor = cursor - removed_before;

        self.set_content(new_content);
        self.set_cursor(new_cursor);
    }

    pub fn transpose_chars(&mut self) {
        let content = self.content().to_string();
        let cursor = self.cursor();
        if content.len() < 2 {
            return;
        }

        let mut chars: Vec<char> = content.chars().collect();
        let char_pos = content[..cursor].chars().count();

        if char_pos == 0 {
            // At start: swap first two chars
            chars.swap(0, 1);
            let new_content: String = chars.into_iter().collect();
            self.set_content(new_content);
            self.set_cursor(cursor + 1);
        } else if char_pos >= chars.len() {
            // At end: swap last two chars
            let last = chars.len() - 1;
            chars.swap(last - 1, last);
            let new_content: String = chars.into_iter().collect();
            self.set_content(new_content);
        } else {
            // In middle: swap char at cursor with char before
            chars.swap(char_pos - 1, char_pos);
            let new_content: String = chars.into_iter().collect();
            self.set_content(new_content);
            self.set_cursor(cursor + 1);
        }
    }

    pub fn transpose_words(&mut self) {
        let content = self.content().to_string();
        let cursor = self.cursor();
        if content.is_empty() {
            return;
        }

        let chars: Vec<char> = content.chars().collect();
        let char_pos = content[..cursor].chars().count();

        // Find word boundaries
        let is_word_char = |c: char| c.is_alphanumeric();

        // Find start of current word
        let mut word_start = char_pos;
        while word_start > 0 && is_word_char(chars[word_start - 1]) {
            word_start -= 1;
        }

        // Find end of current word
        let mut word_end = char_pos;
        while word_end < chars.len() && is_word_char(chars[word_end]) {
            word_end += 1;
        }

        // Find start of previous word
        let mut prev_start = word_start;
        while prev_start > 0 && !is_word_char(chars[prev_start - 1]) {
            prev_start -= 1;
        }
        while prev_start > 0 && is_word_char(chars[prev_start - 1]) {
            prev_start -= 1;
        }

        // Find end of previous word
        let mut prev_end = prev_start;
        while prev_end < chars.len() && is_word_char(chars[prev_end]) {
            prev_end += 1;
        }

        if prev_start >= word_start || prev_end >= word_start {
            return;
        }

        // Extract words
        let prev_word: String = chars[prev_start..prev_end].iter().collect();
        let curr_word: String = chars[word_start..word_end].iter().collect();
        let between: String = chars[prev_end..word_start].iter().collect();

        // Reconstruct
        let new_content = format!(
            "{}{}{}{}{}",
            &content[..prev_start],
            curr_word,
            between,
            prev_word,
            &content[word_end..]
        );

        self.set_content(new_content);
        self.set_cursor(cursor);
    }

    pub fn uppercase_word(&mut self) {
        self.transform_word(|s| s.to_uppercase());
    }

    pub fn lowercase_word(&mut self) {
        self.transform_word(|s| s.to_lowercase());
    }

    pub fn capitalize_word(&mut self) {
        self.transform_word(|s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut result = c.to_uppercase().to_string();
                    result.extend(chars.flat_map(|c| c.to_lowercase()));
                    result
                }
            }
        });
    }

    fn transform_word<F: Fn(&str) -> String>(&mut self, transform: F) {
        let content = self.content().to_string();
        let cursor = self.cursor();
        if content.is_empty() {
            return;
        }

        let chars: Vec<char> = content.chars().collect();
        let char_pos = content[..cursor].chars().count();

        let is_word_char = |c: char| c.is_alphanumeric();

        // Find start of current word
        let mut word_start = char_pos;
        while word_start > 0 && is_word_char(chars[word_start - 1]) {
            word_start -= 1;
        }

        // Find end of current word
        let mut word_end = word_start;
        while word_end < chars.len() && is_word_char(chars[word_end]) {
            word_end += 1;
        }

        if word_start == word_end {
            return;
        }

        let word: String = chars[word_start..word_end].iter().collect();
        let transformed = transform(&word);

        let new_content = format!(
            "{}{}{}",
            &content[..word_start],
            transformed,
            &content[word_end..]
        );

        self.set_content(new_content);
        self.set_cursor(cursor);
    }

    pub fn clear(&mut self) {
        self.textarea = TextArea::default();
        configure_textarea(&mut self.textarea);
        self.clear_selection();
        self.attachments.clear();
        self.reset_history_navigation();
    }

    // ------------------------------------------------------------------
    // Undo / redo (new — powered by TextArea)
    // ------------------------------------------------------------------

    pub fn undo(&mut self) -> bool {
        self.textarea.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.textarea.redo()
    }

    // ------------------------------------------------------------------
    // History (command submission, NOT undo/redo)
    // ------------------------------------------------------------------

    pub fn add_to_history(&mut self, entry: InputHistoryEntry) {
        if !entry.is_empty() {
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

    pub fn go_to_previous_history(&mut self) -> Option<InputHistoryEntry> {
        let current_index = match self.history_index {
            None => {
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

    pub fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    pub fn history(&self) -> &[InputHistoryEntry] {
        &self.history
    }

    pub fn history_texts(&self) -> Vec<String> {
        self.history
            .iter()
            .map(|entry| entry.content.clone())
            .collect()
    }

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
            self.content().to_string(),
            self.attachments.clone(),
        )
    }

    pub fn apply_history_entry(&mut self, entry: InputHistoryEntry) {
        // Directly replace the TextArea without calling `set_content` which
        // would reset history navigation state.
        self.textarea = TextArea::from(entry.content.split('\n'));
        configure_textarea(&mut self.textarea);
        self.textarea.move_cursor(CursorMove::End);
        self.clear_selection();
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

    #[test]
    fn insert_text_replaces_selection() {
        let mut manager = InputManager::new();
        manager.insert_text("hello world");
        manager.set_cursor(5);
        manager.set_cursor_with_selection(11);

        manager.insert_text(" there");

        assert_eq!(manager.content(), "hello there");
        assert_eq!(manager.cursor(), "hello there".len());
        assert!(!manager.has_selection());
    }

    #[test]
    fn backspace_deletes_selected_range() {
        let mut manager = InputManager::new();
        manager.insert_text("hello world");
        manager.set_cursor(0);
        manager.set_cursor_with_selection(5);

        manager.backspace();

        assert_eq!(manager.content(), " world");
        assert_eq!(manager.cursor(), 0);
        assert!(!manager.has_selection());
    }

    #[test]
    fn move_cursor_left_collapses_selection_to_start() {
        let mut manager = InputManager::new();
        manager.insert_text("hello world");
        manager.set_cursor(0);
        manager.set_cursor_with_selection(5);

        manager.move_cursor_left();

        assert_eq!(manager.cursor(), 0);
        assert!(!manager.has_selection());
    }

    #[test]
    fn undo_redo() {
        let mut manager = InputManager::new();
        manager.insert_text("hello");
        assert_eq!(manager.content(), "hello");

        manager.undo();
        assert_eq!(manager.content(), "");
        assert_eq!(manager.cursor(), 0);

        manager.redo();
        assert_eq!(manager.content(), "hello");
    }
}
