use super::Session;

pub(super) fn clear_input(session: &mut Session) {
    session.input_manager.clear();
    session.clear_suggested_prompt_state();
    session.input_compact_mode = false;
    session.scroll_manager.set_offset(0);
    super::slash::update_slash_suggestions(session);
    session.mark_dirty();
}
