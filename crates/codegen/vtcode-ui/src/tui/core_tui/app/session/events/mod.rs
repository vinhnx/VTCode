use super::*;
use ratatui::crossterm::event::KeyModifiers;
use ratatui_cheese::input::InputState;
use std::sync::Arc;
use std::time::Instant;

use super::super::types::{
    ContentPart, DiffPreviewMode, InlineTextStyle, TransientEvent, TransientSelectionChange, TransientSubmission,
};
use crate::tui::core_tui::app::session::transient::TransientSurface;
use crate::tui::core_tui::app::types::InlineMessageKind;
use crate::tui::core_tui::runner::TuiSessionDriver;
use crate::tui::core_tui::session::action::{
    Action, is_double_escape_press, is_readline_editing_key, normalize_terminal_control_event,
};
use crate::tui::core_tui::session::clipboard_image::{
    ClipboardImageError, ClipboardTextError, read_clipboard_image, read_clipboard_text,
};
use crate::tui::core_tui::session::modal;
use crate::tui::core_tui::session::modal::{ModalKeyModifiers, ModalListKeyResult};
use crate::tui::core_tui::session::mode_switch_guard::{self};
use crate::tui::core_tui::session::reverse_search;
use crate::tui::core_tui::types::InlineSegment;
use crate::tui::core_tui::types::{
    InlineEvent as CoreInlineEvent, OverlayEvent, OverlaySelectionChange, SubmittedInput,
};
use crate::tui::ui::theme;

mod emit;
mod history;
mod modal_keys;
mod paste;
mod secure_prompt;
mod submit;
mod viewer;

pub(super) use emit::emit_inline_event;
use history::input_history_entries;
pub(super) use history::open_history_picker;
use modal_keys::{is_inline_lists_toggle_shortcut, maybe_show_help_modal};
pub(super) use paste::handle_paste;
use paste::{
    copy_selected_input_if_requested, handle_image_paste_shortcut_with, handle_raw_text_paste_shortcut_with,
    is_image_paste_shortcut, is_raw_text_paste_shortcut,
};
use secure_prompt::handle_secure_prompt_key;
use submit::{
    clear_submitted_input, enqueue_tab_draft, extract_slash_command_name, handle_running_slash_command_block,
    handle_running_slash_command_block_for_input, maybe_handle_busy_steering_command, take_submitted_input,
};
use viewer::{DiffPreviewKeyResult, ToolOutputViewerKeyResult, handle_diff_preview_key, handle_tool_output_viewer_key};

pub(super) fn process_key(session: &mut Session, key: KeyEvent) -> Option<InlineEvent> {
    process_key_with_clipboard_readers(session, key, read_clipboard_image, read_clipboard_text)
}

pub(super) fn process_key_with_clipboard_image_reader(
    session: &mut Session,
    key: KeyEvent,
    image_reader: impl FnMut() -> Result<ContentPart, ClipboardImageError>,
) -> Option<InlineEvent> {
    process_key_with_clipboard_readers(session, key, image_reader, read_clipboard_text)
}

#[cfg(test)]
pub(super) fn process_key_with_clipboard_text_reader(
    session: &mut Session,
    key: KeyEvent,
    text_reader: impl FnMut() -> Result<String, ClipboardTextError>,
) -> Option<InlineEvent> {
    process_key_with_clipboard_readers(session, key, read_clipboard_image, text_reader)
}

