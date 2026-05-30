//! Vim-style prompt editing engine shared by VT Code terminal surfaces.

mod engine;
mod text;
mod types;

pub use engine::{Editor, HandleKeyOutcome, handle_key};
pub use text::{next_char_boundary, prev_char_boundary};
pub use types::{VimMode, VimState};

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Editor, VimState, handle_key, next_char_boundary, prev_char_boundary};

    #[derive(Debug)]
    struct TestEditor {
        content: String,
        cursor: usize,
    }

    impl TestEditor {
        fn new(content: &str, cursor: usize) -> Self {
            Self {
                content: content.to_string(),
                cursor,
            }
        }
    }

    impl Editor for TestEditor {
        fn content(&self) -> &str {
            &self.content
        }

        fn cursor(&self) -> usize {
            self.cursor
        }

        fn set_cursor(&mut self, pos: usize) {
            self.cursor = pos.min(self.content.len());
        }

        fn move_left(&mut self) {
            self.cursor = prev_char_boundary(&self.content, self.cursor);
        }

        fn move_right(&mut self) {
            self.cursor = next_char_boundary(&self.content, self.cursor);
        }

        fn delete_char_forward(&mut self) {
            if self.cursor >= self.content.len() {
                return;
            }
            let end = next_char_boundary(&self.content, self.cursor);
            self.content.drain(self.cursor..end);
        }

        fn insert_text(&mut self, text: &str) {
            self.content.insert_str(self.cursor, text);
            self.cursor += text.len();
        }

        fn replace(&mut self, content: String, cursor: usize) {
            self.content = content;
            self.cursor = cursor.min(self.content.len());
        }

        fn replace_range(&mut self, start: usize, end: usize, text: &str) {
            self.content.replace_range(start..end, text);
            self.cursor = (start + text.len()).min(self.content.len());
        }
    }

    fn enable_normal_mode(state: &mut VimState, editor: &mut TestEditor, clipboard: &mut String) {
        state.set_enabled(true);
        let outcome = handle_key(
            state,
            editor,
            clipboard,
            &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(outcome.handled);
    }

    #[test]
    fn control_shortcuts_remain_unhandled() {
        let mut state = VimState::new(true);
        let mut editor = TestEditor::new("hello", 5);
        let mut clipboard = String::new();

        let outcome = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );

        assert!(!outcome.handled);
        assert_eq!(editor.content, "hello");
    }

    #[test]
    fn dd_deletes_current_line() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("one\ntwo\nthree", 4);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        assert!(
            handle_key(
                &mut state,
                &mut editor,
                &mut clipboard,
                &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            )
            .handled
        );
        assert!(
            handle_key(
                &mut state,
                &mut editor,
                &mut clipboard,
                &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            )
            .handled
        );

        assert_eq!(editor.content, "one\nthree");
        assert_eq!(editor.cursor, 4);
    }

    #[test]
    fn dot_repeats_change_word_edit() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("alpha beta", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        );
        editor.insert_text("A");
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );

        editor.set_cursor(1);
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE),
        );

        assert_eq!(editor.content, "AA");
    }

    #[test]
    fn dot_repeats_change_line_edit() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("one\ntwo", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        editor.insert_text("ONE");
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );

        editor.set_cursor(4);
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE),
        );

        assert_eq!(editor.content, "ONE\nONE");
    }

    #[test]
    fn vertical_motion_preserves_preferred_column() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("abcd\nxy\nabcd", 3);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 7);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 11);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 7);
    }

    #[test]
    fn find_repeat_reuses_last_character_search() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("a b c b", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
        );
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 2);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char(';'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 6);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 2);
    }

    #[test]
    fn x_deletes_character_at_cursor() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello", 1);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "hllo");
        assert_eq!(editor.cursor, 1);
    }

    #[test]
    fn dw_deletes_word_forward() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello world", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "world");
        assert_eq!(editor.cursor, 0);
    }

    #[test]
    fn j_joins_lines() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello\nworld", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "hello world");
        // Cursor lands after the join space (on 'w'), not on the space itself
        assert_eq!(editor.cursor, 6);
    }

    #[test]
    fn p_pastes_charwise_after_cursor() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("abc", 1);
        let mut clipboard = "XY".to_string();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        state.clipboard_kind = crate::types::ClipboardKind::CharWise;

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        );
        // Paste after cursor char 'b' (pos 1) → inserts at pos 2
        assert_eq!(editor.content, "abXYc");
    }

    #[test]
    fn p_pastes_linewise_after_current_line() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("one\nthree", 0);
        let mut clipboard = "two\n".to_string();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        state.clipboard_kind = crate::types::ClipboardKind::LineWise;

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "one\ntwo\nthree");
    }

    #[test]
    fn y_then_p_yanks_and_pastes_line() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("one\ntwo\nthree", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        // Yank current line
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE),
        );
        assert_eq!(clipboard, "one\n");

        // Paste after
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "one\none\ntwo\nthree");
    }

    #[test]
    fn d_deletes_to_line_end() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello world", 5);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "hello");
        assert_eq!(editor.cursor, 5);
    }

    #[test]
    fn w_moves_to_next_word_start() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello world foo", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 6);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 12);
    }

    #[test]
    fn b_moves_to_prev_word_start() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello world foo", 12);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 6);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        );
        assert_eq!(editor.cursor, 0);
    }

    #[test]
    fn indent_adds_whitespace() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello\nworld", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('>'), KeyModifiers::NONE),
        );
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('>'), KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "    hello\nworld");
    }

    #[test]
    fn ciw_changes_inner_word() {
        let mut state = VimState::new(false);
        let mut editor = TestEditor::new("hello world", 0);
        let mut clipboard = String::new();
        enable_normal_mode(&mut state, &mut editor, &mut clipboard);

        // ciw
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        );
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        );
        // Now in insert mode, type replacement
        editor.insert_text("hi");
        let _ = handle_key(
            &mut state,
            &mut editor,
            &mut clipboard,
            &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(editor.content, "hi world");
    }
}
