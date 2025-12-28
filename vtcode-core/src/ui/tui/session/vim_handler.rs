use ratatui::crossterm::event::KeyEvent;

use super::{Session, InlineEvent};

/// Handle Vim mode keybindings for the session
pub(super) fn handle_vim_mode_key(session: &mut Session, key: &KeyEvent) -> Option<InlineEvent> {
    use crate::ui::tui::session::vim_mode::{VimAction, VimMode};

    let vim_action = session.vim_state.handle_key_event_with_pending(key);

    match vim_action {
        VimAction::SwitchToInsert |
        VimAction::MoveToStartOfLineAndInsert |
        VimAction::MoveRightAndInsert |
        VimAction::MoveToEndOfLineAndInsert |
        VimAction::OpenLineBelowAndInsert |
        VimAction::OpenLineAboveAndInsert => {
            // Switch to insert mode and handle the specific action
            session.vim_state.switch_to_insert();
            match vim_action {
                VimAction::MoveToStartOfLineAndInsert => {
                    session.move_to_start();
                }
                VimAction::MoveRightAndInsert => {
                    session.move_right();
                }
                VimAction::MoveToEndOfLineAndInsert => {
                    session.move_to_end();
                }
                VimAction::OpenLineBelowAndInsert => {
                    // Add a newline at the end of current line
                    let mut content = session.input_manager.content().to_string();
                    content.push('\n');
                    session.input_manager.set_content(content);
                    session.move_to_end();
                }
                VimAction::OpenLineAboveAndInsert => {
                    // Add a newline at the beginning of current line
                    let mut content = session.input_manager.content().to_string();
                    content.insert(0, '\n');
                    session.input_manager.set_content(content);
                    session.move_to_start();
                }
                _ => {} // SwitchToInsert case handled above
            }
            session.mark_dirty();
            return None;
        }
        VimAction::SwitchToNormal => {
            session.mark_dirty();
            return None;
        }
        VimAction::MoveLeft => {
            session.move_left();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveDown => {
            session.scroll_line_down();
            session.mark_dirty();
            return Some(InlineEvent::ScrollLineDown);
        }
        VimAction::MoveUp => {
            session.scroll_line_up();
            session.mark_dirty();
            return Some(InlineEvent::ScrollLineUp);
        }
        VimAction::MoveRight => {
            session.move_right();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveToNextWordStart => {
            // Move to next word start
            session.move_right_word();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveToEndOfWord => {
            // Move to end of current word
            session.move_right_word();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveToPrevWordStart => {
            // Move to previous word start
            session.move_left_word();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveToStartOfLine => {
            session.move_to_start();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveToEndOfLine => {
            session.move_to_end();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveToFirstNonBlank => {
            // Move to first non-blank character in line
            // For now, just move to start of line
            session.move_to_start();
            session.mark_dirty();
            return None;
        }
        VimAction::MoveToTop => {
            // Move to top of transcript
            session.scroll_to_top();
            session.mark_dirty();
            return Some(InlineEvent::ScrollToTop);
        }
        VimAction::MoveToBottom => {
            // Move to bottom of transcript
            session.scroll_to_bottom();
            session.mark_dirty();
            return Some(InlineEvent::ScrollToBottom);
        }
        VimAction::DeleteChar => {
            session.delete_char();
            session.mark_dirty();
            return None;
        }
        VimAction::DeleteCurrentLine => {
            // Clear the current input line
            session.input_manager.clear();
            session.mark_dirty();
            return None;
        }
        VimAction::DeleteWord => {
            session.delete_word_backward();
            session.mark_dirty();
            return None;
        }
        VimAction::DeleteToEndOfLine => {
            // Delete from cursor to end of line
            let cursor_pos = session.input_manager.cursor();
            let content = session.input_manager.content();
            if cursor_pos < content.len() {
                let before_cursor = content[..cursor_pos].to_string();
                session.input_manager.set_content(before_cursor);
                session.mark_dirty();
            }
            return None;
        }
        VimAction::None => {
            // If Vim action is None, continue with normal processing
            // This can happen when waiting for the next key in a multi-key command
            if session.vim_state.pending_command.is_some() {
                session.mark_dirty();
                return None; // Still waiting for the next key
            }
        }
        _ => {
            // Handle other Vim actions as needed
            session.mark_dirty();
            return None;
        }
    }

    None
}