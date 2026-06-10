use super::*;
use ratatui::crossterm::event::KeyModifiers;

use super::super::types::{OverlayEvent, OverlaySubmission};
use crate::tui::ui::tui::session::modal::{ModalKeyModifiers, ModalListKeyResult};

pub(super) fn handle_paste(session: &mut Session, content: &str) {
    if session.input_enabled {
        session.insert_paste_text(content);
        session.mark_dirty();
    } else if let Some(modal) = session.modal_state_mut()
        && let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut())
    {
        search.insert(content);
        list.apply_search(&search.query);
        session.mark_dirty();
    } else if let Some(wizard) = session.wizard_overlay_mut()
        && let Some(search) = wizard.search.as_mut()
    {
        search.insert(content);
        if let Some(step) = wizard.steps.get_mut(wizard.current_step) {
            step.list.apply_search(&search.query);
        }
        session.mark_dirty();
    }
}

fn copy_selected_input_if_requested(
    session: &mut Session,
    key: &KeyEvent,
    has_command: bool,
) -> bool {
    let is_copy_shortcut = if has_command {
        matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
    } else {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => {
                key.modifiers.contains(KeyModifiers::CONTROL)
            }
            KeyCode::Char('\u{3}') => true,
            _ => false,
        }
    };

    if !is_copy_shortcut {
        return false;
    }

    if session.copy_input_selection_to_clipboard() {
        session.mark_dirty();
        return true;
    }

    false
}

fn handle_interrupt(session: &mut Session) -> Option<InlineEvent> {
    if session.mouse_selection.has_selection {
        session.mouse_selection.request_copy();
        session.mark_dirty();
        return None;
    }
    if session.has_active_overlay() {
        session.close_overlay();
    }
    session.mark_dirty();
    Some(InlineEvent::Interrupt)
}

fn dispatch_action(session: &mut Session, action: Action) -> Option<InlineEvent> {
    match action {
        Action::Interrupt => handle_interrupt(session),
        Action::Exit => {
            session.mark_dirty();
            Some(InlineEvent::Exit)
        }
        Action::BackgroundOperation => {
            session.mark_dirty();
            Some(InlineEvent::BackgroundOperation)
        }
        Action::OpenModelPicker => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/model".to_string()))
        }
        Action::ClearScreen => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/clear".to_string()))
        }
        Action::ScrollPageUp => {
            session.scroll_page_up();
            session.mark_dirty();
            Some(InlineEvent::ScrollPageUp)
        }
        Action::ScrollPageDown => {
            session.scroll_page_down();
            session.mark_dirty();
            Some(InlineEvent::ScrollPageDown)
        }
        Action::EditQueue => {
            if !session.queued_inputs.is_empty() {
                if let Some(latest) = session.pop_latest_queued_input() {
                    session.clear_inline_prompt_suggestion();
                    session.input_manager.set_content(latest);
                    session.input_compact_mode = session.input_compact_placeholder().is_some();
                    session.scroll_manager.set_offset(0);
                }
                session.mark_dirty();
                Some(InlineEvent::EditQueue)
            } else {
                None
            }
        }
        Action::HistoryPrevious => {
            if session.navigate_history_previous() {
                session.mark_dirty();
                Some(InlineEvent::HistoryPrevious)
            } else {
                None
            }
        }
        Action::HistoryNext => {
            if session.navigate_history_next() {
                session.clear_inline_prompt_suggestion();
                session.mark_dirty();
                Some(InlineEvent::HistoryNext)
            } else {
                None
            }
        }
        Action::ToggleLogs => {
            session.toggle_logs();
            None
        }
        Action::GeneratePromptSuggestion => {
            if !session.input_enabled {
                return None;
            }
            session.clear_inline_prompt_suggestion();
            session.mark_dirty();
            Some(InlineEvent::RequestInlinePromptSuggestion(
                session.input_manager.content().to_string(),
            ))
        }
    }
}

