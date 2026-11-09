use super::*;
use crate::tui::core_tui::session::config::BusyEnterBehavior;

pub(super) fn handle_escape_key(session: &mut Session) -> Option<InlineEvent> {
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

pub(super) fn handle_enter_key(
    session: &mut Session,
    has_control: bool,
    has_shift: bool,
    has_alt: bool,
) -> Option<InlineEvent> {
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
                    // intent is preserved. Busy plain Enter queues as
                    // its own non-batchable turn instead of steering.
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

    if session.core.consume_enter_jump_to_latest() {
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

    // While a turn is actively running, enqueue plain text so it runs
    // after the current turn completes. Queueing avoids racing a
    // mid-turn steer against the tool-batch boundary and the
    // steering-channel latch: the MPSC FIFO plus the runloop's
    // drain-then-flush_sync keeps UI-optimistic and authoritative
    // queue snapshots in order. Each plain-Enter stays its own
    // non-batchable turn (only Ctrl+Enter merges). Unblocked slash
    // keeps the existing queue path (blockable slash is rejected
    // earlier); Ctrl+Enter keeps the queue role. In steer mode,
    // plain-text input instead injects into the live turn via `Steer`.
    if session.is_running_activity() {
        if matches!(session.core.appearance.busy_enter_behavior, BusyEnterBehavior::Steer)
            && submitted.is_steerable_text()
        {
            return Some(InlineEvent::Steer(submitted));
        }
        session.push_queued_input(submitted.text.clone());
        return Some(InlineEvent::QueueSubmit(submitted));
    }
    Some(InlineEvent::Submit(submitted))
}

pub(super) fn handle_left_key(
    session: &mut Session,
    has_control: bool,
    has_shift: bool,
    has_alt: bool,
    has_command: bool,
) -> Option<InlineEvent> {
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

pub(super) fn handle_right_key(
    session: &mut Session,
    has_shift: bool,
    has_alt: bool,
    has_command: bool,
) -> Option<InlineEvent> {
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