fn process_key_with_clipboard_readers(
    session: &mut Session,
    key: KeyEvent,
    image_reader: impl FnMut() -> Result<ContentPart, ClipboardImageError>,
    text_reader: impl FnMut() -> Result<String, ClipboardTextError>,
) -> Option<InlineEvent> {
    let key = normalize_terminal_control_event(key);
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
        session.core.last_escape_press = None;
    }

    if copy_selected_input_if_requested(session, &key, has_command) {
        return None;
    }

    if is_raw_text_paste_shortcut(&key, has_control, has_alt, has_command, has_shift) {
        if session.core.input_enabled() {
            handle_raw_text_paste_shortcut_with(session, text_reader);
        }
        return None;
    }

    if is_image_paste_shortcut(&key, has_control, has_alt, has_command, has_shift) {
        if session.core.input_enabled() {
            handle_image_paste_shortcut_with(session, image_reader);
        }
        return None;
    }

    // Secure prompt modals own their own key handling. They render as
    // `FloatingOverlay` (Modal focus policy), which disables the regular
    // composer via `core.input_enabled() == false`. The normal key handler
    // gates every character insertion on `input_enabled()`, so without this
    // dedicated handler typed characters would never reach the masked input
    // field. The handler edits the shared input manager directly — the modal
    // renderer reads `input_manager.content()` / `cursor()` to display the
    // masked value — and scopes accepted keys to text editing plus
    // submit/cancel, so composer shortcuts (Ctrl+M, Alt+S, …) do not leak
    // through while the user is entering a secret.
    if session
        .modal_state_mut()
        .is_some_and(|m| m.list.is_none() && m.secure_prompt.is_some())
    {
        return handle_secure_prompt_key(session, key, has_control, has_shift, has_alt, has_command);
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
            return Some(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Hotkey(action.into()))));
        }

        // Text-only modals (no list, no secure prompt): close on Esc or any
        // keypress. Secure prompt modals are handled above and excluded here
        // so character input can flow through to the normal input handler.
        if modal.list.is_none() && modal.secure_prompt.is_none() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    session.close_overlay();
                    session.mark_dirty();
                    return None;
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
                // Synchronous preview: fire the callback and sync session theme
                // before returning so the render picks up the preview in the
                // same frame as the cursor movement.
                if let Some(ref cb) = session.preview_callback
                    && let CoreInlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
                        ref selection,
                    ))) = event
                {
                    let _ = cb(Some(selection));
                    if theme::has_preview_theme() {
                        session.sync_theme_from_runtime();
                    }
                }
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
                session.cancel_theme_preview();
                session.close_overlay();
                return Some(event.into());
            }
            ModalListKeyResult::NotHandled => {}
        }
    }

    let configured_action = session.core.resolve_rebindable_action(&key);
    match configured_action {
        Some(Action::ToggleToolDisplayMode) => {
            session.invalidate_transcript_cache();
            session.mark_dirty();
            return Some(InlineEvent::ToggleToolDisplayMode);
        }
        Some(Action::ToggleTaskPanel) => {
            session.toggle_task_panel();
            return None;
        }
        Some(Action::OpenTranscriptReview) => {
            let width = session.core.transcript_width.max(1);
            let height = session.core.transcript_rows.max(1);
            if session.tool_output_viewer_state().is_some() {
                session.close_tool_output_viewer();
            } else {
                session.open_tool_output_viewer(width, height, None);
            }
            return None;
        }
        Some(Action::BackgroundOperation)
            if session.local_agents_visible() && !session.core.has_active_foreground_pty() =>
        {
            // Toggle-close: the same shortcut that opens the panel hides it again.
            // Entries, selection, and expanded state are retained, so reopening
            // is instant. A live foreground command keeps priority: fall through
            // and emit so the runloop can background it.
            session.close_local_agents_drawer(true);
            session.mark_dirty();
            return None;
        }
        Some(Action::ToggleTranscriptRenderMode) => {
            if let Some(viewer) = session.tool_output_viewer_state_mut() {
                viewer.toggle_render_mode();
                session.mark_dirty();
                return None;
            }
        }
        _ => {}
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

    match handle_tool_output_viewer_key(session, &key, has_control, has_alt, has_command) {
        ToolOutputViewerKeyResult::Emit(event) => return Some(event),
        ToolOutputViewerKeyResult::Handled => return None,
        ToolOutputViewerKeyResult::NotHandled => {}
    }

    match handle_diff_preview_key(session, &key) {
        DiffPreviewKeyResult::Emit(event) => return Some(event),
        DiffPreviewKeyResult::Handled => return None,
        DiffPreviewKeyResult::NotHandled => {}
    }

    // Handle history picker (Ctrl+R) - Visual fuzzy search for command history
    if has_control && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R')) && !session.history_picker_visible() {
        open_history_picker(session);
        return None;
    }

    // Handle forward search (Ctrl+S) - Readline forward search
    if has_control && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) && !session.history_picker_visible() {
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

    if let Some(action) = configured_action {
        let contextual_arrow_action = match key.code {
            KeyCode::Up => Some(Action::HistoryPrevious),
            KeyCode::Down => Some(Action::HistoryNext),
            _ => None,
        };
        let preserve_contextual_arrow = contextual_arrow_action == Some(action);
        let overridden_contextual_arrow = contextual_arrow_action
            .is_some_and(|contextual_action| session.core.rebindable_action_is_overridden(contextual_action));

        let render_mode_without_viewer =
            action == Action::ToggleTranscriptRenderMode && session.tool_output_viewer_state().is_none();
        if !render_mode_without_viewer && !is_readline_editing_key(&key) && !preserve_contextual_arrow {
            return session.core.dispatch_rebindable_action(action).map(Into::into);
        }

        if overridden_contextual_arrow && contextual_arrow_action != Some(action) {
            return None;
        }
    } else if let Some(contextual_action) = match key.code {
        KeyCode::Up => Some(Action::HistoryPrevious),
        KeyCode::Down => Some(Action::HistoryNext),
        _ => None,
    } && session.core.rebindable_action_is_overridden(contextual_action)
    {
        // An explicit replacement or empty list removes the old arrow action;
        // do not let the legacy contextual fallback resurrect it.
        return None;
    }

    match key.code {
        KeyCode::Char('c') | KeyCode::Char('C') if has_control => {
            if session.core.mouse_selection.has_selection {
                session.core.mouse_selection.request_copy();
                session.mark_dirty();
                return None;
            }
            let now = Instant::now();
            if session
                .core
                .last_interrupt_press
                .is_some_and(|last| now.duration_since(last).as_millis() < 1_000)
            {
                session.core.last_interrupt_press = None;
                session.request_exit();
                session.mark_dirty();
                return Some(InlineEvent::Exit);
            }
            session.core.last_interrupt_press = Some(now);
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
            let now = Instant::now();
            if session
                .core
                .last_interrupt_press
                .is_some_and(|last| now.duration_since(last).as_millis() < 1_000)
            {
                session.core.last_interrupt_press = None;
                session.request_exit();
                session.mark_dirty();
                return Some(InlineEvent::Exit);
            }
            session.core.last_interrupt_press = Some(now);
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
        KeyCode::Char('b') if has_control && !has_alt && !has_command => {
            // If the user explicitly unbinds background_operation, preserve the
            // traditional Readline fallback.
            if session.core.input_enabled() {
                session.move_left();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('f') if has_control && !has_alt && !has_command => {
            // Ctrl+F: Move forward a character (Readline)
            if session.core.input_enabled() {
                session.move_right();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('p') if has_control && !has_alt && !has_command => {
            // Ctrl+P: Fetch the previous command from history (Readline)
            if session.navigate_history_previous() {
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('n') if has_control && !has_alt && !has_command => {
            // Ctrl+N: Fetch the next command from history (Readline)
            if session.navigate_history_next() {
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('t') | KeyCode::Char('T') | KeyCode::Char('\u{14}')
            if (has_control || matches!(key.code, KeyCode::Char('\u{14}'))) && !has_alt && !has_command =>
        {
            // The app-level review action handles its configured binding before
            // this fallback. Ctrl+T reaches here only when review is explicitly
            // unbound, preserving the original Readline transpose behavior.
            if session.core.input_enabled() {
                session.transpose_chars();
                session.update_input_triggers();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('m') | KeyCode::Char('M') if has_control && !has_alt && !has_command => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/model".into()))
        }
        KeyCode::Char('s') | KeyCode::Char('S') if has_alt && !has_control && !has_command => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/subprocesses".into()))
        }
        KeyCode::Char('a') | KeyCode::Char('A') if has_control && !has_command && !has_alt => {
            if session.core.input_enabled() {
                // Line-wise so legacy Cmd+Left (0x01) matches the kitty path.
                session.move_to_start_of_line();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('e') | KeyCode::Char('E') if has_control && !has_command && !has_alt => {
            if session.core.input_enabled() {
                // Line-wise so legacy Cmd+Right (0x05) matches the kitty path.
                session.move_to_end_of_line();
                session.mark_dirty();
            }
            None
        }
        KeyCode::Char('g') | KeyCode::Char('G')
            if has_control && !has_command && !has_alt && session.core.input_enabled() =>
        {
            let draft = session.core.input_manager.content().to_string();
            session.mark_dirty();
            Some(InlineEvent::LaunchEditor { draft })
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
                // Clear the current line (or entire input when single-line);
                // legacy Cmd+Backspace sends 0x15 and folds to Ctrl+U.
                session.clear_current_line_or_all();
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
        KeyCode::Char('z') | KeyCode::Char('Z')
            if has_control && !has_command && !has_alt && session.core.input_enabled() =>
        {
            session.core.input_manager.undo();
            session.mark_dirty();
            None
        }
        KeyCode::Char('y') | KeyCode::Char('Y')
            if has_control && !has_command && !has_alt && session.core.input_enabled() =>
        {
            session.core.input_manager.redo();
            session.mark_dirty();
            None
        }
        KeyCode::Char('l') | KeyCode::Char('L') if has_control => {
            session.mark_dirty();
            Some(InlineEvent::Submit("/clear".into()))
        }
        KeyCode::BackTab => {
            session.clear_inline_prompt_suggestion();
            session.mark_dirty();
            if !mode_switch_guard::try_cycle_primary_agent(session, &key) {
                return None;
            }
            Some(InlineEvent::CyclePrimaryAgentPrevious)
        }
        KeyCode::Esc => {
            // A visible text selection is the innermost dismissible state, so a
            // single Esc clears the highlight instead of arming the rewind
            // double-press. Overlay, interrupt, and cancel precedence is
            // preserved: those states are checked first.
            if !session.has_active_overlay()
                && !session.is_running_activity()
                && session.active_pty_session_count() == 0
                && session.core.clear_mouse_selection()
            {
                session.core.last_escape_press = None;
                None
            } else if session.has_active_overlay() {
                session.close_overlay();
                session.core.last_escape_press = None;
                None
            } else if session.is_running_activity() || session.active_pty_session_count() > 0 {
                session.core.last_escape_press = None;
                session.mark_dirty();
                Some(InlineEvent::Interrupt)
            } else if !session.core.input_enabled() {
                session.core.last_escape_press = None;
                session.mark_dirty();
                Some(InlineEvent::Cancel)
            } else if session.core.input_manager.content().is_empty() {
                // Idle composer with empty input: a consecutive double-Escape
                // opens the rewind picker (`/rewind`). A single press remains a
                // no-op cancel so the armed timer does not escalate locally.
                let now = Instant::now();
                let is_double = is_double_escape_press(session.core.last_escape_press, now);
                session.mark_dirty();
                if is_double {
                    session.core.last_escape_press = None;
                    Some(InlineEvent::Submit("/rewind".into()))
                } else {
                    session.core.last_escape_press = Some(now);
                    Some(InlineEvent::Cancel)
                }
            } else {
                // Focused composer with content: require consecutive
                // double-Escape. First press arms, second clears current line
                // (multiline) or entire input (single-line, compact/image).
                let now = Instant::now();
                let is_double = is_double_escape_press(session.core.last_escape_press, now);
                if is_double {
                    session.core.last_escape_press = None;
                    if session.core.input_manager.is_single_line() {
                        session
                            .core
                            .handle_command(crate::tui::core_tui::types::InlineCommand::ClearInput);
                    } else {
                        session.clear_current_line_or_all();
                        session.update_input_triggers();
                    }
                    session.mark_dirty();
                    None
                } else {
                    session.core.last_escape_press = Some(now);
                    session.clear_inline_prompt_suggestion();
                    session.mark_dirty();
                    None
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
            if !crate::tui::core_tui::session::terminal_capabilities::queued_input_edit_uses_shift_left()
                && edit_queue_modifier
                && !session.core.queued_inputs.is_empty()
            {
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
            } else if !has_control && !has_alt && !has_command && !has_shift && session.is_multi_row_composer() {
                // Multi-row composer consumes Up as an intra-buffer visual
                // move (no-op at the first row) and never traverses history.
                // Single-row history follows below; Ctrl+P remains the
                // unconditional history shortcut.
                let _ = session.move_up_within_composer();
                session.mark_dirty();
                None
            } else if !has_control && !has_alt && !has_command && !has_shift && session.move_cursor_up_for_history() {
                session.mark_dirty();
                None
            } else if session.navigate_history_previous() {
                session.mark_dirty();
                Some(InlineEvent::HistoryPrevious)
            } else {
                None
            }
        }
        KeyCode::Down => {
            if session.should_open_local_agents_with_down(&key, has_control, has_alt, has_command) {
                session.open_local_agents_drawer(false);
                session.mark_dirty();
                return None;
            }
            if !has_control && !has_alt && !has_command && !has_shift && session.is_multi_row_composer() {
                // Multi-row composer consumes Down as an intra-buffer visual
                // move (no-op at the last row) and never traverses history.
                // Single-row history follows below; Ctrl+N remains the
                // unconditional history shortcut.
                let _ = session.move_down_within_composer();
                session.mark_dirty();
                None
            } else if !has_control && !has_alt && !has_command && !has_shift && session.move_cursor_down_for_history() {
                session.mark_dirty();
                None
            } else if session.navigate_history_next() {
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
                    && !palette.get_selected().is_some_and(|e| e.is_dir)
                    && let Some(entry) = palette.get_selected()
                {
                    let file_path = entry.relative_path.clone();
                    session.insert_file_reference(&file_path);
                    session.close_file_palette();
                    session.mark_dirty();
                    return Some(InlineEvent::FileSelected(file_path));
                }
                return None;
            }

            // Check for multiline input options (Shift/Alt)
            if !has_control && (has_shift || has_alt) {
                // Insert newline for multiline input
                session.insert_char('\n');
                session.mark_dirty();
                return None;
            }

            if maybe_show_help_modal(session) {
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
                return Some(InlineEvent::Submit("/jobs".into()));
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
                    match extract_slash_command_name(&submitted.text) {
                        Some("stop") => Some(InlineEvent::Interrupt),
                        Some("pause") => Some(InlineEvent::Pause),
                        Some("resume") => Some(InlineEvent::Resume),
                        other => {
                            // Ctrl+Enter while a turn is running joins the
                            // visible queue, so the message hangs above the
                            // composer, renders as the user's own bubble when
                            // dispatched, and can be edited via Shift+← before
                            // it sends. Plain messages are marked batchable so
                            // several queued messages coalesce into ONE turn;
                            // slash commands stay one per turn so command
                            // intent is preserved. Plain Enter steers the
                            // active run instead of queueing.
                            if let Some(command_name) = other {
                                tracing::debug!(target: "vtcode_ui::keys", %command_name, "ctrl+enter queued slash command");
                                session.push_queued_input(submitted.text.clone());
                                Some(InlineEvent::QueueSubmit(submitted))
                            } else {
                                tracing::debug!(target: "vtcode_ui::keys", "ctrl+enter queued message");
                                session.push_queued_input(submitted.text.clone());
                                Some(InlineEvent::QueueSubmit(submitted.batchable()))
                            }
                        }
                    }
                } else {
                    Some(InlineEvent::Submit(submitted))
                };
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

            // While a turn is actively running, steer plain text: the message
            // is injected into the conversation right after the current
            // tool-call batch, so the model sees it on its next request within
            // this turn. Slash commands keep the queue path, and Ctrl+Enter
            // keeps the queue role. Otherwise submit directly so the turn
            // starts now.
            if session.is_running_activity() {
                if submitted.text.trim_start().starts_with('/') {
                    session.push_queued_input(submitted.text.clone());
                    return Some(InlineEvent::QueueSubmit(submitted));
                }
                return Some(InlineEvent::Steer(submitted));
            }
            Some(InlineEvent::Submit(submitted))
        }
        KeyCode::Tab => {
            if !session.core.input_enabled() {
                return None;
            }

            if session.accept_inline_prompt_suggestion() {
                session.update_input_triggers();
                return None;
            }

            // Shift+Tab arriving as Tab+SHIFT still switches agents; plain Tab
            // enqueues the draft like Ctrl+Enter.
            if has_shift {
                if mode_switch_guard::try_cycle_primary_agent(session, &key) {
                    session.mark_dirty();
                    return Some(InlineEvent::CyclePrimaryAgent);
                }
                return None;
            }

            enqueue_tab_draft(session)
        }
        KeyCode::Backspace => {
            if session.core.input_enabled() {
                if has_alt {
                    session.delete_word_backward();
                } else if has_command {
                    session.clear_current_line_or_all();
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
                let tmux_queue_edit = has_shift
                    && !has_control
                    && !has_command
                    && !has_alt
                    && crate::tui::core_tui::session::terminal_capabilities::queued_input_edit_uses_shift_left()
                    && !session.core.queued_inputs.is_empty();
                if tmux_queue_edit {
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
            if session.core.input_enabled() {
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
        KeyCode::Char('o') | KeyCode::Char('O') if has_control && !has_alt && !has_command => {
            // Ctrl+O: Copy last agent response as markdown to clipboard
            session.mark_dirty();
            Some(InlineEvent::Submit("/copy".into()))
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

            if ch == '?' && !has_control && !has_alt && !has_command && session.core.input_manager.content().is_empty()
            {
                session.show_help_modal();
                return None;
            }

            if ch == '\t' {
                if session.accept_inline_prompt_suggestion() {
                    session.update_input_triggers();
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
                        session.update_input_triggers();
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
                    // Alt+D: Kill (cut) forwards to the end of the current word
                    'd' | 'D' if session.core.input_enabled() => {
                        session.delete_word_forward();
                        session.update_input_triggers();
                        session.mark_dirty();
                    }
                    // Alt+U: Uppercase the current word
                    'u' | 'U' if session.core.input_enabled() => {
                        session.uppercase_word();
                        session.update_input_triggers();
                        session.mark_dirty();
                    }
                    // Alt+L: Lowercase the current word
                    'l' | 'L' if session.core.input_enabled() => {
                        session.lowercase_word();
                        session.update_input_triggers();
                        session.mark_dirty();
                    }
                    // Alt+C: Capitalize the current word
                    'c' | 'C' if session.core.input_enabled() => {
                        session.capitalize_word();
                        session.update_input_triggers();
                        session.mark_dirty();
                    }
                    // Alt+\: Delete whitespace around the cursor
                    '\\' if session.core.input_enabled() => {
                        session.delete_whitespace_around_cursor();
                        session.update_input_triggers();
                        session.mark_dirty();
                    }
                    _ => {}
                }
                return None;
            }

            if has_control {
                match ch {
                    'f' | 'F' => {
                        // Ctrl+F: Move forward a character (Readline)
                        if session.core.input_enabled() {
                            session.move_right();
                            session.mark_dirty();
                        }
                        return None;
                    }
                    'b' | 'B' => {
                        // Ctrl+B: Move back a character (Readline)
                        if session.core.input_enabled() {
                            session.move_left();
                            session.mark_dirty();
                        }
                        return None;
                    }
                    'p' | 'P' => {
                        // Ctrl+P: Fetch the previous command from history (Readline)
                        if session.navigate_history_previous() {
                            session.mark_dirty();
                        }
                        return None;
                    }
                    'n' | 'N' => {
                        // Ctrl+N: Fetch the next command from history (Readline)
                        if session.navigate_history_next() {
                            session.mark_dirty();
                        }
                        return None;
                    }
                    't' | 'T' => {
                        // Ctrl+T: Transpose characters (Readline)
                        if session.core.input_enabled() {
                            session.transpose_chars();
                            session.update_input_triggers();
                            session.mark_dirty();
                        }
                        return None;
                    }
                    _ => {}
                }
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

#[cfg(test)]
mod tests;
