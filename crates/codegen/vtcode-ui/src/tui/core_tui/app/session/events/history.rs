//! Command-history picker support.

use super::*;

pub(crate) fn input_history_entries(
    session: &Session,
) -> Vec<(String, Vec<ContentPart>, chrono::DateTime<chrono::Utc>)> {
    // Live entries only: archived prompts arrive through the picker's own
    // scoped list, and the merged history would bypass scope filtering.
    session
        .core
        .input_manager
        .live_history()
        .iter()
        .map(|entry| (entry.content().to_string(), entry.attachment_elements(), entry.timestamp()))
        .collect()
}

pub(crate) fn open_history_picker(session: &mut Session) {
    if session.history_picker_state.active {
        return;
    }

    session.ensure_inline_lists_visible_for_trigger();
    session.show_transient_surface(TransientSurface::HistoryPicker);
    session.history_picker_state.open(&session.core.input_manager);
    let history = input_history_entries(session);
    session.history_picker_state.update_search(&history);
    session.mark_dirty();
}
