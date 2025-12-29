use ratatui::crossterm::event::KeyEvent;

use super::{InlineEvent, Session};

/// Find the next occurrence of a character after the current cursor position
fn find_next_char(content: &str, cursor_pos: usize, target_char: char) -> Option<usize> {
    let content_chars: Vec<char> = content.chars().collect();
    let start_pos = cursor_pos.min(content_chars.len());

    for (i, &ch) in content_chars.iter().enumerate().skip(start_pos) {
        if ch == target_char {
            // Calculate the byte position for the cursor
            let mut byte_pos = 0;
            for (j, c) in content.chars().enumerate() {
                if j == i {
                    return Some(byte_pos);
                }
                byte_pos += c.len_utf8();
            }
        }
    }
    None
}

/// Find the previous occurrence of a character before the current cursor position
fn find_prev_char(content: &str, cursor_pos: usize, target_char: char) -> Option<usize> {
    let content_chars: Vec<char> = content.chars().collect();
    let start_pos = if cursor_pos > 0 { cursor_pos - 1 } else { 0 };
    let end_pos = start_pos.min(content_chars.len().saturating_sub(1));

    for i in (0..=end_pos).rev() {
        if content_chars[i] == target_char {
            // Calculate the byte position for the cursor
            let mut byte_pos = 0;
            for (j, c) in content.chars().enumerate() {
                if j == i {
                    return Some(byte_pos);
                }
                byte_pos += c.len_utf8();
            }
        }
    }
    None
}

/// Find the boundaries of the word at the given cursor position
fn find_word_boundaries(content: &str, cursor_pos: usize) -> (usize, usize) {
    let chars: Vec<char> = content.chars().collect();
    let mut start = cursor_pos;
    let mut end = cursor_pos;

    // Move backward to find the start of the word
    while start > 0 {
        let idx = start - 1;
        if idx < chars.len() {
            let ch = chars[idx];
            if !is_word_char(ch) {
                break;
            }
            start = idx;
        } else {
            break;
        }
    }

    // Move forward to find the end of the word
    while end < chars.len() {
        let ch = chars[end];
        if !is_word_char(ch) {
            break;
        }
        end += 1;
    }

    // Convert character indices to byte indices
    let mut byte_start = 0;
    for (i, c) in content.chars().enumerate() {
        if i == start {
            break;
        }
        byte_start += c.len_utf8();
    }

    let mut byte_end = byte_start;
    for (i, c) in content.chars().enumerate().skip(start) {
        if i >= end {
            break;
        }
        byte_end += c.len_utf8();
    }

    (byte_start, byte_end)
}

/// Check if a character is considered part of a word (vim-style word)
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Check if a character is considered part of a WORD (vim-style WORD - whitespace delimited)
fn is_big_word_char(ch: char) -> bool {
    !ch.is_whitespace()
}

/// Convert a byte index to a character index in a string
fn byte_to_char_index(content: &str, byte_idx: usize) -> usize {
    let mut char_idx = 0;
    let mut current_byte_idx = 0;

    for ch in content.chars() {
        if current_byte_idx == byte_idx {
            break;
        }
        if current_byte_idx > byte_idx {
            // If we've gone past the target byte index, return the current char index
            break;
        }
        current_byte_idx += ch.len_utf8();
        char_idx += 1;
    }

    char_idx
}

