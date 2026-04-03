use super::*;
use ratatui::crossterm::event::KeyModifiers;
use std::sync::Arc;

use super::super::types::{
    ContentPart, DiffPreviewMode, InlineTextStyle, TransientEvent, TransientSelectionChange,
    TransientSubmission,
};
use crate::core_tui::app::session::transient::TransientSurface;
use crate::core_tui::app::types::InlineMessageKind;
use crate::core_tui::session::modal::{ModalKeyModifiers, ModalListKeyResult};
use crate::core_tui::session::reverse_search;
use crate::core_tui::types::InlineSegment;

fn input_history_entries(session: &Session) -> Vec<(String, Vec<ContentPart>)> {
    session
        .core
        .input_manager
        .history()
        .iter()
        .map(|entry| (entry.content().to_string(), entry.attachment_elements()))
        .collect()
}

pub(super) fn handle_paste(session: &mut Session, content: &str) {
    if let Some(review) = session.transcript_review_state_mut()
        && review.search_active()
    {
        review.insert_search_text(content);
        session.mark_dirty();
    } else if session.core.input_enabled() {
        session.insert_paste_text(content);
        session.update_input_triggers();
        session.mark_dirty();
    } else if session.history_picker_visible() {
        let history = input_history_entries(session);
        session.history_picker_state.search_query.push_str(content);
        session.history_picker_state.update_search(&history);
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
    if has_command && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
        if session.core.input_manager.copy_selected_text_to_clipboard() {
            session.mark_dirty();
        }
        return true;
    }

    let is_copy_shortcut = match key.code {
        KeyCode::Char('c') | KeyCode::Char('C') => key.modifiers.contains(KeyModifiers::CONTROL),
        KeyCode::Char('\u{3}') => true,
        _ => false,
    };

    if !is_copy_shortcut {
        return false;
    }

    if session.core.input_manager.copy_selected_text_to_clipboard() {
        session.mark_dirty();
        return true;
    }

    false
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
            return Some(InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Hotkey(action.into()),
            )));
        }

        let result = modal.handle_list_key_event(&key, modal_modifiers);

        match result {
            ModalListKeyResult::Redraw => {
                session.mark_dirty();
                return None;
            }
            ModalListKeyResult::Emit(event) => {
                session.mark_dirty();
                return Some(event.into());
            }
            ModalListKeyResult::HandledNoRedraw => {
                return None;
            }
            ModalListKeyResult::Submit(event) | ModalListKeyResult::Cancel(event) => {
                session.close_overlay();
                return Some(event.into());
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
                return Some(event.into());
            }
            ModalListKeyResult::HandledNoRedraw => {
                return None;
            }
            ModalListKeyResult::Submit(event) => {
                session.close_overlay();
                return Some(event.into());
            }
            ModalListKeyResult::Cancel(event) => {
                session.close_overlay();
                return Some(event.into());
            }
            ModalListKeyResult::NotHandled => {}
        }
    }

    match session.handle_local_agents_key(&key) {
        local_agents::LocalAgentsKeyResult::Emit(event) => return Some(event),
        local_agents::LocalAgentsKeyResult::Handled => return None,
        local_agents::LocalAgentsKeyResult::NotHandled => {}
    }

    if session.inline_lists_visible() && session.handle_agent_palette_key(&key) {
        return None;
    }

    if session.inline_lists_visible() && session.handle_file_palette_key(&key) {
        return None;
    }

    if slash::try_handle_slash_navigation(session, &key, has_control, has_alt, has_command) {
        return None;
    }

    match handle_transcript_review_key(session, &key, has_control, has_alt, has_command) {
        TranscriptReviewKeyResult::Emit(event) => return Some(event),
        TranscriptReviewKeyResult::Handled => return None,
        TranscriptReviewKeyResult::NotHandled => {}
    }

    if let Some(event) = handle_diff_preview_key(session, &key) {
        return Some(event);
    }

    // Handle history picker (Ctrl+R) - Visual fuzzy search for command history
    if has_control
        && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
        && !session.history_picker_visible()
    {
        open_history_picker(session);
        return None;
    }

    // Handle history picker if active
    if session.inline_lists_visible() && session.history_picker_visible() {
        let history = input_history_entries(session);
        let was_active = session.history_picker_visible();
        let handled = history_picker::handle_history_picker_key(
            &key,
            &mut session.history_picker_state,
            &mut session.core.input_manager,
            &history,
        );
        if handled {
            session.finish_history_picker_interaction(was_active);
            session.mark_dirty();
            return None;
        }
    }

    if session.handle_vim_key(&key) {
        return None;
    }

    if is_inline_lists_toggle_shortcut(&key, has_control, has_alt, has_command) {
        session.toggle_inline_lists_visibility();
        return None;
    }

    // Legacy reverse search handling (kept for backward compatibility)
    // Handle reverse search (Ctrl+R) - disabled in favor of history picker
    // if has_control && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R')) {
    //     if !session.core.reverse_search_state.active {
    //         session.core.reverse_search_state.start_search(
    //             &session.core.input_manager,
    //             &session.core.input_manager.history_texts(),
    //         );
    //         session.mark_dirty();
    //         return None;
    //     }
    // }

    // Handle reverse search if active (legacy)
    if session.core.reverse_search_state.active {
        // Get history first to avoid borrow conflicts
        let history = session.core.input_manager.history_texts();
        let handled = reverse_search::handle_reverse_search_key(
            &key,
            &mut session.core.reverse_search_state,
            &mut session.core.input_manager,
            &history,
        );
        if handled {
            session.mark_dirty();
            return None;
        }
    }

    match key.code {
        KeyCode::Char('c') | KeyCode::Char('C') if has_control => {
            if session.core.mouse_selection.has_selection {
                session.core.mouse_selection.request_copy();
                session.mark_dirty();
                return None;
            }
            if session.has_active_overlay() {
                session.close_overlay();
            }
            session.mark_dirty();
            Some(InlineEvent::Interrupt)
        }
        KeyCode::Char('\u{3}') => {
            if session.core.mouse_selection.has_selection {
                session.core.mouse_selection.request_copy();
                session.mark_dirty();
                return None;
            }
            if session.has_active_overlay() {
                session.close_overlay();
            }
            session.mark_dirty();
            Some(InlineEvent::Interrupt)
        }
        KeyCode::Char('d') if has_control => {
            session.mark_dirty();
            Some(InlineEvent::Exit)
        }
        KeyCode::Char('b') if has_control => {
            // Ctrl+B - Background current operation or move to background
            session.mark_dirty();
            Some(InlineEvent::BackgroundOperation)
        }
        KeyCode::Char('s') | KeyCode::Char('S') if has_alt && !has_control && !has_command => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/subprocesses".to_string()))
        }
        KeyCode::Char('a') | KeyCode::Char('A') if has_control && !has_command && !has_alt => {
            if session.core.input_enabled() {
                session.move_to_start();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('e') | KeyCode::Char('E') if has_control && !has_command && !has_alt => {
            if !session.core.input_enabled() {
                None
            } else if session.core.input_manager.content().is_empty() {
                session.mark_dirty();
                Some(InlineEvent::LaunchEditor)
            } else {
                session.move_to_end();
                session.mark_dirty();
                None
            }
        }
        KeyCode::Char('w') | KeyCode::Char('W') if has_control && !has_command && !has_alt => {
            if session.core.input_enabled() {
                session.delete_word_backward();
                session.update_input_triggers();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('u') | KeyCode::Char('U') if has_control && !has_command && !has_alt => {
            if session.core.input_enabled() {
                session.delete_to_start_of_line();
                session.update_input_triggers();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('k') | KeyCode::Char('K') if has_control && !has_command && !has_alt => {
            if session.core.input_enabled() {
                session.delete_to_end_of_line();
                session.update_input_triggers();
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
        KeyCode::Char('l') | KeyCode::Char('L') if has_control => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/clear".to_string()))
        }
        KeyCode::BackTab => {
            // Shift+Tab: Toggle editing mode
            session.clear_inline_prompt_suggestion();
            session.mark_dirty();
            Some(InlineEvent::ToggleMode)
        }
        KeyCode::Esc => {
            if session.has_active_overlay() {
                session.close_overlay();
                None
            } else {
                let is_double_escape = session.core.input_manager.check_escape_double_tap();
                let active_pty_count = session.active_pty_session_count();
                let has_running_activity = session.is_running_activity();

                if has_running_activity || active_pty_count > 0 {
                    session.mark_dirty();
                    if is_double_escape {
                        Some(InlineEvent::Exit)
                    } else {
                        Some(InlineEvent::Interrupt)
                    }
                } else if is_double_escape && !has_running_activity {
                    // Double-escape while idle rewinds to the latest checkpoint.
                    session.mark_dirty();
                    Some(InlineEvent::Submit("/rewind".to_string()))
                } else if !session.core.input_manager.content().is_empty() {
                    // Single escape with content: clear input
                    session
                        .core
                        .handle_command(crate::core_tui::types::InlineCommand::ClearInput);
                    session.mark_dirty();
                    None
                } else {
                    // Single escape with no content: cancel
                    session.mark_dirty();
                    Some(InlineEvent::Cancel)
                }
            }
        }
        KeyCode::PageUp => {
            session.scroll_page_up();
            session.mark_dirty();
            Some(InlineEvent::ScrollPageUp)
        }
        KeyCode::PageDown => {
            session.scroll_page_down();
            session.mark_dirty();
            Some(InlineEvent::ScrollPageDown)
        }
        KeyCode::Home if has_control && session.core.fullscreen.active => {
            session.scroll_to_top();
            session.mark_dirty();
            None
        }
        KeyCode::End if has_control && session.core.fullscreen.active => {
            session.scroll_to_bottom();
            session.mark_dirty();
            None
        }
        KeyCode::Up => {
            let edit_queue_modifier = has_alt || (raw_meta && !has_super);
            if edit_queue_modifier && !session.core.queued_inputs.is_empty() {
                if let Some(latest) = session.pop_latest_queued_input() {
                    session.clear_inline_prompt_suggestion();
                    session.core.input_manager.set_content(latest);
                    session
                        .core
                        .set_input_compact_mode(session.input_compact_placeholder().is_some());
                    session.core.scroll_manager.set_offset(0);
                    slash::update_slash_suggestions(session);
                }
                session.mark_dirty();
                Some(InlineEvent::EditQueue)
            } else if session.navigate_history_previous() {
                session.mark_dirty();
                Some(InlineEvent::HistoryPrevious)
            } else {
                None
            }
        }
        KeyCode::Down => {
            if session.should_open_local_agents_with_down(&key, has_control, has_alt, has_command) {
                session.show_transient_surface(TransientSurface::LocalAgents);
                session.mark_dirty();
                return None;
            }
            if session.navigate_history_next() {
                session.clear_inline_prompt_suggestion();
                session.mark_dirty();
                Some(InlineEvent::HistoryNext)
            } else {
                None
            }
        }
        KeyCode::Enter => {
            if !session.core.input_enabled() {
                return None;
            }

            if session.file_palette_visible() {
                if let Some(palette) = session.file_palette.as_ref()
                    && let Some(entry) = palette.get_selected()
                {
                    let file_path = entry.path.clone();
                    session.insert_file_reference(&file_path);
                    session.close_file_palette();
                    session.mark_dirty();
                    return Some(InlineEvent::FileSelected(file_path));
                }
                return None;
            }

            if !has_control && let Some(event) = maybe_handle_busy_steering_command(session) {
                return Some(event);
            }

            if !has_control && handle_running_slash_command_block(session) {
                return None;
            }

            if !has_control
                && !has_shift
                && !has_alt
                && session.core.input_manager.content().trim().is_empty()
                && session.active_pty_session_count() > 0
            {
                session.mark_dirty();
                return Some(InlineEvent::Submit("/jobs".to_string()));
            }

            // Check for backslash + Enter quick escape (insert newline without submitting)
            if !has_control && session.core.input_manager.content().ends_with('\\') {
                // Remove the backslash and insert a newline
                let mut content = session.core.input_manager.content().to_string();
                content.pop(); // Remove the backslash
                content.push('\n');
                session.core.input_manager.set_content(content);
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
                // Insert newline for multiline input
                session.insert_char('\n');
                session.mark_dirty();
                return None;
            }

            let should_submit_now = slash::should_submit_immediately_from_palette(session);
            let Some(submitted) = take_submitted_input(session) else {
                session.mark_dirty();
                return None;
            };

            session.mark_dirty();

            if should_submit_now {
                return Some(InlineEvent::Submit(submitted));
            }

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
            if !session.core.input_enabled() {
                return None;
            }

            if session.accept_inline_prompt_suggestion() {
                session.update_input_triggers();
                return None;
            }

            if handle_running_slash_command_block(session) {
                return None;
            }

            let Some(submitted) = take_submitted_input(session) else {
                session.mark_dirty();
                return None;
            };
            session.push_queued_input(submitted.clone());
            session.mark_dirty();
            Some(InlineEvent::QueueSubmit(submitted))
        }
        KeyCode::Backspace => {
            if session.core.input_enabled() {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.delete_to_start_of_line();
                } else {
                    session.delete_char();
                }
                session.update_input_triggers();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Delete => {
            if session.core.input_enabled() {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.delete_to_end_of_line();
                } else {
                    session.delete_char_forward();
                }
                session.update_input_triggers();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Left => {
            if session.core.input_enabled() {
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
            if session.core.input_enabled() {
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
            None // Right arrow never triggers any event, including editor launch
        }
        KeyCode::Home => {
            if session.core.input_enabled() {
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
            if session.core.input_enabled() {
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
        KeyCode::Char('t') | KeyCode::Char('T') if has_control => {
            session.toggle_logs();
            None
        }
        KeyCode::Char(ch) => {
            if !session.core.input_enabled() {
                return None;
            }

            if has_alt && matches!(ch, 'p' | 'P') {
                session.clear_inline_prompt_suggestion();
                session.mark_dirty();
                return Some(InlineEvent::RequestInlinePromptSuggestion(
                    session.core.input_manager.content().to_string(),
                ));
            }

            if ch == '?'
                && !has_control
                && !has_alt
                && !has_command
                && session.core.input_manager.content().is_empty()
            {
                session.show_modal("Keyboard Shortcuts".to_string(), quick_help_lines(), None);
                return None;
            }

            if ch == '\t' {
                if session.accept_inline_prompt_suggestion() {
                    session.update_input_triggers();
                    return None;
                }
                let Some(submitted) = take_submitted_input(session) else {
                    session.mark_dirty();
                    return None;
                };
                session.push_queued_input(submitted.clone());
                session.mark_dirty();
                return Some(InlineEvent::QueueSubmit(submitted));
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
                session.update_input_triggers();
                session.mark_dirty();
            }
            None
        }
        _ => None,
    }
}

pub(super) fn open_history_picker(session: &mut Session) {
    if session.history_picker_state.active {
        return;
    }

    session.ensure_inline_lists_visible_for_trigger();
    session.show_transient_surface(TransientSurface::HistoryPicker);
    session
        .history_picker_state
        .open(&session.core.input_manager);
    let history = input_history_entries(session);
    session.history_picker_state.update_search(&history);
    session.mark_dirty();
}

fn is_inline_lists_toggle_shortcut(
    key: &KeyEvent,
    has_control: bool,
    has_alt: bool,
    has_command: bool,
) -> bool {
    if !has_control || has_alt || has_command {
        return false;
    }

    matches!(
        key.code,
        KeyCode::Char('i')
            | KeyCode::Char('I')
            | KeyCode::Char('/')
            | KeyCode::Char('?')
            | KeyCode::Char('\u{1f}')
    )
}

fn quick_help_lines() -> Vec<String> {
    vec![
        "Enter queues; Tab queues or accepts an inline suggestion.".to_string(),
        "Alt+P: Generate an inline prompt suggestion.".to_string(),
        "Ctrl+Enter: Run now while idle, or steer the active task.".to_string(),
        "Shift+Enter: Insert a newline.".to_string(),
        "/vim: Toggle Vim-style prompt editing.".to_string(),
        "Ctrl+A / Ctrl+E: Move to start/end of line.".to_string(),
        "Ctrl+W: Delete previous word.".to_string(),
        "Ctrl+U / Ctrl+K: Delete to start/end of line.".to_string(),
        "Ctrl+I or Ctrl+/: Toggle inline lists.".to_string(),
        "Alt+Left / Alt+Right: Move by word.".to_string(),
        "Ctrl+Home / Ctrl+End: Jump transcript to top or bottom in fullscreen.".to_string(),
        "Ctrl+O: Open fullscreen transcript review.".to_string(),
        "Ctrl+Z (Unix): Suspend VT Code; use `fg` to resume.".to_string(),
        "Esc: Close this overlay.".to_string(),
    ]
}

enum TranscriptReviewKeyResult {
    NotHandled,
    Handled,
    Emit(InlineEvent),
}

fn handle_transcript_review_key(
    session: &mut Session,
    key: &KeyEvent,
    has_control: bool,
    has_alt: bool,
    has_command: bool,
) -> TranscriptReviewKeyResult {
    let open_shortcut = has_control
        && !has_alt
        && !has_command
        && matches!(key.code, KeyCode::Char('o') | KeyCode::Char('O'));
    if session.transcript_review_state().is_none() {
        if !session.core.fullscreen.active || !open_shortcut {
            return TranscriptReviewKeyResult::NotHandled;
        }

        let width = session.core.transcript_width.max(1);
        let height = session.core.transcript_rows.max(1);
        session.open_transcript_review(width, height);
        return TranscriptReviewKeyResult::Handled;
    }

    if open_shortcut {
        session.close_transcript_review();
        return TranscriptReviewKeyResult::Handled;
    }

    let review_copy_shortcut = matches!(
        key.code,
        KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Char('\u{3}')
    ) && has_control;
    if review_copy_shortcut && session.core.mouse_selection.has_selection {
        session.core.mouse_selection.request_copy();
        session.mark_dirty();
        return TranscriptReviewKeyResult::Handled;
    }

    let viewport_height = session.core.transcript_rows.max(1);
    let Some(review) = session.transcript_review_state_mut() else {
        return TranscriptReviewKeyResult::Handled;
    };

    if review.search_active() {
        match key.code {
            KeyCode::Esc => {
                review.cancel_search();
                session.mark_dirty();
                return TranscriptReviewKeyResult::Handled;
            }
            KeyCode::Enter => {
                review.commit_search(viewport_height);
                session.mark_dirty();
                return TranscriptReviewKeyResult::Handled;
            }
            KeyCode::Backspace => {
                review.backspace_search();
                session.mark_dirty();
                return TranscriptReviewKeyResult::Handled;
            }
            KeyCode::Char(ch) if !has_control && !has_alt && !has_command => {
                review.insert_search_text(&ch.to_string());
                session.mark_dirty();
                return TranscriptReviewKeyResult::Handled;
            }
            _ => {
                return TranscriptReviewKeyResult::Handled;
            }
        }
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            session.close_transcript_review();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('/') if !has_control && !has_alt && !has_command => {
            review.start_search();
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('n') if !has_control && !has_alt && !has_command => {
            review.jump_next_match(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('N') if !has_control && !has_alt && !has_command => {
            review.jump_previous_match(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Up | KeyCode::Char('k') if !has_control && !has_alt && !has_command => {
            review.scroll_line_up(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Down | KeyCode::Char('j') if !has_control && !has_alt && !has_command => {
            review.scroll_line_down(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::PageUp => {
            review.scroll_half_page_up(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::PageDown => {
            review.scroll_half_page_down(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('u') | KeyCode::Char('U') if has_control && !has_alt && !has_command => {
            review.scroll_half_page_up(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('d') | KeyCode::Char('D') if has_control && !has_alt && !has_command => {
            review.scroll_half_page_down(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('b') | KeyCode::Char('B') if !has_alt && !has_command => {
            review.scroll_full_page_up(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('f') | KeyCode::Char('F') if has_control && !has_alt && !has_command => {
            review.scroll_full_page_down(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char(' ') if !has_control && !has_alt && !has_command => {
            review.scroll_full_page_down(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Home | KeyCode::Char('g') if !has_control && !has_alt && !has_command => {
            review.scroll_to_top();
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::End | KeyCode::Char('G') if !has_control && !has_alt && !has_command => {
            review.scroll_to_bottom(viewport_height);
            session.mark_dirty();
            TranscriptReviewKeyResult::Handled
        }
        KeyCode::Char('[') if !has_control && !has_alt && !has_command => {
            TranscriptReviewKeyResult::Emit(InlineEvent::OpenTranscriptReviewScrollback(
                review.export_text(),
            ))
        }
        KeyCode::Char('v') | KeyCode::Char('V') if !has_control && !has_alt && !has_command => {
            TranscriptReviewKeyResult::Emit(InlineEvent::OpenTranscriptReviewInEditor(
                review.export_text(),
            ))
        }
        _ => TranscriptReviewKeyResult::Handled,
    }
}

fn take_submitted_input(session: &mut Session) -> Option<String> {
    let submitted = session.core.input_manager.content().to_owned();
    let submitted_entry = session.core.input_manager.current_history_entry();
    clear_submitted_input(session);

    if submitted.trim().is_empty() {
        return None;
    }

    session.remember_submitted_input(submitted_entry);
    Some(submitted)
}

fn clear_submitted_input(session: &mut Session) {
    session.core.input_manager.clear();
    session.clear_suggested_prompt_state();
    session.clear_inline_prompt_suggestion();
    session.core.set_input_compact_mode(false);
    session.core.scroll_manager.set_offset(0);
    session.update_input_triggers();
}

fn handle_running_slash_command_block(session: &mut Session) -> bool {
    if !session.is_running_activity() {
        return false;
    }

    let Some(command_name) = extract_slash_command_name(session.core.input_manager.content())
    else {
        return false;
    };

    let message = format!(
        "'/{}' is disabled while a task is in progress. Please wait for the current task to complete before using this command.",
        command_name
    );
    session.push_line(
        InlineMessageKind::Warning,
        vec![InlineSegment {
            text: message,
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.core.request_transcript_clear();
    session.mark_dirty();
    true
}

fn maybe_handle_busy_steering_command(session: &mut Session) -> Option<InlineEvent> {
    if !session.is_running_activity() {
        return None;
    }

    let event = match extract_slash_command_name(session.core.input_manager.content()) {
        Some("stop") => InlineEvent::Interrupt,
        Some("pause") => InlineEvent::Pause,
        Some("resume") => InlineEvent::Resume,
        _ => return None,
    };

    clear_submitted_input(session);
    session.mark_dirty();
    Some(event)
}

fn extract_slash_command_name(input: &str) -> Option<&str> {
    let trimmed = input.trim_start();
    let command_input = trimmed.strip_prefix('/')?;
    let command = command_input.split_whitespace().next()?;
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

/// Emits an InlineEvent through the event channel and callback
#[inline]
pub(super) fn emit_inline_event(
    event: &InlineEvent,
    events: &UnboundedSender<InlineEvent>,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
) {
    if let Some(cb) = callback {
        cb(event);
    }
    let _ = events.send(event.clone());
}

/// Handles scroll down event from mouse input
#[inline]
#[allow(dead_code)]
pub(super) fn handle_scroll_down(
    session: &mut Session,
    events: &UnboundedSender<InlineEvent>,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
) {
    session.scroll_line_down();
    session.mark_dirty();
    emit_inline_event(&InlineEvent::ScrollLineDown, events, callback);
}

/// Handles scroll up event from mouse input
#[inline]
#[allow(dead_code)]
pub(super) fn handle_scroll_up(
    session: &mut Session,
    events: &UnboundedSender<InlineEvent>,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
) {
    session.scroll_line_up();
    session.mark_dirty();
    emit_inline_event(&InlineEvent::ScrollLineUp, events, callback);
}

#[allow(dead_code)]
pub(super) fn handle_diff_preview_key(
    session: &mut Session,
    key: &KeyEvent,
) -> Option<InlineEvent> {
    let mode = session.diff_preview_state()?.mode;

    match key.code {
        KeyCode::Tab => {
            let diff_state = session.diff_preview_state_mut()?;
            if diff_state.current_hunk + 1 < diff_state.hunk_count() {
                diff_state.current_hunk += 1;
            }
            session.mark_dirty();
            None
        }
        KeyCode::BackTab => {
            let diff_state = session.diff_preview_state_mut()?;
            if diff_state.current_hunk > 0 {
                diff_state.current_hunk -= 1;
            }
            session.mark_dirty();
            None
        }
        KeyCode::Enter => {
            session.close_diff_overlay();
            session.mark_dirty();
            Some(InlineEvent::Transient(TransientEvent::Submitted(
                match mode {
                    DiffPreviewMode::EditApproval => TransientSubmission::DiffApply,
                    DiffPreviewMode::FileConflict => TransientSubmission::DiffProceed,
                    DiffPreviewMode::ReadonlyReview => TransientSubmission::DiffAbort,
                },
            )))
        }
        KeyCode::Char('r') | KeyCode::Char('R')
            if matches!(mode, DiffPreviewMode::FileConflict) =>
        {
            session.close_diff_overlay();
            session.mark_dirty();
            Some(InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::DiffReload,
            )))
        }
        KeyCode::Esc => {
            session.close_diff_overlay();
            session.mark_dirty();
            Some(InlineEvent::Transient(TransientEvent::Submitted(
                match mode {
                    DiffPreviewMode::EditApproval => TransientSubmission::DiffReject,
                    DiffPreviewMode::FileConflict => TransientSubmission::DiffAbort,
                    DiffPreviewMode::ReadonlyReview => TransientSubmission::DiffAbort,
                },
            )))
        }
        KeyCode::Char('1') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::Once;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('2') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::Session;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('3') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::Always;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('4') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::AutoTrust;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::ui;
    use crate::core_tui::types::{InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme};
    use std::sync::Arc;

    fn build_session() -> Session {
        let mut session = Session::new(InlineTheme::default(), None, 24);
        session.core.set_fullscreen_active(true);
        session.core.apply_transcript_rows(8);
        session.core.apply_transcript_width(60);
        session
    }

    fn text_segment(text: impl Into<String>) -> InlineSegment {
        InlineSegment {
            text: text.into(),
            style: Arc::new(InlineTextStyle::default()),
        }
    }

    #[test]
    fn ctrl_o_opens_and_closes_transcript_review() {
        let mut session = build_session();
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("hello review")]);

        assert!(session.transcript_review_state().is_none());

        assert!(
            session
                .process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
                .is_none()
        );
        assert!(session.transcript_review_state().is_some());

        assert!(
            session
                .process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
                .is_none()
        );
        assert!(session.transcript_review_state().is_none());
    }

    #[test]
    fn ctrl_home_and_end_jump_transcript_in_fullscreen() {
        let mut session = build_session();
        for index in 0..40 {
            session.core.push_line(
                InlineMessageKind::Agent,
                vec![text_segment(format!("line {index}"))],
            );
        }

        session.core.scroll_page_up();
        assert!(session.core.scroll_offset() > 0);

        let _ = session.process_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
        assert_eq!(session.core.scroll_offset(), 0);

        let _ = session.process_key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL));
        assert_eq!(
            session.core.scroll_offset(),
            session.core.current_max_scroll_offset()
        );
    }

    #[test]
    fn transcript_review_search_accept_and_cancel_work() {
        let mut session = build_session();
        for line in ["alpha", "beta alpha", "gamma alpha"] {
            session
                .core
                .push_line(InlineMessageKind::Agent, vec![text_segment(line)]);
        }

        let _ = session.process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in ['a', 'l', 'p', 'h', 'a'] {
            let _ = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let _ = session.process_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        let status = session
            .transcript_review_state()
            .expect("review open")
            .status_label();
        assert!(status.contains("search 'alpha'"));
        assert!(status.contains("(2/3)"));

        let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        let _ = session.process_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        let _ = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        let status = session
            .transcript_review_state()
            .expect("review open")
            .status_label();
        assert!(status.contains("search 'alpha'"));
    }

    #[test]
    fn transcript_review_exports_expanded_collapsed_content() {
        let mut session = build_session();
        let line_total = ui::INLINE_JSON_COLLAPSE_LINE_THRESHOLD + 5;
        let payload = format!(
            "[\n{}\n]",
            (0..line_total)
                .map(|index| format!("  {{\"line\": {index}}}"))
                .collect::<Vec<_>>()
                .join(",\n")
        );
        session
            .core
            .append_pasted_message(InlineMessageKind::Tool, payload.clone(), line_total);

        let _ = session.process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));

        match session.process_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)) {
            Some(InlineEvent::OpenTranscriptReviewInEditor(text)) => {
                assert!(text.contains("\"line\": 0"));
                assert!(text.contains(&format!("\"line\": {}", line_total - 1)));
            }
            other => panic!("unexpected review editor event: {other:?}"),
        }

        match session.process_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE)) {
            Some(InlineEvent::OpenTranscriptReviewScrollback(text)) => {
                assert!(text.contains("\"line\": 0"));
                assert!(text.contains(&format!("\"line\": {}", line_total - 1)));
            }
            other => panic!("unexpected review scrollback event: {other:?}"),
        }
    }

    #[test]
    fn mouse_events_are_ignored_when_fullscreen_mouse_capture_is_disabled() {
        let mut session = build_session();
        session.core.fullscreen.interaction.mouse_capture = false;
        for index in 0..20 {
            session.core.push_line(
                InlineMessageKind::Agent,
                vec![text_segment(format!("line {index}"))],
            );
        }
        session.core.scroll_page_up();
        let initial_offset = session.core.scroll_offset();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        session.handle_event(
            CrosstermEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }),
            &tx,
            None,
        );

        assert_eq!(session.core.scroll_offset(), initial_offset);
    }
}
