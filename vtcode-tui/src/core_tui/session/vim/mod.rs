use ratatui::crossterm::event::KeyEvent;
use vtcode_vim::{Editor as VimEditor, handle_key};

use super::Session;
use super::input_manager::InputManager;

pub(crate) use vtcode_vim::VimState;

struct SessionVimEditor<'a> {
    input_manager: &'a mut InputManager,
}

impl VimEditor for SessionVimEditor<'_> {
    fn content(&self) -> &str {
        self.input_manager.content()
    }

    fn cursor(&self) -> usize {
        self.input_manager.cursor()
    }

    fn set_cursor(&mut self, pos: usize) {
        self.input_manager.set_cursor(pos);
    }

    fn move_left(&mut self) {
        self.input_manager.move_cursor_left();
    }

    fn move_right(&mut self) {
        self.input_manager.move_cursor_right();
    }

    fn delete_char_forward(&mut self) {
        self.input_manager.delete();
    }

    fn insert_text(&mut self, text: &str) {
        self.input_manager.insert_text(text);
    }

    fn replace(&mut self, content: String, cursor: usize) {
        self.input_manager.set_content(content);
        self.input_manager.set_cursor(cursor);
    }
}

impl Session {
    pub(crate) fn handle_vim_key(&mut self, key: &KeyEvent) -> bool {
        if !self.vim_state.enabled() || !self.input_enabled {
            return false;
        }

        let outcome = {
            let mut editor = SessionVimEditor {
                input_manager: &mut self.input_manager,
            };
            handle_key(&mut self.vim_state, &mut editor, &mut self.clipboard, key)
        };

        if !outcome.handled {
            return false;
        }

        if outcome.clear_selection {
            self.input_manager.clear_selection();
        }
        self.refresh_input_edit_state();
        self.mark_dirty();
        true
    }
}
