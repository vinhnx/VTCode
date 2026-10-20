use super::*;
use ratatui::crossterm::event::KeyModifiers;
use ratatui_cheese::input::InputState;

use super::super::types::{InlineSegment, OverlayEvent, OverlaySubmission, SubmittedInput};
use crate::tui::core_tui::runner::TuiSessionDriver;
use crate::tui::core_tui::session::mode_switch_guard::{self};
use crate::tui::ui::tui::session::modal::{ModalKeyModifiers, ModalListKeyResult};

/// Shared Tab-to-queue path mirroring `Ctrl+Enter`.
///
/// Tab accepts ghost suggestions first (caller handles that), then enqueues
/// the draft: running turns get `QueueSubmit`, idle turns get `Submit`, and
/// empty drafts get `ProcessLatestQueued` (idle) or nothing (busy).
/// Plain text queued while busy is batchable so it coalesces like
/// `Ctrl+Enter`; slash stays one per turn.
fn enqueue_tab_draft(session: &mut Session) -> Option<InlineEvent> {
    let Some(submitted) = take_submitted_input(session) else {
        session.mark_dirty();
        return if session.is_running_activity() {
            None
        } else {
            Some(InlineEvent::ProcessLatestQueued)
        };
    };
    session.mark_dirty();
    if session.is_running_activity() {
        if submitted.text.trim_start().starts_with('/') {
            session.push_queued_input(submitted.text.clone());
            Some(InlineEvent::QueueSubmit(submitted))
        } else {
            session.push_queued_input(submitted.text.clone());
            Some(InlineEvent::QueueSubmit(submitted.batchable()))
        }
    } else {
        Some(InlineEvent::Submit(submitted))
    }
}

pub(super) fn handle_paste(session: &mut Session, content: &str) {
    if let Some(modal) = session.modal_state_mut() {
        if let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut()) {
            search.insert(content);
            list.apply_search(&search.query, search.fuzzy);
            session.mark_dirty();
            return;
        }
        if modal.secure_prompt.is_none() || modal.list.is_some() {
            return;
        }
    } else if let Some(wizard) = session.wizard_overlay_mut() {
        if let Some(search) = wizard.search.as_mut() {
            search.insert(content);
            if let Some(step) = wizard.steps.get_mut(wizard.current_step) {
                step.list.apply_search(&search.query, search.fuzzy);
            }
            session.mark_dirty();
            return;
        }
        // Mirror typed input: pasted text lands in the custom-note editor when
        // it is active (or when the custom-note item is selected, which typed
        // input would auto-activate).
        if let Some(step) = wizard.steps.get_mut(wizard.current_step)
            && (step.notes_active || modal::inline_editor_for_step(step).is_some())
        {
            step.notes_active = true;
            let mut state = InputState::new();
            state.set_value(step.notes.clone());
            state.end();
            for ch in content.chars().filter(|ch| !matches!(ch, '\n' | '\r')) {
                state.insert_char(ch);
            }
            step.notes = state.value().to_owned();
            session.mark_dirty();
        }
        return;
    }

    if session.input_enabled {
        session.insert_paste_text(content);
        session.mark_dirty();
    }
}

