/// Vim mode implementation for VTCode
/// Supports a subset of Vim keybindings as described in the Claude Code guidelines
use ratatui::crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, PartialEq)]
pub enum VimMode {
    Normal,
    Insert,
}

#[derive(Debug, Clone)]
pub struct VimState {
    pub mode: VimMode,
    pub last_command: Option<char>,
    pub pending_command: Option<char>, // For multi-key commands like dd, gg, etc.
    pub last_find_char: Option<char>,  // For f/F/t/T repeat functionality
    pub last_find_direction: Option<FindDirection>, // Direction of last f/F/t/T command
}

#[derive(Debug, Clone, PartialEq)]
pub enum FindDirection {
    Forward,
    Backward,
}

impl Default for VimState {
    fn default() -> Self {
        Self {
            mode: VimMode::Normal, // Start in normal mode to match Vim behavior
            last_command: None,
            pending_command: None,
            last_find_char: None,
            last_find_direction: None,
        }
    }
}

impl VimState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn switch_to_normal(&mut self) {
        self.mode = VimMode::Normal;
    }

    pub fn switch_to_insert(&mut self) {
        self.mode = VimMode::Insert;
    }

    pub fn is_normal(&self) -> bool {
        matches!(self.mode, VimMode::Normal)
    }

    #[allow(dead_code)]
    pub fn is_insert(&self) -> bool {
        matches!(self.mode, VimMode::Insert)
    }

    #[allow(dead_code)]
    pub fn handle_key_event(&mut self, key: &KeyEvent) -> VimAction {
        match self.mode {
            VimMode::Normal => self.handle_normal_mode(key),
            VimMode::Insert => self.handle_insert_mode(key),
        }
    }

    pub fn handle_key_event_with_pending(&mut self, key: &KeyEvent) -> VimAction {
        // If we have a pending command, check if this key completes it
        if let Some(pending) = self.pending_command {
            let result = self.handle_pending_command(
                pending,
                Some(match key.code {
                    KeyCode::Char(c) => c,
                    _ => return VimAction::None,
                }),
            );

            // Clear the pending command after processing
            self.pending_command = None;

            if result != VimAction::None {
                return result;
            }
        }

        // Handle the key normally
        let action = match self.mode {
            VimMode::Normal => self.handle_normal_mode(key),
            VimMode::Insert => self.handle_insert_mode(key),
        };

        // If the action is a pending command, store it for the next key
        if let VimAction::PendingCommand(cmd) = action {
            self.pending_command = Some(cmd);
            return VimAction::None; // Don't process anything else yet
        }

        action
    }

    fn handle_normal_mode(&mut self, key: &KeyEvent) -> VimAction {
        match key.code {
            // Mode switching
            KeyCode::Char('i') => {
                self.switch_to_insert();
                VimAction::SwitchToInsert
            }
            KeyCode::Char('I') => {
                self.switch_to_insert();
                VimAction::MoveToStartOfLineAndInsert
            }
            KeyCode::Char('a') => {
                self.switch_to_insert();
                VimAction::MoveRightAndInsert
            }
            KeyCode::Char('A') => {
                self.switch_to_insert();
                VimAction::MoveToEndOfLineAndInsert
            }
            KeyCode::Char('o') => {
                self.switch_to_insert();
                VimAction::OpenLineBelowAndInsert
            }
            KeyCode::Char('O') => {
                self.switch_to_insert();
                VimAction::OpenLineAboveAndInsert
            }
            KeyCode::Esc => {
                self.switch_to_normal();
                VimAction::SwitchToNormal
            }

            // Navigation
            KeyCode::Char('h') => VimAction::MoveLeft,
            KeyCode::Char('j') => VimAction::MoveDown,
            KeyCode::Char('k') => VimAction::MoveUp,
            KeyCode::Char('l') => VimAction::MoveRight,
            KeyCode::Char('w') => VimAction::MoveToNextWordStart,
            KeyCode::Char('e') => VimAction::MoveToEndOfWord,
            KeyCode::Char('b') => VimAction::MoveToPrevWordStart,
            KeyCode::Char('0') => VimAction::MoveToStartOfLine,
            KeyCode::Char('$') => VimAction::MoveToEndOfLine,
            KeyCode::Char('^') => VimAction::MoveToFirstNonBlank,
            KeyCode::Char('g') => VimAction::PendingCommand('g'), // For gg
            KeyCode::Char('G') => VimAction::MoveToBottom,

            // Editing
            KeyCode::Char('x') => VimAction::DeleteChar,
            KeyCode::Char('d') => VimAction::PendingCommand('d'), // For dd, dw, etc.
            KeyCode::Char('c') => VimAction::PendingCommand('c'), // For cc, cw, etc.
            KeyCode::Char('y') => VimAction::PendingCommand('y'), // For yy, yw, etc.
            KeyCode::Char('.') => VimAction::RepeatLastCommand,

            // Additional navigation
            KeyCode::Char('f') => VimAction::PendingCommand('f'), // For f<char> - find next char
            KeyCode::Char('F') => VimAction::PendingCommand('F'), // For F<char> - find previous char
            KeyCode::Char('t') => VimAction::PendingCommand('t'), // For t<char> - move before next char
            KeyCode::Char('T') => VimAction::PendingCommand('T'), // For T<char> - move after previous char
            KeyCode::Char(';') => VimAction::RepeatFind,          // Repeat last f/F/t/T
            KeyCode::Char(',') => VimAction::RepeatFindReverse, // Repeat last f/F/t/T in opposite direction
            KeyCode::Char('p') => VimAction::Paste,             // Paste after cursor

            _ => VimAction::None,
        }
    }

    fn handle_insert_mode(&mut self, key: &KeyEvent) -> VimAction {
        match key.code {
            KeyCode::Esc => {
                self.switch_to_normal();
                VimAction::SwitchToNormal
            }
            _ => VimAction::None,
        }
    }

    pub fn handle_pending_command(&mut self, command: char, next_char: Option<char>) -> VimAction {
        match command {
            'g' => {
                if next_char == Some('g') {
                    VimAction::MoveToTop
                } else {
                    VimAction::None
                }
            }
            'd' => match next_char {
                Some('d') => VimAction::DeleteCurrentLine,
                Some('w') => VimAction::DeleteWord,
                Some('e') => VimAction::DeleteToEndOfWord,
                Some('b') => VimAction::DeleteToStartOfWord,
                Some('D') | Some('$') => VimAction::DeleteToEndOfLine,
                _ => VimAction::None,
            },
            'c' => match next_char {
                Some('c') => VimAction::ChangeCurrentLine,
                Some('w') => VimAction::ChangeWord,
                Some('e') => VimAction::ChangeToEndOfWord,
                Some('b') => VimAction::ChangeToStartOfWord,
                Some('C') | Some('$') => VimAction::ChangeToEndOfLine,
                _ => VimAction::None,
            },
            'y' => match next_char {
                Some('y') => VimAction::YankCurrentLine,
                Some('w') => VimAction::YankWord,
                Some('$') => VimAction::YankToEndOfLine,
                _ => VimAction::None,
            },
            'f' => {
                // f<char> - move to next occurrence of char
                if let Some(target_char) = next_char {
                    // Store the character and direction for repeat functionality
                    self.last_find_char = Some(target_char);
                    self.last_find_direction = Some(FindDirection::Forward);
                    self.last_command = Some('f'); // Store command type for repeat functionality
                    VimAction::FindNextChar(target_char)
                } else {
                    VimAction::None
                }
            }
            'F' => {
                // F<char> - move to previous occurrence of char
                if let Some(target_char) = next_char {
                    // Store the character and direction for repeat functionality
                    self.last_find_char = Some(target_char);
                    self.last_find_direction = Some(FindDirection::Backward);
                    self.last_command = Some('F'); // Store command type for repeat functionality
                    VimAction::FindPrevChar(target_char)
                } else {
                    VimAction::None
                }
            }
            't' => {
                // t<char> - move to just before next occurrence of char
                if let Some(target_char) = next_char {
                    // Store the character and direction for repeat functionality
                    self.last_find_char = Some(target_char);
                    self.last_find_direction = Some(FindDirection::Forward);
                    self.last_command = Some('t'); // Store command type for repeat functionality
                    VimAction::FindTillNextChar(target_char)
                } else {
                    VimAction::None
                }
            }
            'T' => {
                // T<char> - move to just after previous occurrence of char
                if let Some(target_char) = next_char {
                    // Store the character and direction for repeat functionality
                    self.last_find_char = Some(target_char);
                    self.last_find_direction = Some(FindDirection::Backward);
                    self.last_command = Some('T'); // Store command type for repeat functionality
                    VimAction::FindTillPrevChar(target_char)
                } else {
                    VimAction::None
                }
            }
            _ => VimAction::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VimAction {
    None,
    SwitchToInsert,
    SwitchToNormal,
    MoveToStartOfLineAndInsert,
    MoveRightAndInsert,
    MoveToEndOfLineAndInsert,
    OpenLineBelowAndInsert,
    OpenLineAboveAndInsert,
    MoveLeft,
    MoveDown,
    MoveUp,
    MoveRight,
    MoveToNextWordStart,
    MoveToEndOfWord,
    MoveToPrevWordStart,
    MoveToStartOfLine,
    MoveToEndOfLine,
    MoveToFirstNonBlank,
    MoveToTop,
    MoveToBottom,
    DeleteChar,
    DeleteCurrentLine,
    DeleteWord,
    DeleteToEndOfWord,
    DeleteToStartOfWord,
    DeleteToEndOfLine,
    ChangeCurrentLine,
    ChangeWord,
    ChangeToEndOfWord,
    ChangeToStartOfWord,
    ChangeToEndOfLine,
    RepeatLastCommand,
    PendingCommand(char), // For multi-key commands like 'dd', 'cw', etc.
    // Additional Vim actions
    YankCurrentLine,
    YankWord,
    YankToEndOfLine,
    RepeatFind,
    RepeatFindReverse,
    FindNextChar(char),     // f<char> - find next occurrence
    FindPrevChar(char),     // F<char> - find previous occurrence
    FindTillNextChar(char), // t<char> - move before next occurrence
    FindTillPrevChar(char), // T<char> - move after previous occurrence
    Paste,                  // p - paste from clipboard
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_mode() {
        let vim_state = VimState::new();
        assert_eq!(vim_state.mode, VimMode::Normal);
    }

    #[test]
    fn test_mode_switching() {
        let mut vim_state = VimState::new();

        // Start in normal mode
        assert!(vim_state.is_normal());
        assert!(!vim_state.is_insert());

        // Switch to insert
        vim_state.switch_to_insert();
        assert!(vim_state.is_insert());
        assert!(!vim_state.is_normal());

        // Switch back to normal
        vim_state.switch_to_normal();
        assert!(vim_state.is_normal());
        assert!(!vim_state.is_insert());
    }

    #[test]
    fn test_normal_mode_navigation() {
        let mut vim_state = VimState::new();

        // Test basic navigation
        let h_key = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(vim_state.handle_key_event(&h_key), VimAction::MoveLeft);

        let j_key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(vim_state.handle_key_event(&j_key), VimAction::MoveDown);

        let k_key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(vim_state.handle_key_event(&k_key), VimAction::MoveUp);

        let l_key = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE);
        assert_eq!(vim_state.handle_key_event(&l_key), VimAction::MoveRight);
    }

    #[test]
    fn test_mode_switching_keys() {
        let mut vim_state = VimState::new();

        // Test 'i' to insert mode
        let i_key = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(
            vim_state.handle_key_event(&i_key),
            VimAction::SwitchToInsert
        );
        assert!(vim_state.is_insert());

        // Back to normal with Esc
        let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            vim_state.handle_key_event(&esc_key),
            VimAction::SwitchToNormal
        );
        assert!(vim_state.is_normal());
    }
}
