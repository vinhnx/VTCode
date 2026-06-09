use super::Session;

pub(super) fn clear_input(session: &mut Session) {
    session.input_manager.clear();
    session.clear_suggested_prompt_state();
    session.clear_inline_prompt_suggestion();
    session.input_compact_mode = false;
    session.scroll_manager.set_offset(0);
    session.mark_dirty();
}