fn copy_selected_input_if_requested(session: &mut Session, key: &KeyEvent, has_command: bool) -> bool {
    // Composer selection must not pre-empt the active modal's Ctrl+C handling.
    // Transcript mouse selection is handled separately by `handle_interrupt`.
    if !session.input_enabled() {
        return false;
    }

    let is_copy_shortcut = if has_command {
        matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
    } else {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => key.modifiers.contains(KeyModifiers::CONTROL),
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
    let now = Instant::now();
    if session
        .last_interrupt_press
        .is_some_and(|last| now.duration_since(last).as_millis() < 1_000)
    {
        session.last_interrupt_press = None;
        session.request_exit();
        session.mark_dirty();
        return Some(InlineEvent::Exit);
    }
    session.last_interrupt_press = Some(now);
    if session.has_active_overlay() {
        session.close_overlay();
    }
    session.mark_dirty();
    Some(InlineEvent::Interrupt)
}

pub(crate) fn dispatch_rebindable_action(session: &mut Session, action: Action) -> Option<InlineEvent> {
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
            Some(InlineEvent::Submit("/model".into()))
        }
        Action::ClearScreen => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/clear".into()))
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
        Action::ToggleToolDisplayMode => {
            session.invalidate_transcript_cache();
            session.mark_dirty();
            Some(InlineEvent::ToggleToolDisplayMode)
        }
        // Task panel visibility lives on the AppSession layer, which dispatches
        // `ToggleTaskPanel` before the core action dispatch.
        Action::ToggleTaskPanel => None,
        // Transcript review visibility lives on the AppSession layer, which
        // handles this action before delegating to the core session.
        Action::OpenTranscriptReview => None,
        // Transcript review render mode is also owned by the AppSession layer.
        Action::ToggleTranscriptRenderMode => None,
        Action::GeneratePromptSuggestion => {
            if !session.input_enabled {
                return None;
            }
            session.clear_inline_prompt_suggestion();
            session.mark_dirty();
            Some(InlineEvent::RequestInlinePromptSuggestion(session.input_manager.content().to_string()))
        }
        Action::JumpToLastChange => {
            // Only jump while scrolled away from the change. At the live bottom
            // the legacy Ctrl+End cursor/scroll behavior wins, so a stale tracked
            // change cannot turn the key into a no-op.
            if session.should_show_jump_to_last_change() && session.jump_to_last_change() {
                Some(InlineEvent::JumpToLastChange)
            } else if session.user_scrolled {
                // Scrolled up but no live target (fresh/cleared transcript):
                // fall back to the legacy bottom jump.
                session.scroll_to_bottom();
                Some(InlineEvent::JumpToLastChange)
            } else if session.input_enabled {
                // Not scrolled and nothing to jump to: preserve legacy cursor-end
                // instead of swallowing the key.
                session.clear_inline_prompt_suggestion();
                session.move_to_end();
                session.mark_dirty();
                None
            } else {
                None
            }
        }
    }
}

