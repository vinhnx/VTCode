use super::*;
use ratatui::crossterm::event::{KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};

use crate::ui::tui::session::modal::{ModalKeyModifiers, ModalListKeyResult};

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
                session.emit_inline_event(&outbound, events, callback);
            }
        }
        CrosstermEvent::Mouse(MouseEvent { kind, .. }) => match kind {
            MouseEventKind::ScrollDown => {}
            MouseEventKind::ScrollUp => {}
            _ => {}
        },
        CrosstermEvent::Paste(content) => {
            if session.input_enabled {
                session.insert_paste_text(&content);
                session.check_file_reference_trigger();
                session.check_prompt_reference_trigger();
                session.mark_dirty();
            } else if let Some(modal) = session.modal.as_mut()
                && let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut())
            {
                search.insert(&content);
                list.apply_search(&search.query);
                session.mark_dirty();
            } else if let Some(wizard) = session.wizard_modal.as_mut()
                && let Some(search) = wizard.search.as_mut()
            {
                // Paste into wizard modal search
                search.insert(&content);
                if let Some(step) = wizard.steps.get_mut(wizard.current_step) {
                    step.list.apply_search(&search.query);
                }
                session.mark_dirty();
            }
        }
        CrosstermEvent::Resize(_, rows) => {
            crate::ui::tui::session::render::apply_view_rows(session, rows);
            session.mark_dirty();
        }
        _ => {}
    }
}

