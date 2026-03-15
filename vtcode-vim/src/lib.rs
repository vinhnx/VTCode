//! Vim-style prompt editing engine shared by VT Code terminal surfaces.

mod engine;
mod text;
mod types;

pub use engine::{Editor, HandleKeyOutcome, handle_key};
pub use types::{VimMode, VimState};

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Editor, VimState, handle_key};

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
            if self.cursor == 0 {
                return;
            }
            let mut pos = self.cursor - 1;
            while pos > 0 && !self.content.is_char_boundary(pos) {
                pos -= 1;
            }
            self.cursor = pos;
        }

        fn move_right(&mut self) {
            if self.cursor >= self.content.len() {
                return;
            }
            let mut pos = self.cursor + 1;
            while pos < self.content.len() && !self.content.is_char_boundary(pos) {
                pos += 1;
            }
            self.cursor = pos;
        }

        fn delete_char_forward(&mut self) {
            if self.cursor >= self.content.len() {
                return;
            }
            let mut end = self.cursor + 1;
            while end < self.content.len() && !self.content.is_char_boundary(end) {
                end += 1;
            }
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
}
