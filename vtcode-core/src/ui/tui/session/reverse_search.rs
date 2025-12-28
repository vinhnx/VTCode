use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::input_manager::InputManager;

#[derive(Debug, Clone)]
pub struct ReverseSearchState {
    pub active: bool,
    pub search_term: String,
    pub search_position: usize,  // Position in history where search started
    pub original_content: String, // Content before search started
    pub original_cursor: usize,   // Cursor position before search started
    pub matches: Vec<(usize, String)>, // (index, matching command)
    pub current_match_index: usize, // Current position in matches
}

impl Default for ReverseSearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl ReverseSearchState {
    pub fn new() -> Self {
        Self {
            active: false,
            search_term: String::new(),
            search_position: 0,
            original_content: String::new(),
            original_cursor: 0,
            matches: Vec::new(),
            current_match_index: 0,
        }
    }

    pub fn start_search(&mut self, input_manager: &InputManager, history: &[String]) {
        self.active = true;
        self.search_term = String::new();
        self.original_content = input_manager.content().to_string();
        self.original_cursor = input_manager.cursor();
        self.search_position = history.len();
        self.matches = Vec::new();
        self.current_match_index = 0;
    }

    pub fn cancel_search(&mut self, input_manager: &mut InputManager) {
        self.active = false;
        self.search_term.clear();
        input_manager.set_content(self.original_content.clone());
        input_manager.set_cursor(self.original_cursor);
        self.matches.clear();
    }

    pub fn accept_search(&mut self, input_manager: &mut InputManager) {
        if let Some((_, command)) = self.matches.get(self.current_match_index) {
            input_manager.set_content(command.clone());
            input_manager.set_cursor(command.len());
        }
        self.active = false;
        self.search_term.clear();
        self.matches.clear();
    }

    pub fn update_search(&mut self, history: &[String]) {
        self.matches.clear();
        
        // Search backwards through history for commands containing the search term
        for (i, command) in history.iter().enumerate().rev() {
            if command.to_lowercase().contains(&self.search_term.to_lowercase()) {
                self.matches.push((history.len() - 1 - i, command.clone()));
            }
        }
        
        self.current_match_index = 0;
    }

    pub fn cycle_backward(&mut self) {
        if !self.matches.is_empty() {
            if self.current_match_index == 0 {
                self.current_match_index = self.matches.len() - 1;
            } else {
                self.current_match_index -= 1;
            }
        }
    }

    pub fn cycle_forward(&mut self) {
        if !self.matches.is_empty() {
            self.current_match_index = (self.current_match_index + 1) % self.matches.len();
        }
    }

    pub fn add_char(&mut self, ch: char, history: &[String]) {
        self.search_term.push(ch);
        self.update_search(history);
    }

    pub fn backspace(&mut self, history: &[String]) {
        if !self.search_term.is_empty() {
            self.search_term.pop();
            self.update_search(history);
        }
    }
}

pub fn handle_reverse_search_key(
    key: &KeyEvent,
    reverse_search_state: &mut ReverseSearchState,
    input_manager: &mut InputManager,
    history: &[String],
) -> bool {
    match key.code {
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+R pressed while already in reverse search - cycle to previous match
            if reverse_search_state.active {
                reverse_search_state.cycle_backward();
                return true;
            }
            false
        }
        KeyCode::Char(ch) => {
            if reverse_search_state.active {
                reverse_search_state.add_char(ch, history);
                true
            } else {
                false
            }
        }
        KeyCode::Backspace => {
            if reverse_search_state.active {
                reverse_search_state.backspace(history);
                true
            } else {
                false
            }
        }
        KeyCode::Enter => {
            if reverse_search_state.active {
                reverse_search_state.accept_search(input_manager);
                true
            } else {
                false
            }
        }
        KeyCode::Esc => {
            if reverse_search_state.active {
                reverse_search_state.cancel_search(input_manager);
                true
            } else {
                false
            }
        }
        KeyCode::Up => {
            if reverse_search_state.active {
                reverse_search_state.cycle_backward();
                true
            } else {
                false
            }
        }
        KeyCode::Down => {
            if reverse_search_state.active {
                reverse_search_state.cycle_forward();
                true
            } else {
                false
            }
        }
        _ => false,
    }
}