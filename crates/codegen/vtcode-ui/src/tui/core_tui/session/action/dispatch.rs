use super::super::*;
use ratatui::crossterm::event::KeyModifiers;
use std::time::Instant;

use crate::tui::core_tui::types::OverlayEvent;
use crate::tui::ui::tui::session::modal::{ModalKeyModifiers, ModalListKeyResult};

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
        Action::FocusInput => focus_input(session),
    }
}

/// Dismiss key-capturing session state (overlay, reverse search) and return
/// to the composer input. Mirrors what Esc does to these states — including
/// modal Cancel emission — without Esc's interrupt, cancel-submit, or rewind
/// tails. The synthetic Esc passes are bounded at three: a wizard notes
/// editor deactivates first, then a modal/wizard search query clears, then
/// the overlay cancels. Each pass strictly reduces open sub-state, so three
/// always suffice.
fn focus_input(session: &mut Session) -> Option<InlineEvent> {
    if session.reverse_search_state.active {
        session.reverse_search_state.cancel_search(&mut session.input_manager);
        session.mark_dirty();
    }
    if session.modal_state().is_some_and(|modal| modal.secure_prompt.is_some()) {
        // Masked secret entry: clear the shared scratch like the app-layer
        // secure Esc arm, never strand the secret in the composer draft.
        session.input_manager.clear();
        session.close_overlay();
        session.mark_dirty();
        return None;
    }
    if session.modal_state().is_some_and(|modal| modal.list.is_none()) {
        // Text-only overlay: Esc closes these with a Cancel event.
        session.close_overlay();
        session.mark_dirty();
        return Some(InlineEvent::Overlay(OverlayEvent::Cancelled));
    }
    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
    for _ in 0..3 {
        let modifiers = ModalKeyModifiers { control: false, alt: false, command: false };
        let result = if let Some(modal) = session.modal_state_mut() {
            modal.handle_list_key_event(&esc, modifiers)
        } else if let Some(wizard) = session.wizard_overlay_mut() {
            wizard.handle_key_event(&esc, modifiers)
        } else {
            break;
        };
        match result {
            ModalListKeyResult::Submit(event) | ModalListKeyResult::Cancel(event) => {
                session.close_overlay();
                session.mark_dirty();
                return Some(event);
            }
            ModalListKeyResult::Emit(_) | ModalListKeyResult::Redraw => {
                // A sub-state collapsed (notes editor, query); loop on toward cancel.
                session.mark_dirty();
            }
            ModalListKeyResult::HandledNoRedraw | ModalListKeyResult::NotHandled => break,
        }
    }
    None
}
