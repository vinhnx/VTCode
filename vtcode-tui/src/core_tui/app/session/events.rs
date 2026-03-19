use super::*;
use ratatui::crossterm::event::{KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use std::sync::Arc;
use std::time::Instant;

use super::super::types::{
    ContentPart, DiffPreviewMode, InlineTextStyle, OverlayEvent, OverlaySelectionChange,
    OverlaySubmission,
};
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
    if session.core.input_enabled() {
        session.insert_paste_text(content);
        session.update_input_triggers();
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

#[allow(dead_code)]
pub(super) fn handle_event(
    session: &mut Session,
    event: CrosstermEvent,
    events: &UnboundedSender<InlineEvent>,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
) {
    match event {
        CrosstermEvent::Key(key) => {
            if matches!(key.kind, KeyEventKind::Press)
                && let Some(outbound) = process_key(session, key)
            {
                emit_inline_event(&outbound, events, callback);
            }
        }
        CrosstermEvent::Mouse(MouseEvent {
            kind, column, row, ..
        }) => match kind {
            MouseEventKind::ScrollDown => {
                session.core.mouse_selection.clear_click_history();
                // Check if history picker is active - delegate scrolling to picker
                if session.history_picker_state.active {
                    session.history_picker_state.move_down();
                    session.mark_dirty();
                } else {
                    session.scroll_line_down();
                    session.mark_dirty();
                }
            }
            MouseEventKind::ScrollUp => {
                session.core.mouse_selection.clear_click_history();
                // Check if history picker is active - delegate scrolling to picker
                if session.history_picker_state.active {
                    session.history_picker_state.move_up();
                    session.mark_dirty();
                } else {
                    session.scroll_line_up();
                    session.mark_dirty();
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if session
                    .core
                    .mouse_selection
                    .register_click(column, row, Instant::now())
                {
                    let _ = session.core.select_transcript_word_at(column, row);
                    session.core.mouse_selection.clear_click_history();
                } else {
                    session.core.mouse_selection.start_selection(column, row);
                }
                session.mark_dirty();
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                session.core.mouse_selection.update_selection(column, row);
                session.mark_dirty();
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                session.core.mouse_selection.finish_selection(column, row);
                session.mark_dirty();
            }
            _ => {}
        },
        CrosstermEvent::Paste(content) => {
            handle_paste(session, &content);
        }
        CrosstermEvent::Resize(_, rows) => {
            session.apply_view_rows(rows);
            session.mark_dirty();
        }
        CrosstermEvent::FocusGained => {
            // No-op: focus tracking is host/application concern.
        }
        CrosstermEvent::FocusLost => {
            // No-op: focus tracking is host/application concern.
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
                OverlaySubmission::Hotkey(action.into()),
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

    if session.inline_lists_visible() && session.handle_file_palette_key(&key) {
        return None;
    }

    if slash::try_handle_slash_navigation(session, &key, has_control, has_alt, has_command) {
        return None;
    }

    if let Some(event) = handle_diff_preview_key(session, &key) {
        return Some(event);
    }

    // Handle history picker (Ctrl+R) - Visual fuzzy search for command history
    if has_control
        && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
        && !session.history_picker_state.active
    {
        open_history_picker(session);
        return None;
    }

    // Handle history picker if active
    if session.inline_lists_visible() && session.history_picker_state.active {
        let history = input_history_entries(session);
        let was_active = session.history_picker_state.active;
        let handled = history_picker::handle_history_picker_key(
            &key,
            &mut session.history_picker_state,
            &mut session.core.input_manager,
            &history,
        );
        if handled {
            if was_active && !session.history_picker_state.active {
                session.update_input_triggers();
            }
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
            session.mark_dirty();
            Some(InlineEvent::Interrupt)
        }
        KeyCode::Char('\u{3}') => {
            if session.core.mouse_selection.has_selection {
                session.core.mouse_selection.request_copy();
                session.mark_dirty();
                return None;
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
        KeyCode::Up => {
            let edit_queue_modifier = has_alt || (raw_meta && !has_super);
            if edit_queue_modifier && !session.core.queued_inputs.is_empty() {
                if let Some(latest) = session.pop_latest_queued_input() {
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
            if session.navigate_history_next() {
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

            if session.file_palette_active {
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

            if should_submit_now {
                session.mark_dirty();
                return Some(InlineEvent::Submit(submitted));
            }

            // Note: The thinking spinner message is no longer added here.
            // Instead, it's added in session_loop.rs after the user message is displayed,
            // ensuring proper message ordering in the transcript (user message first, then spinner).

            session.push_queued_input(submitted.clone());
            session.mark_dirty();
            Some(InlineEvent::QueueSubmit(submitted))
        }
        KeyCode::Tab => {
            if !session.core.input_enabled() {
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
                session.mark_dirty();
                return Some(InlineEvent::Submit("/suggest".to_string()));
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
        "Enter / Tab: Queue the current message.".to_string(),
        "Ctrl+Enter: Run now while idle, or steer the active task.".to_string(),
        "Shift+Enter: Insert a newline.".to_string(),
        "/vim: Toggle Vim-style prompt editing.".to_string(),
        "Ctrl+A / Ctrl+E: Move to start/end of line.".to_string(),
        "Ctrl+W: Delete previous word.".to_string(),
        "Ctrl+U / Ctrl+K: Delete to start/end of line.".to_string(),
        "Ctrl+I or Ctrl+/: Toggle inline lists.".to_string(),
        "Alt+Left / Alt+Right: Move by word.".to_string(),
        "Ctrl+Z (Unix): Suspend VT Code; use `fg` to resume.".to_string(),
        "Esc: Close this overlay.".to_string(),
    ]
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
    session.core.transcript_content_changed = true;
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
            Some(InlineEvent::Overlay(OverlayEvent::Submitted(match mode {
                DiffPreviewMode::EditApproval => OverlaySubmission::DiffApply,
                DiffPreviewMode::FileConflict => OverlaySubmission::DiffProceed,
            })))
        }
        KeyCode::Char('r') | KeyCode::Char('R')
            if matches!(mode, DiffPreviewMode::FileConflict) =>
        {
            session.close_diff_overlay();
            session.mark_dirty();
            Some(InlineEvent::Overlay(OverlayEvent::Submitted(
                OverlaySubmission::DiffReload,
            )))
        }
        KeyCode::Esc => {
            session.close_diff_overlay();
            session.mark_dirty();
            Some(InlineEvent::Overlay(OverlayEvent::Submitted(match mode {
                DiffPreviewMode::EditApproval => OverlaySubmission::DiffReject,
                DiffPreviewMode::FileConflict => OverlaySubmission::DiffAbort,
            })))
        }
        KeyCode::Char('1') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::Once;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
                OverlaySelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('2') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::Session;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
                OverlaySelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('3') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::Always;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
                OverlaySelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('4') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let diff_state = session.diff_preview_state_mut()?;
            diff_state.trust_mode = crate::core_tui::app::types::TrustMode::AutoTrust;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            Some(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
                OverlaySelectionChange::DiffTrustMode { mode },
            )))
        }
        _ => None,
    }
}