pub(super) fn process_key(session: &mut Session, key: KeyEvent) -> Option<InlineEvent> {
    let modifiers = key.modifiers;
    let has_control = modifiers.contains(KeyModifiers::CONTROL);
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);
    let raw_alt = modifiers.contains(KeyModifiers::ALT);
    let raw_meta = modifiers.contains(KeyModifiers::META);
    let has_super = modifiers.contains(KeyModifiers::SUPER);
    let has_alt = raw_alt || (!has_super && raw_meta);
    let has_command = has_super || (raw_meta && !has_alt);

    if let Some(modal) = session.modal.as_mut() {
        let result = modal.handle_list_key_event(
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
            ModalListKeyResult::HandledNoRedraw => {
                return None;
            }
            ModalListKeyResult::Submit(event) | ModalListKeyResult::Cancel(event) => {
                session.close_modal();
                return Some(event);
            }
            ModalListKeyResult::NotHandled => {}
        }
    }

    if let Some(wizard) = session.wizard_modal.as_mut() {
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
            ModalListKeyResult::HandledNoRedraw => {
                return None;
            }
            ModalListKeyResult::Submit(event) | ModalListKeyResult::Cancel(event) => {
                session.close_modal();
                return Some(event);
            }
            ModalListKeyResult::NotHandled => {}
        }
    }

    if session.handle_file_palette_key(&key) {
        return None;
    }

    if session.handle_prompt_palette_key(&key) {
        return None;
    }

    if handle_config_palette_key(session, &key) {
        return None;
    }

    if crate::ui::tui::session::slash::try_handle_slash_navigation(
        session,
        &key,
        has_control,
        has_alt,
        has_command,
    ) {
        return None;
    }

    if let Some(event) = handle_diff_preview_key(session, &key) {
        return Some(event);
    }

    // Handle reverse search (Ctrl+R)
    if has_control && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R')) {
        if !session.reverse_search_state.active {
            // Start reverse search
            session.reverse_search_state.start_search(
                &session.input_manager,
                &session.input_manager.history_texts(),
            );
            session.mark_dirty();
            return None;
        }
    }

    // Handle reverse search if active
    if session.reverse_search_state.active {
        // Get history first to avoid borrow conflicts
        let history = session.input_manager.history_texts();
        let handled = crate::ui::tui::session::reverse_search::handle_reverse_search_key(
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

    match key.code {
        KeyCode::Char('c') | KeyCode::Char('C') if has_control => {
            session.mark_dirty();
            Some(InlineEvent::Interrupt)
        }
        KeyCode::Char('\u{3}') => {
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
        KeyCode::Char('j') if has_control => {
            // Ctrl+J is a line feed character, insert newline for multiline input
            session.insert_char('\n');
            session.mark_dirty();
            return None;
        }
        KeyCode::BackTab => {
            // Shift+Tab: Toggle editing mode (Edit -> Plan -> Agent -> Edit)
            session.mark_dirty();
            Some(InlineEvent::ToggleMode)
        }
        // External editor launch disabled - use /edit command instead
        // KeyCode::Char('e') | KeyCode::Char('E') if has_control && !has_command => {
        //     session.mark_dirty();
        //     Some(InlineEvent::LaunchEditor)
        // }
        KeyCode::Esc => {
            if session.modal.is_some() {
                session.close_modal();
                None
            } else {
                let is_double_escape = session.input_manager.check_escape_double_tap();
                let active_pty_count = session
                    .active_pty_sessions
                    .as_ref()
                    .map(|s| s.load(std::sync::atomic::Ordering::Relaxed))
                    .unwrap_or(0);

                if is_double_escape {
                    // Double-escape: trigger rewind functionality
                    session.mark_dirty();
                    Some(InlineEvent::Submit("/rewind".to_string()))
                } else if active_pty_count > 0 {
                    // Single escape with active PTY sessions: force cancel them
                    session.mark_dirty();
                    Some(InlineEvent::ForceCancelPtySession)
                } else if !session.input_manager.content().is_empty() {
                    // Single escape with content: clear input
                    crate::ui::tui::session::command::clear_input(session);
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
            if session.navigate_history_previous() {
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
            if !session.input_enabled {
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

            // Check for backslash + Enter quick escape (insert newline without submitting)
            if session.input_manager.content().ends_with('\\') {
                // Remove the backslash and insert a newline
                let mut content = session.input_manager.content().to_string();
                content.pop(); // Remove the backslash
                content.push('\n');
                session.input_manager.set_content(content);
                session.mark_dirty();
                return None;
            }

            // Check for multiline input options
            if has_shift || has_control || has_alt || has_command {
                // Insert newline for multiline input
                session.insert_char('\n');
                session.mark_dirty();
                return None;
            }

            let submitted = session.input_manager.content().to_owned();
            let submitted_entry = session.input_manager.current_history_entry();
            session.input_manager.clear();
            session.scroll_manager.set_offset(0);
            crate::ui::tui::session::slash::update_slash_suggestions(session);

            if submitted.trim().is_empty() {
                session.mark_dirty();
                return None;
            }

            session.remember_submitted_input(submitted_entry);

            // Note: The thinking spinner message is no longer added here.
            // Instead, it's added in session_loop.rs after the user message is displayed,
            // ensuring proper message ordering in the transcript (user message first, then spinner).

            if has_control || has_command {
                Some(InlineEvent::QueueSubmit(submitted))
            } else {
                Some(InlineEvent::Submit(submitted))
            }
        }
        KeyCode::Backspace => {
            if session.input_enabled {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.delete_sentence_backward();
                } else {
                    session.delete_char();
                }
                session.check_file_reference_trigger();
                session.check_prompt_reference_trigger();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Delete => {
            if session.input_enabled {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.delete_sentence_backward();
                } else {
                    session.delete_char_forward();
                }
                session.check_file_reference_trigger();
                session.check_prompt_reference_trigger();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Left => {
            if session.input_enabled {
                if has_command {
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
                if has_command {
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
            if session.input_enabled {
                session.move_to_start();
                session.mark_dirty();
            }
            None
        }
        KeyCode::End => {
            if session.input_enabled {
                session.move_to_end();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('t') | KeyCode::Char('T') if has_control => {
            session.toggle_logs();
            None
        }
        KeyCode::Char(ch) => {
            if !session.input_enabled {
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
                session.check_file_reference_trigger();
                session.check_prompt_reference_trigger();
                session.mark_dirty();
            }
            None
        }
        _ => None,
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
    if session.diff_preview.is_none() {
        return None;
    }

    let Some(ref mut diff_state) = session.diff_preview else {
        return None;
    };

    match key.code {
        KeyCode::Tab => {
            if diff_state.current_hunk + 1 < diff_state.hunk_count() {
                diff_state.current_hunk += 1;
            }
            session.mark_dirty();
            None
        }
        KeyCode::BackTab => {
            if diff_state.current_hunk > 0 {
                diff_state.current_hunk -= 1;
            }
            session.mark_dirty();
            None
        }
        KeyCode::Enter => {
            session.diff_preview = None;
            session.input_enabled = true;
            session.cursor_visible = true;
            session.mark_dirty();
            Some(InlineEvent::DiffPreviewApply)
        }
        KeyCode::Esc => {
            session.diff_preview = None;
            session.input_enabled = true;
            session.cursor_visible = true;
            session.mark_dirty();
            Some(InlineEvent::DiffPreviewReject)
        }
        KeyCode::Char('1') => {
            diff_state.trust_mode = crate::ui::tui::types::TrustMode::Once;
            session.mark_dirty();
            None
        }
        KeyCode::Char('2') => {
            diff_state.trust_mode = crate::ui::tui::types::TrustMode::Session;
            session.mark_dirty();
            None
        }
        KeyCode::Char('3') => {
            diff_state.trust_mode = crate::ui::tui::types::TrustMode::Always;
            session.mark_dirty();
            None
        }
        KeyCode::Char('4') => {
            diff_state.trust_mode = crate::ui::tui::types::TrustMode::AutoTrust;
            session.mark_dirty();
            None
        }
        _ => None,
    }
}

#[allow(dead_code)]
pub(super) fn handle_file_palette_key(session: &mut Session, key: &KeyEvent) -> bool {
    if !session.file_palette_active {
        return false;
    }

    let Some(palette) = session.file_palette.as_mut() else {
        return false;
    };

    match key.code {
        KeyCode::Up => {
            palette.move_selection_up();
            session.mark_dirty();
            true
        }
        KeyCode::Down => {
            palette.move_selection_down();
            session.mark_dirty();
            true
        }
        KeyCode::PageUp => {
            palette.page_up();
            session.mark_dirty();
            true
        }
        KeyCode::PageDown => {
            palette.page_down();
            session.mark_dirty();
            true
        }
        KeyCode::Home => {
            palette.move_to_first();
            session.mark_dirty();
            true
        }
        KeyCode::End => {
            palette.move_to_last();
            session.mark_dirty();
            true
        }
        KeyCode::Esc => {
            crate::ui::tui::session::command::close_file_palette(session);
            session.mark_dirty();
            true
        }
        KeyCode::Tab => {
            if let Some(entry) = palette.get_selected() {
                let path = entry.relative_path.clone();
                crate::ui::tui::session::command::insert_file_reference(session, &path);
                crate::ui::tui::session::command::close_file_palette(session);
                session.mark_dirty();
            }
            true
        }
        KeyCode::Enter => {
            if let Some(entry) = palette.get_selected() {
                let path = entry.relative_path.clone();
                crate::ui::tui::session::command::insert_file_reference(session, &path);
                crate::ui::tui::session::command::close_file_palette(session);
                session.mark_dirty();
            }
            true
        }
        _ => false,
    }
}

#[allow(dead_code)]
pub(super) fn handle_prompt_palette_key(session: &mut Session, key: &KeyEvent) -> bool {
    if !session.prompt_palette_active {
        return false;
    }

    let Some(palette) = session.prompt_palette.as_mut() else {
        return false;
    };

    match key.code {
        KeyCode::Up => {
            palette.move_selection_up();
            session.mark_dirty();
            true
        }
        KeyCode::Down => {
            palette.move_selection_down();
            session.mark_dirty();
            true
        }
        KeyCode::PageUp => {
            palette.page_up();
            session.mark_dirty();
            true
        }
        KeyCode::PageDown => {
            palette.page_down();
            session.mark_dirty();
            true
        }
        KeyCode::Home => {
            palette.move_to_first();
            session.mark_dirty();
            true
        }
        KeyCode::End => {
            palette.move_to_last();
            session.mark_dirty();
            true
        }
        KeyCode::Esc => {
            crate::ui::tui::session::command::close_prompt_palette(session);
            session.mark_dirty();
            true
        }
        KeyCode::Tab | KeyCode::Enter => {
            if let Some(entry) = palette.get_selected() {
                let prompt_name = entry.name.clone();
                crate::ui::tui::session::command::insert_prompt_reference(session, &prompt_name);
                crate::ui::tui::session::command::close_prompt_palette(session);
                session.mark_dirty();
            }
            true
        }
        _ => false,
    }
}

#[allow(dead_code)]
pub(super) fn handle_config_palette_key(session: &mut Session, key: &KeyEvent) -> bool {
    if !session.config_palette_active {
        return false;
    }

    let Some(palette) = session.config_palette.as_mut() else {
        return false;
    };

    match key.code {
        KeyCode::Up => {
            palette.move_up();
            session.mark_dirty();
            true
        }
        KeyCode::Down => {
            palette.move_down();
            session.mark_dirty();
            true
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            palette.toggle_selected();
            let config = palette.config.clone();
            if let Err(e) = palette.apply_changes() {
                tracing::error!("Failed to save config: {}", e);
            }
            session.apply_config(&config);
            session.mark_dirty();
            true
        }
        KeyCode::Left => {
            palette.adjust_numeric_val(-1);
            let config = palette.config.clone();
            if let Err(e) = palette.apply_changes() {
                tracing::error!("Failed to save config: {}", e);
            }
            session.apply_config(&config);
            session.mark_dirty();
            true
        }
        KeyCode::Right => {
            palette.adjust_numeric_val(1);
            let config = palette.config.clone();
            if let Err(e) = palette.apply_changes() {
                tracing::error!("Failed to save config: {}", e);
            }
            session.apply_config(&config);
            session.mark_dirty();
            true
        }
        KeyCode::Esc => {
            // Extract config before closing to apply changes to session
            let config = palette.config.clone();

            // Save and close
            if let Err(e) = palette.apply_changes() {
                eprintln!("Failed to save config: {}", e);
            }
            session.config_palette = None;
            session.config_palette_active = false;
            session.input_enabled = true;
            session.cursor_visible = true;

            // Apply config to session state (real-time reload)
            session.apply_config(&config);

            session.mark_dirty();
            true
        }
        _ => false,
    }
}