pub(super) fn process_key(session: &mut Session, key: KeyEvent) -> Option<InlineEvent> {
    let key = action::normalize_terminal_control_event(key);
    if !session.has_active_overlay()
        && key.code == KeyCode::F(8)
        && key.kind != KeyEventKind::Release
        && session.toggle_latest_diagnostic()
    {
        return None;
    }
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

    // Double-Escape must be consecutive: any non-Esc key disarms the timer.
    if !matches!(key.code, KeyCode::Esc) {
        session.last_escape_press = None;
    }

    // Only the composer owner may consume Ctrl+C as an input-selection copy.
    // Active modal and runtime owners route the same key below.
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
            return Some(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Hotkey(action))));
        }

        // Text-only modals (no list): close on Esc or any keypress.
        // Without a list, handle_list_key_event returns NotHandled for all keys,
        // so we must handle the close/consume logic here to prevent keys from
        // falling through to normal input processing.
        // Secure prompt modals are excluded: character input must reach the
        // input handler so typed text and pasted content populate the field.
        if modal.list.is_none() && modal.secure_prompt.is_none() {
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

    // Arrow-Up/Down navigate history only for single-row input. A multi-row
    // composer (soft-wrapped or logical lines) always consumes Up/Down as
    // intra-buffer cursor moves (no-op at the first/last visual row).
    // Ctrl+P/N remain unconditional history shortcuts via the hardcoded
    // paths below.
    // Shift is excluded so Shift+Up/Down keep their prior fallback behavior
    // instead of being consumed as plain cursor moves (which would discard
    // selection semantics). Rebound or explicitly unbound Up/Down fall through
    // to the binding dispatch below, matching the app-layer fallback.
    if !has_control && !has_alt && !has_command && !has_shift {
        let cursor_move_claims_key = match key.code {
            KeyCode::Up => match session.bindings.resolve(&key) {
                Some(Action::HistoryPrevious) => true,
                None => !session.rebindable_action_is_overridden(Action::HistoryPrevious),
                _ => false,
            },
            KeyCode::Down => match session.bindings.resolve(&key) {
                Some(Action::HistoryNext) => true,
                None => !session.rebindable_action_is_overridden(Action::HistoryNext),
                _ => false,
            },
            _ => false,
        };
        if cursor_move_claims_key {
            if session.is_multi_row_composer() {
                // Multi-row composer (soft-wrapped or logical lines) consumes
                // Up/Down as intra-buffer moves (no-op at the first/last
                // visual row) and never traverses history.
                if matches!(key.code, KeyCode::Up) {
                    let _ = session.move_up_within_composer();
                } else {
                    let _ = session.move_down_within_composer();
                }
                session.mark_dirty();
                return None;
            }
            match key.code {
                KeyCode::Up => {
                    if session.move_cursor_up_for_history() {
                        session.mark_dirty();
                        return None;
                    }
                }
                KeyCode::Down if session.move_cursor_down_for_history() => {
                    session.mark_dirty();
                    return None;
                }
                _ => {}
            }
        }
    }

    // Binding store: resolve user-rebindable actions first. Readline editing
    // shortcuts remain owned by the hardcoded composer paths below.
    if let Some(action) = session.bindings.resolve(&key) {
        let is_readline_key = action::is_readline_editing_key(&key);
        let is_app_owned_action = matches!(action, Action::OpenTranscriptReview | Action::ToggleTranscriptRenderMode);
        if !is_readline_key && !is_app_owned_action {
            return dispatch_rebindable_action(session, action);
        }
    }

    match key.code {
        // --- Emacs-style editing shortcuts (hardcoded, not rebindable) ---
        //
        // Unified line-wise composer controls. The same operations serve two
        // input families so the behavior is identical everywhere:
        //   1. Kitty-protocol terminals report Cmd/Command as SUPER
        //      (`Cmd+Left`, `Cmd+Right`, `Cmd+Backspace`, `Cmd+A`).
        //   2. Legacy macOS terminals (Terminal.app, iTerm2, VS Code, …) map
        //      those same keys to C0 control codes — Cmd+Left → `0x01`
        //      (Ctrl+A), Cmd+Right → `0x05` (Ctrl+E), Cmd+Backspace → `0x15`
        //      (Ctrl+U), Cmd+Delete → `0x0B` (Ctrl+K). `normalize_terminal_`
        //      `control_event` folds those bytes into Ctrl+<letter>, so both
        //      families dispatch through the Ctrl+A/E/U/K arms below.
        // Single-line input degenerates to buffer edges, satisfying the
        // "clear/move the entire input" contract without a separate branch.
        KeyCode::Char('a') | KeyCode::Char('A') if has_control && !has_command && !has_alt => {
            if session.input_enabled {
                session.move_to_start_of_line();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('e') | KeyCode::Char('E') if has_control && !has_command && !has_alt => {
            if session.input_enabled {
                session.move_to_end_of_line();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('g') | KeyCode::Char('G') if has_control && !has_command && !has_alt && session.input_enabled => {
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
                // Clear the current logical line (or the whole input when
                // single-line). Matches Cmd+Backspace on kitty-protocol
                // terminals; legacy terminals send 0x15 (Ctrl+U).
                session.clear_current_line_or_all();
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
        KeyCode::Char('j') if has_control && session.input_enabled => {
            // Ctrl+J is a line feed character, insert newline for multiline input
            session.insert_char('\n');
            session.mark_dirty();
            None
        }
        KeyCode::Char('z') | KeyCode::Char('Z') if has_control && !has_command && !has_alt && session.input_enabled => {
            session.input_manager.undo();
            session.mark_dirty();
            None
        }
        KeyCode::Char('y') | KeyCode::Char('Y') if has_control && !has_command && !has_alt && session.input_enabled => {
            session.input_manager.redo();
            session.mark_dirty();
            None
        }

        // --- Readline-style editing shortcuts ---
        KeyCode::Char('f') | KeyCode::Char('F') if has_control && !has_command && !has_alt && session.input_enabled => {
            session.move_right();
            session.mark_dirty();
            None
        }
        KeyCode::Char('b') | KeyCode::Char('B') if has_control && !has_command && !has_alt && session.input_enabled => {
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
        KeyCode::Char('t') | KeyCode::Char('T') if has_control && !has_command && !has_alt && session.input_enabled => {
            session.transpose_chars();
            session.mark_dirty();
            None
        }
        KeyCode::Char('d') | KeyCode::Char('D') if has_alt && !has_control && !has_command && session.input_enabled => {
            session.delete_word_forward();
            session.mark_dirty();
            None
        }
        KeyCode::Char('t') | KeyCode::Char('T') if has_alt && !has_control && !has_command && session.input_enabled => {
            session.transpose_words();
            session.mark_dirty();
            None
        }
        KeyCode::Char('u') | KeyCode::Char('U') if has_alt && !has_control && !has_command && session.input_enabled => {
            session.uppercase_word();
            session.mark_dirty();
            None
        }
        KeyCode::Char('l') | KeyCode::Char('L') if has_alt && !has_control && !has_command && session.input_enabled => {
            session.lowercase_word();
            session.mark_dirty();
            None
        }
        KeyCode::Char('c') | KeyCode::Char('C') if has_alt && !has_control && !has_command && session.input_enabled => {
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
            // A visible text selection is the innermost dismissible state, so a
            // single Esc clears the highlight instead of arming the rewind
            // double-press. Overlay, interrupt, and cancel precedence is
            // preserved: those states are checked first.
            if !session.has_active_overlay()
                && !session.is_running_activity()
                && session.active_pty_session_count() == 0
                && session.clear_mouse_selection()
            {
                session.last_escape_press = None;
                None
            } else if session.has_active_overlay() {
                session.close_overlay();
                session.last_escape_press = None;
                None
            } else if session.is_running_activity() || session.active_pty_session_count() > 0 {
                session.last_escape_press = None;
                session.mark_dirty();
                Some(InlineEvent::Interrupt)
            } else if !session.input_enabled {
                session.last_escape_press = None;
                session.mark_dirty();
                Some(InlineEvent::Cancel)
            } else if session.input_manager.content().is_empty() {
                // Idle composer with empty input: a consecutive double-Escape
                // opens the rewind picker (`/rewind`). A single press remains a
                // no-op cancel so the armed timer does not escalate locally.
                let now = Instant::now();
                let is_double = action::is_double_escape_press(session.last_escape_press, now);
                session.mark_dirty();
                if is_double {
                    session.last_escape_press = None;
                    Some(InlineEvent::Submit("/rewind".into()))
                } else {
                    session.last_escape_press = Some(now);
                    Some(InlineEvent::Cancel)
                }
            } else {
                // Focused composer with content: require consecutive
                // double-Escape. First press arms, second clears current line
                // (multiline) or entire input (single-line, compact/image).
                let now = Instant::now();
                let is_double = action::is_double_escape_press(session.last_escape_press, now);
                if is_double {
                    session.last_escape_press = None;
                    if session.input_manager.is_single_line() {
                        command::clear_input(session);
                    } else {
                        session.clear_current_line_or_all();
                    }
                    session.mark_dirty();
                    None
                } else {
                    session.last_escape_press = Some(now);
                    session.clear_inline_prompt_suggestion();
                    session.mark_dirty();
                    None
                }
            }
        }
        KeyCode::Enter => {
            if !session.input_enabled {
                return None;
            }

            // Check for multiline input options (Shift/Alt)
            if !has_control && (has_shift || has_alt) {
                session.insert_char('\n');
                session.mark_dirty();
                return None;
            }

            if !has_control
                && !has_shift
                && !has_alt
                && session.input_manager.content().trim().is_empty()
                && session.active_pty_session_count() > 0
            {
                session.mark_dirty();
                return Some(InlineEvent::Submit("/jobs".into()));
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

                // Ctrl+Enter while a turn is running joins the queue so the
                // message dispatches after the current turn completes.
                // Plain text is batchable so consecutive queued messages
                // coalesce into one turn; slash stays one per turn.
                // Mirrors the app session's Ctrl+Enter path.
                return if session.is_running_activity() {
                    if submitted.text.trim_start().starts_with('/') {
                        session.push_queued_input(submitted.text.clone());
                        Some(InlineEvent::QueueSubmit(submitted))
                    } else {
                        session.push_queued_input(submitted.text.clone());
                        Some(InlineEvent::QueueSubmit(submitted.batchable()))
                    }
                } else {
                    Some(InlineEvent::Submit(submitted))
                };
            }

            let Some(submitted) = take_submitted_input(session) else {
                session.mark_dirty();
                return None;
            };

            session.mark_dirty();

            // While a turn is actively running, enqueue the message so it
            // dispatches after the current turn completes. This avoids racing
            // a mid-turn steer against the tool-batch boundary and the
            // steering-channel lifetime: the MPSC FIFO plus the runloop's
            // drain-then-flush_sync keeps UI-optimistic and authoritative
            // queue snapshots in order. Each plain-Enter stays its own
            // non-batchable turn.
            if session.is_running_activity() {
                session.push_queued_input(submitted.text.clone());
                return Some(InlineEvent::QueueSubmit(submitted));
            }
            Some(InlineEvent::Submit(submitted))
        }
        KeyCode::Tab => {
            if !session.input_enabled {
                return None;
            }

            if session.accept_inline_prompt_suggestion() {
                return None;
            }

            // Shift+Tab arriving as Tab+SHIFT still switches agents; plain Tab
            // enqueues the draft like Ctrl+Enter. Agent cycling lives on
            // BackTab (Shift+Tab) — see `can_cycle_primary_agent`.
            if has_shift {
                if mode_switch_guard::try_cycle_primary_agent(session, &key) {
                    session.mark_dirty();
                    return Some(InlineEvent::CyclePrimaryAgent);
                }
                return None;
            }

            enqueue_tab_draft(session)
        }
        KeyCode::BackTab => {
            if !session.input_enabled {
                return None;
            }

            session.clear_inline_prompt_suggestion();
            session.mark_dirty();
            if !mode_switch_guard::try_cycle_primary_agent(session, &key) {
                return None;
            }
            Some(InlineEvent::CyclePrimaryAgentPrevious)
        }
        KeyCode::Backspace => {
            if session.input_enabled {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.clear_current_line_or_all();
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
                    session.select_to_start_of_line();
                } else if has_shift {
                    session.select_left();
                } else if has_command {
                    session.move_to_start_of_line();
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
                    session.select_to_end_of_line();
                } else if has_shift {
                    session.select_right();
                } else if has_command {
                    session.move_to_end_of_line();
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

            if ch == '?' && !has_control && !has_alt && !has_command && session.input_manager.content().is_empty() {
                session.show_help_modal();
                return None;
            }

            if ch == '\t' {
                if session.accept_inline_prompt_suggestion() {
                    return None;
                }
                // Terminals that deliver Tab as Char('\t'): plain Tab enqueues
                // like Ctrl+Enter; Shift+Tab still cycles agents.
                if has_shift {
                    if mode_switch_guard::try_cycle_primary_agent(session, &key) {
                        session.mark_dirty();
                        return Some(InlineEvent::CyclePrimaryAgent);
                    }
                    return None;
                }
                return enqueue_tab_draft(session);
            }

            if has_command {
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
    // Agent/mode switching lives on Shift+Tab only (BackTab). Plain Tab
    // enqueues the draft like Ctrl+Enter, so it must not cycle.
    // Crossterm reports Shift+Tab as BackTab (with or without SHIFT bit);
    // some terminals report Tab+SHIFT instead — accept both.
    let valid_modifiers = match key.code {
        KeyCode::BackTab => key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT,
        KeyCode::Tab => key.modifiers == KeyModifiers::SHIFT,
        KeyCode::Char('\t') => key.modifiers == KeyModifiers::SHIFT,
        _ => false,
    };
    valid_modifiers && !session.has_active_overlay()
}

/// Notice shown when the user requests a primary-agent mode switch while a turn
/// is actively processing. Mode switches are locked for the duration of a turn.
fn push_mode_switch_busy_notice(session: &mut Session) {
    session.push_line(
        InlineMessageKind::Warning,
        vec![InlineSegment {
            text: mode_switch_guard::MODE_SWITCH_BUSY_NOTICE.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.mark_dirty();
}

impl mode_switch_guard::ModeSwitchGuardSession for Session {
    fn is_running_activity(&self) -> bool {
        TuiSessionDriver::is_running_activity(self)
    }

    fn is_mode_switch_locked(&self) -> bool {
        self.activity_state.locks_mode_switch()
    }

    fn can_cycle_primary_agent(&self, key: &KeyEvent) -> bool {
        can_cycle_primary_agent(self, key)
    }

    fn notify_mode_switch_busy(&mut self) {
        push_mode_switch_busy_notice(self);
    }
}

fn take_submitted_input(session: &mut Session) -> Option<SubmittedInput> {
    let submitted = session.input_manager.content().to_owned();
    let submitted_entry = session.input_manager.current_history_entry();
    clear_submitted_input(session);

    if submitted_entry.is_empty() {
        return None;
    }

    let attachments = submitted_entry.attachment_elements();
    session.remember_submitted_input(submitted_entry);
    Some(SubmittedInput::new(submitted, attachments))
}

fn clear_submitted_input(session: &mut Session) {
    session.input_manager.clear();
    session.clear_suggested_prompt_state();
    session.clear_inline_prompt_suggestion();
    session.input_compact_mode = false;
    session.scroll_manager.set_offset(0);
}
