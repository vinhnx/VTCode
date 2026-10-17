//! Secure-prompt (masked secret input) key handling.

use super::*;

/// Key handler for a secure-prompt modal (text-only, masked input such as an
/// API-key entry). See `process_key_with_clipboard_image_reader` for why this
/// exists separately from the normal composer handler.
///
/// Every key is handled here — editing keys mutate the shared input manager,
/// Enter submits and closes the modal, Esc cancels and closes it, and anything
/// else is consumed so no composer shortcut leaks through while a secret is
/// being typed.
pub(crate) fn handle_secure_prompt_key(
    session: &mut Session,
    key: KeyEvent,
    has_control: bool,
    has_shift: bool,
    has_alt: bool,
    has_command: bool,
) -> Option<InlineEvent> {
    match key.code {
        KeyCode::Esc => {
            session.core.last_escape_press = None;
            session.core.input_manager.clear();
            session.close_overlay();
            session.mark_dirty();
            None
        }
        // Plain Enter and Cmd+Enter submit. Shift/Ctrl+Alt line-feed combos are
        // not meaningful for a single-line secret, so require no shift/alt/control.
        KeyCode::Enter if !has_control && !has_shift && !has_alt => {
            let submitted = session.core.input_manager.content().trim().to_string();
            if submitted.is_empty() {
                return None;
            }
            session.core.input_manager.clear();
            session.close_overlay();
            session.mark_dirty();
            Some(InlineEvent::Submit(submitted.into()))
        }
        KeyCode::Backspace => {
            if has_alt {
                session.delete_word_backward();
            } else if has_command {
                session.clear_current_line_or_all();
            } else {
                session.delete_char();
            }
            session.mark_dirty();
            None
        }
        KeyCode::Delete => {
            if has_command {
                session.delete_to_end_of_line();
            } else {
                session.delete_char_forward();
            }
            session.mark_dirty();
            None
        }
        KeyCode::Left => {
            if has_command {
                session.move_to_start_of_line();
            } else if has_alt {
                session.move_left_word();
            } else {
                session.move_left();
            }
            session.mark_dirty();
            None
        }
        KeyCode::Right => {
            if has_command {
                session.move_to_end_of_line();
            } else if has_alt {
                session.move_right_word();
            } else {
                session.move_right();
            }
            session.mark_dirty();
            None
        }
        KeyCode::Home => {
            session.move_to_start();
            session.mark_dirty();
            None
        }
        KeyCode::End => {
            session.move_to_end();
            session.mark_dirty();
            None
        }
        KeyCode::Char(ch) => {
            // Readline-style Ctrl+<ch> editing/navigation (no Alt/Cmd).
            // These double as the legacy macOS mappings for terminals that
            // encode Cmd+Left/Right/Backspace as C0 control codes (0x01, 0x05,
            // 0x15), so the line-wise operations below are shared with the
            // kitty-protocol Cmd/SUPER path in the main composer handler.
            if has_control && !has_alt && !has_command {
                match ch {
                    'a' | 'A' => {
                        session.move_to_start_of_line();
                        session.mark_dirty();
                    }
                    'e' | 'E' => {
                        session.move_to_end_of_line();
                        session.mark_dirty();
                    }
                    'b' | 'B' => {
                        session.move_left();
                        session.mark_dirty();
                    }
                    'f' | 'F' => {
                        session.move_right();
                        session.mark_dirty();
                    }
                    'w' | 'W' => {
                        session.delete_word_backward();
                        session.mark_dirty();
                    }
                    'u' | 'U' => {
                        session.clear_current_line_or_all();
                        session.mark_dirty();
                    }
                    'k' | 'K' => {
                        session.delete_to_end_of_line();
                        session.mark_dirty();
                    }
                    'h' | 'H' => {
                        // Ctrl+H is backspace on many terminals.
                        session.delete_char();
                        session.mark_dirty();
                    }
                    _ => {}
                }
                return None;
            }
            // Cmd+A clears single-line secret, Cmd+E jumps to end (mirrors composer).
            if has_command && !has_control && !has_alt {
                match ch {
                    'a' | 'A' => {
                        session.clear_current_line_or_all();
                        session.mark_dirty();
                        return None;
                    }
                    'e' | 'E' => {
                        session.move_to_end_of_line();
                        session.mark_dirty();
                        return None;
                    }
                    _ => {}
                }
            }
            // Plain character insertion (Shift is allowed — produces the shifted
            // glyph). Control characters (Tab, newline, etc.) are ignored so the
            // field stays a single-line secret.
            if !has_control && !has_alt && !has_command && !ch.is_control() {
                session.insert_char(ch);
                session.mark_dirty();
            }
            None
        }
        // Consume everything else (function keys, Tab, modifiers, …) so the
        // secure prompt stays focused and no composer shortcut fires.
        _ => None,
    }
}
