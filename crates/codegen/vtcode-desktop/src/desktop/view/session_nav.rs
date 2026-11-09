//! Threads-panel navigation: Up/Down/Home/End highlight, Enter to load.
//!
//! Mirrors [`DesktopView::move_transcript_focus`]: a keyboard highlight over
//! the thread lists that stays distinct from the loaded selection, with
//! position memory across renders. The sidebar, the thread browser, and the
//! tab strip share [`DesktopView::keyboard_focused_session`] and step it
//! through their own row order via [`step_highlight_order`]. Stale ids
//! self-heal — an id that no longer resolves restarts movement from the list
//! edge — so session-list refreshes need no explicit invalidation.

use super::*;

/// Step a session-id highlight across `order`, clamping at both ends.
/// Unknown or missing ids restart from the edge in the travel direction.
/// Total: empty orders yield `None`, and the returned index is always in
/// bounds (`order` is non-empty below, so `len() - 1` cannot underflow).
pub(super) fn step_highlight_order(order: &[&str], current: Option<&str>, down: bool) -> Option<usize> {
    if order.is_empty() {
        return None;
    }
    let position = current.and_then(|id| order.iter().position(|candidate| *candidate == id));
    Some(match (position, down) {
        (Some(index), true) => index.saturating_add(1).min(order.len() - 1),
        (Some(index), false) => index.saturating_sub(1),
        (None, true) => 0,
        (None, false) => order.len() - 1,
    })
}

impl DesktopView {
    /// Move the keyboard highlight across threads (Up/Down with the session
    /// list focused). Clamps at both ends and scrolls the row into view.
    pub(super) fn move_session_focus(&mut self, down: bool, cx: &mut Context<Self>) {
        let next = {
            let order: Vec<&str> = self.sessions.iter().map(|session| session.session_id.as_str()).collect();
            step_highlight_order(&order, self.keyboard_focused_session.as_deref(), down)
        };
        let Some(next) = next else {
            return;
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
        if self.session_known(&session_id) && self.load_session_from_ui(session_id, window, cx) {
            self.request_panel_dismiss(window, cx);
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

    /// Delete the highlighted thread (Backspace/Delete with a thread list
    /// focused). Routes through the same confirm-gated flow as the
    /// right-click menu, including its selected-thread and busy-state
    /// refusals; a missing or vanished highlight is a no-op.
    pub(super) fn delete_highlighted_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.keyboard_focused_session.clone() else {
            return;
        };
        if !self.session_known(&session_id) {
            return;
        }
        self.confirm_delete_thread(&session_id, window, cx);
    }

    /// Whether a session id still names a cached thread. Enter/Delete consult
    /// this so a highlight left on a vanished thread never acts on a ghost.
    fn session_known(&self, session_id: &str) -> bool {
        self.sessions.iter().any(|session| session.session_id == session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_highlight_order_clamps_and_heals_from_edges() {
        let order = ["first", "mid", "last"];
        assert_eq!(step_highlight_order(&order, None, true), Some(0));
        assert_eq!(step_highlight_order(&order, None, false), Some(2));
        assert_eq!(step_highlight_order(&order, Some("gone"), true), Some(0));
        assert_eq!(step_highlight_order(&order, Some("gone"), false), Some(2));
        assert_eq!(step_highlight_order(&order, Some("last"), true), Some(2));
        assert_eq!(step_highlight_order(&order, Some("first"), false), Some(0));
        assert_eq!(step_highlight_order(&order, Some("mid"), true), Some(2));
        assert_eq!(step_highlight_order(&order, Some("mid"), false), Some(0));
        assert_eq!(step_highlight_order(&[], Some("first"), true), None);
    }
}
