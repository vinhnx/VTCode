//! Threads-panel navigation: Up/Down/Home/End highlight, Enter to load.
//!
//! Mirrors [`DesktopView::move_transcript_focus`]: a keyboard highlight over
//! the sidebar session list that stays distinct from the loaded selection,
//! with position memory across renders. Stale ids self-heal — an id that no
//! longer resolves restarts movement from the list edge — so session-list
//! refreshes need no explicit invalidation.

use super::*;

impl DesktopView {
    /// Move the keyboard highlight across threads (Up/Down with the session
    /// list focused). Clamps at both ends and scrolls the row into view.
    pub(super) fn move_session_focus(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.sessions.is_empty() {
            return;
        }
        let current = self
            .keyboard_focused_session
            .as_deref()
            .and_then(|id| self.sessions.iter().position(|session| session.session_id.as_str() == id));
        let next = match (current, down) {
            (Some(position), true) => (position + 1).min(self.sessions.len() - 1),
            (Some(position), false) => position.saturating_sub(1),
            (None, true) => 0,
            (None, false) => self.sessions.len() - 1,
        };
        self.focus_session_at(next, cx);
    }

    /// Jump the highlight to the first/last thread (Home/End, vim `g`/`G`).
    pub(super) fn move_session_focus_edge(&mut self, first: bool, cx: &mut Context<Self>) {
        if self.sessions.is_empty() {
            return;
        }
        let index = if first { 0 } else { self.sessions.len() - 1 };
        self.focus_session_at(index, cx);
    }

    /// Load the highlighted thread (Enter). Reuses the row-click path, so
    /// guards (running-confirm, retained drafts) behave identically.
    pub(super) fn activate_focused_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.keyboard_focused_session.clone() else {
            return;
        };
        if self.sessions.iter().any(|session| session.session_id == session_id)
            && self.load_session_from_ui(session_id, window, cx)
        {
            self.close_active_route(window, cx);
        }
    }

    fn focus_session_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(session_id) = self.sessions.get(index).map(|session| session.session_id.clone()) else {
            return;
        };
        self.keyboard_focused_session = Some(session_id);
        // Same Kit limitation as the transcript: `scroll_to_item` pins to the
        // viewport top, so every step re-pins instead of revealing minimally.
        self.session_scroll.scroll_to_item(index, gpui_kit::ScrollStrategy::Top);
        cx.notify();
    }
}