/// Handle Vim mode keybindings for the session
pub(super) fn handle_vim_mode_key(session: &mut Session, key: &KeyEvent) -> Option<InlineEvent> {
    use crate::ui::tui::session::vim_mode::VimAction;

    let vim_action = session.vim_state.handle_key_event_with_pending(key);

    match vim_action {
        VimAction::SwitchToInsert
        | VimAction::MoveToStartOfLineAndInsert
        | VimAction::MoveRightAndInsert
        | VimAction::MoveToEndOfLineAndInsert
        | VimAction::OpenLineBelowAndInsert
        | VimAction::OpenLineAboveAndInsert => {
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
            return None;
        }
        VimAction::MoveToBottom => {
            // Move to bottom of transcript
            session.scroll_to_bottom();
            session.mark_dirty();
            return None;
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
        VimAction::YankCurrentLine => {
            // Copy the entire input content to clipboard
            let content = session.input_manager.content().to_string();
            session.clipboard = content;
            session.mark_dirty();
            return None;
        }
        VimAction::YankWord => {
            // Copy the word at cursor position to clipboard
            let content = session.input_manager.content();
            let cursor_pos = session.input_manager.cursor();

            // Find the start and end of the current word
            let (start, end) = find_word_boundaries(content, cursor_pos);
            let word = content[start..end].to_string();
            session.clipboard = word;
            session.mark_dirty();
            return None;
        }
        VimAction::YankToEndOfLine => {
            // Copy from cursor to end of line to clipboard
            let content = session.input_manager.content();
            let cursor_pos = session.input_manager.cursor();

            if cursor_pos < content.len() {
                let text_from_cursor = content[cursor_pos..].to_string();
                session.clipboard = text_from_cursor;
            } else {
                session.clipboard = String::new();
            }
            session.mark_dirty();
            return None;
        }
        VimAction::FindNextChar(target_char) => {
            // Find next occurrence of target_char and move cursor to it
            let content = session.input_manager.content();
            let cursor_pos = session.input_manager.cursor();

            // Search for the character after the current cursor position
            if let Some(pos) = find_next_char(content, cursor_pos, target_char) {
                session.input_manager.set_cursor(pos);
                session.mark_dirty();
            }
            return None;
        }
        VimAction::FindPrevChar(target_char) => {
            // Find previous occurrence of target_char and move cursor to it
            let content = session.input_manager.content();
            let cursor_pos = session.input_manager.cursor();

            // Search for the character before the current cursor position
            if let Some(pos) = find_prev_char(content, cursor_pos, target_char) {
                session.input_manager.set_cursor(pos);
                session.mark_dirty();
            }
            return None;
        }
        VimAction::FindTillNextChar(target_char) => {
            // Find next occurrence of target_char and move cursor just before it
            let content = session.input_manager.content();
            let cursor_pos = session.input_manager.cursor();

            // Search for the character after the current cursor position
            if let Some(pos) = find_next_char(content, cursor_pos, target_char) {
                // Move to position just before the found character
                if pos > 0 {
                    session.input_manager.set_cursor(pos - 1);
                } else {
                    session.input_manager.set_cursor(0);
                }
                session.mark_dirty();
            }
            return None;
        }
        VimAction::FindTillPrevChar(target_char) => {
            // Find previous occurrence of target_char and move cursor just after it
            let content = session.input_manager.content();
            let cursor_pos = session.input_manager.cursor();

            // Search for the character before the current cursor position
            if let Some(pos) = find_prev_char(content, cursor_pos, target_char) {
                // Move to position just after the found character
                let new_pos = pos + 1;
                if new_pos <= content.len() {
                    session.input_manager.set_cursor(new_pos);
                } else {
                    session.input_manager.set_cursor(content.len());
                }
                session.mark_dirty();
            }
            return None;
        }
        VimAction::RepeatFind => {
            // Repeat the last f/F/t/T command in the same direction
            if let (Some(last_char), Some(direction)) = (
                session.vim_state.last_find_char,
                &session.vim_state.last_find_direction,
            ) {
                let content = session.input_manager.content();
                let cursor_pos = session.input_manager.cursor();

                match direction {
                    crate::ui::tui::session::vim_mode::FindDirection::Forward => {
                        // For f and t commands (search forward)
                        if let Some(pos) = find_next_char(content, cursor_pos, last_char) {
                            // Determine if the original command was 't' (move before) or 'f' (move to)
                            let final_pos = if matches!(session.vim_state.last_command, Some('t')) {
                                // For t command, move just before the found character
                                if pos > 0 { pos - 1 } else { 0 }
                            } else {
                                // For f command, move to the found character
                                pos
                            };
                            session.input_manager.set_cursor(final_pos);
                        }
                    }
                    crate::ui::tui::session::vim_mode::FindDirection::Backward => {
                        // For F and T commands (search backward)
                        if let Some(pos) = find_prev_char(content, cursor_pos, last_char) {
                            // Determine if the original command was 'T' (move after) or 'F' (move to)
                            let final_pos = if matches!(session.vim_state.last_command, Some('T')) {
                                // For T command, move just after the found character
                                let new_pos = pos + 1;
                                new_pos.min(content.len())
                            } else {
                                // For F command, move to the found character
                                pos
                            };
                            session.input_manager.set_cursor(final_pos);
                        }
                    }
                }
                session.mark_dirty();
            }
            return None;
        }
        VimAction::RepeatFindReverse => {
            // Repeat the last f/F/t/T command in the opposite direction
            if let (Some(last_char), Some(direction)) = (
                session.vim_state.last_find_char,
                &session.vim_state.last_find_direction,
            ) {
                let content = session.input_manager.content();
                let cursor_pos = session.input_manager.cursor();

                match direction {
                    crate::ui::tui::session::vim_mode::FindDirection::Forward => {
                        // If the original was forward (f or t), now search backward (F or T)
                        if let Some(pos) = find_prev_char(content, cursor_pos, last_char) {
                            // The original command was f or t, so we're now doing F or T
                            let final_pos = if matches!(session.vim_state.last_command, Some('t')) {
                                // Original was t, now doing T (reverse), move just after the found character
                                let new_pos = pos + 1;
                                new_pos.min(content.len())
                            } else {
                                // Original was f, now doing F (reverse), move to the found character
                                pos
                            };
                            session.input_manager.set_cursor(final_pos);
                        }
                    }
                    crate::ui::tui::session::vim_mode::FindDirection::Backward => {
                        // If the original was backward (F or T), now search forward (f or t)
                        if let Some(pos) = find_next_char(content, cursor_pos, last_char) {
                            // The original command was F or T, so we're now doing f or t
                            let final_pos = if matches!(session.vim_state.last_command, Some('T')) {
                                // Original was T, now doing t (reverse), move just before the found character
                                if pos > 0 { pos - 1 } else { 0 }
                            } else {
                                // Original was F, now doing f (reverse), move to the found character
                                pos
                            };
                            session.input_manager.set_cursor(final_pos);
                        }
                    }
                }
                session.mark_dirty();
            }
            return None;
        }
        VimAction::Paste => {
            // Paste the content from clipboard after the cursor
            let cursor_pos = session.input_manager.cursor();
            let content = session.input_manager.content();

            // Properly handle UTF-8 characters by using char indices
            let content_chars: Vec<char> = content.chars().collect();
            let clipboard_chars: Vec<char> = session.clipboard.chars().collect();

            // Split content at cursor position (in chars)
            let cursor_char_pos = byte_to_char_index(content, cursor_pos);
            let before_cursor: String = content_chars[..cursor_char_pos].iter().collect();
            let after_cursor: String = content_chars[cursor_char_pos..].iter().collect();

            // Calculate the byte position for the start of the pasted content before moving strings
            let paste_start_pos = before_cursor.chars().map(|c| c.len_utf8()).sum::<usize>();

            // Combine with clipboard content
            let mut new_content = before_cursor;
            new_content.extend(clipboard_chars);
            new_content.push_str(&after_cursor);

            session.input_manager.set_content(new_content);

            // In vim, after pasting with 'p', the cursor moves to the first character of the pasted text
            session.input_manager.set_cursor(paste_start_pos);
            session.mark_dirty();
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
