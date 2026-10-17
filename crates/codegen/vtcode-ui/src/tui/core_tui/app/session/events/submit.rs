//! Tab-to-queue drafts and slash-command submit gating.

use super::*;

/// Shared Tab-to-queue path mirroring `Ctrl+Enter` for the app session.
///
/// Plain text becomes batchable when queued during a running turn; slash
/// commands stay non-batchable so command intent is preserved.
pub(crate) fn enqueue_tab_draft(session: &mut Session) -> Option<InlineEvent> {
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
        match extract_slash_command_name(&submitted.text) {
            Some("stop") => Some(InlineEvent::Interrupt),
            Some("pause") => Some(InlineEvent::Pause),
            Some("resume") => Some(InlineEvent::Resume),
            Some(_) => {
                let text = submitted.text.clone();
                session.push_queued_input(text);
                Some(InlineEvent::QueueSubmit(submitted))
            }
            None => {
                let text = submitted.text.clone();
                session.push_queued_input(text);
                Some(InlineEvent::QueueSubmit(submitted.batchable()))
            }
        }
    } else {
        Some(InlineEvent::Submit(submitted))
    }
}

pub(crate) fn take_submitted_input(session: &mut Session) -> Option<SubmittedInput> {
    let submitted = session.core.input_manager.content().to_owned();
    let submitted_entry = session.core.input_manager.current_history_entry();
    clear_submitted_input(session);

    if submitted_entry.is_empty() {
        return None;
    }

    let attachments = submitted_entry.attachment_elements();
    session.remember_submitted_input(submitted_entry);
    Some(SubmittedInput::new(submitted, attachments))
}

pub(crate) fn clear_submitted_input(session: &mut Session) {
    session.core.input_manager.clear();
    session.clear_suggested_prompt_state();
    session.clear_inline_prompt_suggestion();
    session.core.set_input_compact_mode(false);
    session.core.scroll_manager.set_offset(0);
    session.update_input_triggers();
}

pub(crate) fn handle_running_slash_command_block(session: &mut Session) -> bool {
    let input = session.core.input_manager.content().to_owned();
    handle_running_slash_command_block_for_input(session, &input)
}

pub(crate) fn handle_running_slash_command_block_for_input(session: &mut Session, input: &str) -> bool {
    let Some(command_name) = extract_slash_command_name(input) else {
        return false;
    };

    // Building and recovery keep the composer available for follow-up input,
    // but they still own the primary-agent/planning boundary. A blocked turn
    // is quiescent, so every explicit slash command remains available for
    // recovery or user-directed changes.
    let is_blocked = matches!(session.core.activity_state, vtcode_commons::ui_protocol::ActivityState::Blocked);
    let is_mode_switch = matches!(command_name, "mode" | "plan");
    let command_is_locked = !is_blocked
        && (session.is_running_activity() || (session.core.activity_state.locks_mode_switch() && is_mode_switch));
    if !command_is_locked {
        return false;
    }

    // Navigation commands must always submit: `/new` starts a fresh session
    // and `/exit` leaves the program. Blocking them while a turn runs forces
    // a retype after the turn and reads as "doesn't trigger". Falling
    // through lets the immediate-submit path queue them so they run right
    // after the current turn instead of being dropped.
    if matches!(command_name, "new" | "exit") {
        return false;
    }

    // Read-only local commands are safe to defer: falling through lets the normal
    // queueing path run them right after the current turn instead of dropping them.
    if matches!(command_name, "copy") {
        return false;
    }

    // Mode switches (agent selection, planning workflow) are locked while a turn
    // is processing; surface the dedicated notice for those commands.
    let message = if is_mode_switch {
        mode_switch_guard::MODE_SWITCH_BUSY_NOTICE.to_string()
    } else {
        format!(
            "'/{command_name}' is disabled while a task is in progress. Please wait for the current task to complete before using this command."
        )
    };
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

pub(crate) fn maybe_handle_busy_steering_command(session: &mut Session) -> Option<InlineEvent> {
    if !TuiSessionDriver::is_running_activity(session) {
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

pub(crate) fn extract_slash_command_name(input: &str) -> Option<&str> {
    let trimmed = input.trim_start();
    let command_input = trimmed.strip_prefix('/')?;
    let command = command_input.split_whitespace().next()?;
    if command.is_empty() { None } else { Some(command) }
}
