//! Modal, wizard-list, help, and mode-switch key gating.

use super::*;

pub(crate) fn is_inline_lists_toggle_shortcut(
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
        KeyCode::Char('i') | KeyCode::Char('I') | KeyCode::Char('/') | KeyCode::Char('?') | KeyCode::Char('\u{1f}')
    )
}

pub(crate) fn maybe_show_help_modal(session: &mut Session) -> bool {
    if session.core.input_manager.content().trim() != "/help" {
        return false;
    }

    clear_submitted_input(session);
    session.show_help_modal();
    true
}

pub(crate) fn can_cycle_primary_agent(session: &Session, key: &KeyEvent) -> bool {
    // Agent/mode switching lives on Shift+Tab only (BackTab). Plain Tab
    // enqueues the draft like Ctrl+Enter, so it must not cycle.
    let valid_modifiers = match key.code {
        KeyCode::BackTab => key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT,
        KeyCode::Tab => key.modifiers == KeyModifiers::SHIFT,
        KeyCode::Char('\t') => key.modifiers == KeyModifiers::SHIFT,
        _ => false,
    };
    valid_modifiers && session.visible_transient_surface().is_none() && !session.has_active_overlay()
}

/// Notice shown when the user requests a mode switch (primary-agent cycle or
/// planning workflow) while a turn is actively processing. Mode switches are
/// locked for the duration of a turn to keep agent state consistent.
pub(crate) fn push_mode_switch_busy_notice(session: &mut Session) {
    session.push_line(
        InlineMessageKind::Warning,
        vec![InlineSegment {
            text: mode_switch_guard::MODE_SWITCH_BUSY_NOTICE.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.core.request_transcript_clear();
    session.mark_dirty();
}

impl mode_switch_guard::ModeSwitchGuardSession for Session {
    fn is_running_activity(&self) -> bool {
        TuiSessionDriver::is_running_activity(self)
    }

    fn is_mode_switch_locked(&self) -> bool {
        self.core.activity_state.locks_mode_switch()
    }

    fn can_cycle_primary_agent(&self, key: &KeyEvent) -> bool {
        can_cycle_primary_agent(self, key)
    }

    fn notify_mode_switch_busy(&mut self) {
        push_mode_switch_busy_notice(self);
    }
}