pub(super) fn process_key(session: &mut Session, key: KeyEvent) -> Option<InlineEvent> {
    let modifiers = key.modifiers;
    let has_control = modifiers.contains(KeyModifiers::CONTROL);
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);
    let raw_alt = modifiers.contains(KeyModifiers::ALT);
    let raw_meta = modifiers.contains(KeyModifiers::META);
    let has_super = modifiers.contains(KeyModifiers::SUPER);
    // Command key detection: prioritize Command/Super over Alt
    // On macOS: Command = SUPER, on some terminals Alt = META
    let has_command = has_super || raw_meta;
    let has_alt = raw_alt && !has_command;

    // Allow copy-to-clipboard even when a modal is active so users can
    // copy selected transcript text without dismissing the overlay first.
    // The modal's own key handler below will still consume Ctrl+C/Esc to
    // close the overlay when no text is selected.
    if copy_selected_input_if_requested(session, &key, has_command) {
        return None;
    }

    if let Some(modal) = session.modal_state_mut() {
        let modal_modifiers = ModalKeyModifiers {
            control: has_control,
            alt: has_alt,
            command: has_command,
        };

        if let Some(action) = modal.hotkey_action(&key, modal_modifiers) {
            session.close_overlay();
            session.mark_dirty();
            return Some(InlineEvent::Overlay(OverlayEvent::Submitted(
                OverlaySubmission::Hotkey(action),
            )));
        }

        // Text-only modals (no list): close on Esc or any keypress.
        // Without a list, handle_list_key_event returns NotHandled for all keys,
        // so we must handle the close/consume logic here to prevent keys from
        // falling through to normal input processing.
        if modal.list.is_none() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    session.close_overlay();
                    session.mark_dirty();
                    return Some(InlineEvent::Overlay(OverlayEvent::Cancelled));
                }
                _ => {
                    // Consume all other key events so they don't reach the input handler
                    return None;
                }
            }
        }

        let result = modal.handle_list_key_event(&key, modal_modifiers);

        match result {
            ModalListKeyResult::Redraw => {
                session.mark_dirty();
                return None;
            }
            ModalListKeyResult::Emit(event) => {
                session.mark_dirty();
                return Some(event);
            }
            ModalListKeyResult::HandledNoRedraw => {
                return None;
            }
            ModalListKeyResult::Submit(event) | ModalListKeyResult::Cancel(event) => {
                session.close_overlay();
                return Some(event);
            }
            ModalListKeyResult::NotHandled => {}
        }
    }

    if let Some(wizard) = session.wizard_overlay_mut() {
        let result = wizard.handle_key_event(
            &key,
            ModalKeyModifiers {
                control: has_control,
                alt: has_alt,
                command: has_command,
            },
        );

        match result {
            ModalListKeyResult::Redraw => {
                session.mark_dirty();
                return None;
            }
            ModalListKeyResult::Emit(event) => {
                session.mark_dirty();
                return Some(event);
            }
            ModalListKeyResult::HandledNoRedraw => {
                return None;
            }
            ModalListKeyResult::Submit(event) => {
                session.close_overlay();
                return Some(event);
            }
            ModalListKeyResult::Cancel(event) => {
                session.close_overlay();
                return Some(event);
            }
            ModalListKeyResult::NotHandled => {}
        }
    }

    if session.handle_vim_key(&key) {
        return None;
    }

    // Handle reverse search if active (legacy)
    if session.reverse_search_state.active {
        // Get history first to avoid borrow conflicts
        let history = session.input_manager.history_texts();
        let handled = reverse_search::handle_reverse_search_key(
            &key,
            &mut session.reverse_search_state,
            &mut session.input_manager,
            &history,
        );
        if handled {
            session.mark_dirty();
            return None;
        }
    }

    // Binding store: resolve user-rebindable actions first.
    // Readline keybindings (Ctrl+F/B/P/N/T, Alt+D/T/U/L/C/\) are hardcoded
    // editing shortcuts and take priority over the binding store.
    if let Some(action) = session.bindings.resolve(&key) {
        // Skip binding store for Readline keybindings that are hardcoded below
        let is_readline_key = has_control
            && !has_command
            && !has_alt
            && matches!(
                key.code,
                KeyCode::Char('f')
                    | KeyCode::Char('F')
                    | KeyCode::Char('b')
                    | KeyCode::Char('B')
                    | KeyCode::Char('p')
                    | KeyCode::Char('P')
                    | KeyCode::Char('n')
                    | KeyCode::Char('N')
                    | KeyCode::Char('t')
                    | KeyCode::Char('T')
            );
        let is_alt_key = has_alt
            && !has_control
            && !has_command
            && matches!(
                key.code,
                KeyCode::Char('d')
                    | KeyCode::Char('D')
                    | KeyCode::Char('t')
                    | KeyCode::Char('T')
                    | KeyCode::Char('u')
                    | KeyCode::Char('U')
                    | KeyCode::Char('l')
                    | KeyCode::Char('L')
                    | KeyCode::Char('c')
                    | KeyCode::Char('C')
                    | KeyCode::Char('\\')
            );
        if !is_readline_key && !is_alt_key {
            return dispatch_action(session, action);
        }
    }

    match key.code {
        // --- Emacs-style editing shortcuts (hardcoded, not rebindable) ---
        KeyCode::Char('a') | KeyCode::Char('A') if has_control && !has_command && !has_alt => {
            if session.input_enabled {
                session.move_to_start();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('g') | KeyCode::Char('G')
            if has_control && !has_command && !has_alt && session.input_enabled =>
        {
            let draft = session.input_manager.content().to_string();
            session.mark_dirty();
            Some(InlineEvent::LaunchEditor { draft })
        }
        KeyCode::Char('w') | KeyCode::Char('W') if has_control && !has_command && !has_alt => {
            if session.input_enabled {
                session.delete_word_backward();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('u') | KeyCode::Char('U') if has_control && !has_command && !has_alt => {
            if session.input_enabled {
                session.delete_to_start_of_line();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('k') | KeyCode::Char('K') if has_control && !has_command && !has_alt => {
            if session.input_enabled {
                session.delete_to_end_of_line();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('j') if has_control => {
            // Ctrl+J is a line feed character, insert newline for multiline input
            session.insert_char('\n');
            session.mark_dirty();
            None
        }
        KeyCode::Char('z') | KeyCode::Char('Z')
            if has_control && !has_command && !has_alt && session.input_enabled =>
        {
            session.input_manager.undo();
            session.mark_dirty();
            None
        }
        KeyCode::Char('y') | KeyCode::Char('Y')
            if has_control && !has_command && !has_alt && session.input_enabled =>
        {
            session.input_manager.redo();
            session.mark_dirty();
            None
        }

        // --- Readline-style editing shortcuts ---
        KeyCode::Char('f') | KeyCode::Char('F')
            if has_control && !has_command && !has_alt && session.input_enabled =>
        {
            session.move_right();
            session.mark_dirty();
            None
        }
        KeyCode::Char('b') | KeyCode::Char('B')
            if has_control && !has_command && !has_alt && session.input_enabled =>
        {
            session.move_left();
            session.mark_dirty();
            None
        }
        KeyCode::Char('p') | KeyCode::Char('P') if has_control && !has_command && !has_alt => {
            if session.navigate_history_previous() {
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('n') | KeyCode::Char('N') if has_control && !has_command && !has_alt => {
            if session.navigate_history_next() {
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('t') | KeyCode::Char('T')
            if has_control && !has_command && !has_alt && session.input_enabled =>
        {
            session.transpose_chars();
            session.mark_dirty();
            None
        }
        KeyCode::Char('d') | KeyCode::Char('D')
            if has_alt && !has_control && !has_command && session.input_enabled =>
        {
            session.delete_word_forward();
            session.mark_dirty();
            None
        }
        KeyCode::Char('t') | KeyCode::Char('T')
            if has_alt && !has_control && !has_command && session.input_enabled =>
        {
            session.transpose_words();
            session.mark_dirty();
            None
        }
        KeyCode::Char('u') | KeyCode::Char('U')
            if has_alt && !has_control && !has_command && session.input_enabled =>
        {
            session.uppercase_word();
            session.mark_dirty();
            None
        }
        KeyCode::Char('l') | KeyCode::Char('L')
            if has_alt && !has_control && !has_command && session.input_enabled =>
        {
            session.lowercase_word();
            session.mark_dirty();
            None
        }
        KeyCode::Char('c') | KeyCode::Char('C')
            if has_alt && !has_control && !has_command && session.input_enabled =>
        {
            session.capitalize_word();
            session.mark_dirty();
            None
        }
        KeyCode::Char('\\') if has_alt && !has_control && !has_command && session.input_enabled => {
            session.delete_whitespace_around_cursor();
            session.mark_dirty();
            None
        }

        // --- Context-sensitive keys (too complex to rebind) ---
        KeyCode::Esc => {
            if session.has_active_overlay() {
                session.close_overlay();
                None
            } else if session.is_running_activity() || session.active_pty_session_count() > 0 {
                session.mark_dirty();
                Some(InlineEvent::Interrupt)
            } else if !session.input_manager.content().is_empty() {
                // Escape with content: clear input
                command::clear_input(session);
                session.mark_dirty();
                None
            } else {
                // Escape with no content: cancel
                session.mark_dirty();
                Some(InlineEvent::Cancel)
            }
        }
        KeyCode::Enter => {
            if !session.input_enabled {
                return None;
            }

            if !has_control
                && !has_shift
                && !has_alt
                && session.input_manager.content().trim().is_empty()
                && session.active_pty_session_count() > 0
            {
                session.mark_dirty();
                return Some(InlineEvent::Submit("/jobs".to_string()));
            }

            // Check for backslash + Enter quick escape (insert newline without submitting)
            if !has_control && session.input_manager.content().ends_with('\\') {
                let mut content = session.input_manager.content().to_string();
                content.pop();
                content.push('\n');
                session.input_manager.set_content(content);
                session.mark_dirty();
                return None;
            }

            if has_control {
                let Some(submitted) = take_submitted_input(session) else {
                    session.mark_dirty();
                    return if session.is_running_activity() {
                        None
                    } else {
                        Some(InlineEvent::ProcessLatestQueued)
                    };
                };
                session.mark_dirty();

                return if session.is_running_activity() {
                    Some(InlineEvent::Steer(submitted))
                } else {
                    Some(InlineEvent::Submit(submitted))
                };
            }

            // Check for multiline input options (Shift/Alt)
            if has_shift || has_alt {
                session.insert_char('\n');
                session.mark_dirty();
                return None;
            }

            let Some(submitted) = take_submitted_input(session) else {
                session.mark_dirty();
                return None;
            };

            session.mark_dirty();

            // If a turn is actively running, queue the message so it starts immediately after
            // the current turn completes. Otherwise submit directly so the turn starts now.
            if session.is_running_activity() {
                session.push_queued_input(submitted.clone());
                Some(InlineEvent::QueueSubmit(submitted))
            } else {
                Some(InlineEvent::Submit(submitted))
            }
        }
        KeyCode::Tab => {
            if !session.input_enabled {
                return None;
            }

            if session.accept_inline_prompt_suggestion() {
                return None;
            }

            if can_cycle_primary_agent(session, &key) {
                session.mark_dirty();
                return Some(InlineEvent::CyclePrimaryAgent);
            }
            None
        }
        KeyCode::BackTab => {
            if !session.input_enabled {
                return None;
            }

            session.clear_inline_prompt_suggestion();
            session.mark_dirty();
            Some(InlineEvent::CyclePrimaryAgentPrevious)
        }
        KeyCode::Backspace => {
            if session.input_enabled {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.delete_to_start_of_line();
                } else {
                    session.delete_char();
                }
                session.mark_dirty();
            }
            None
        }
        KeyCode::Delete => {
            if session.input_enabled {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.delete_to_end_of_line();
                } else {
                    session.delete_char_forward();
                }
                session.mark_dirty();
            }
            None
        }
        KeyCode::Left => {
            if session.input_enabled {
                let tmux_queue_edit = has_shift
                    && !has_control
                    && !has_command
                    && !has_alt
                    && terminal_capabilities::queued_input_edit_uses_shift_left()
                    && !session.queued_inputs.is_empty();
                if tmux_queue_edit {
                    if let Some(latest) = session.pop_latest_queued_input() {
                        session.clear_inline_prompt_suggestion();
                        session.input_manager.set_content(latest);
                        session.input_compact_mode = session.input_compact_placeholder().is_some();
                        session.scroll_manager.set_offset(0);
                    }
                    session.mark_dirty();
                    return Some(InlineEvent::EditQueue);
                }

                session.clear_inline_prompt_suggestion();
                if has_shift && has_command {
                    session.select_to_start();
                } else if has_shift {
                    session.select_left();
                } else if has_command {
                    session.move_to_start();
                } else if has_alt {
                    session.move_left_word();
                } else {
                    session.move_left();
                }
                session.mark_dirty();
            }
            None
        }
        KeyCode::Right => {
            if session.input_enabled {
                session.clear_inline_prompt_suggestion();
                if has_shift && has_command {
                    session.select_to_end();
                } else if has_shift {
                    session.select_right();
                } else if has_command {
                    session.move_to_end();
                } else if has_alt {
                    session.move_right_word();
                } else {
                    session.move_right();
                }
                session.mark_dirty();
            }
            None
        }
        KeyCode::Home => {
            if session.input_enabled {
                session.clear_inline_prompt_suggestion();
                if has_shift {
                    session.select_to_start();
                } else {
                    session.move_to_start();
                }
                session.mark_dirty();
            }
            None
        }
        KeyCode::End => {
            if session.input_enabled {
                session.clear_inline_prompt_suggestion();
                if has_shift {
                    session.select_to_end();
                } else {
                    session.move_to_end();
                }
                session.mark_dirty();
            }
            None
        }

        // --- Character input ---
        KeyCode::Char(ch) => {
            if !session.input_enabled {
                return None;
            }

            if ch == '?'
                && !has_control
                && !has_alt
                && !has_command
                && session.input_manager.content().is_empty()
            {
                session.show_help_modal();
                return None;
            }

            if ch == '\t' {
                if session.accept_inline_prompt_suggestion() {
                    return None;
                }
                if can_cycle_primary_agent(session, &key) {
                    session.mark_dirty();
                    return Some(InlineEvent::CyclePrimaryAgent);
                }
                return None;
            }

            if has_command {
                match ch {
                    'a' | 'A' => {
                        session.move_to_start();
                        session.mark_dirty();
                        return None;
                    }
                    'e' | 'E' => {
                        session.move_to_end();
                        session.mark_dirty();
                        return None;
                    }
                    _ => {}
                }
            }

            if has_alt {
                match ch {
                    'b' | 'B' => {
                        session.move_left_word();
                        session.mark_dirty();
                    }
                    'f' | 'F' => {
                        session.move_right_word();
                        session.mark_dirty();
                    }
                    _ => {}
                }
                return None;
            }

            if !has_control {
                session.insert_char(ch);
                session.mark_dirty();
            }
            None
        }
        _ => None,
    }
}

fn can_cycle_primary_agent(session: &Session, key: &KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE && !session.has_active_overlay()
}

fn take_submitted_input(session: &mut Session) -> Option<String> {
    let submitted = session.input_manager.content().to_owned();
    let submitted_entry = session.input_manager.current_history_entry();
    clear_submitted_input(session);

    if submitted.trim().is_empty() {
        return None;
    }

    session.remember_submitted_input(submitted_entry);
    Some(submitted)
}

fn clear_submitted_input(session: &mut Session) {
    session.input_manager.clear();
    session.clear_suggested_prompt_state();
    session.clear_inline_prompt_suggestion();
    session.input_compact_mode = false;
    session.scroll_manager.set_offset(0);
}
