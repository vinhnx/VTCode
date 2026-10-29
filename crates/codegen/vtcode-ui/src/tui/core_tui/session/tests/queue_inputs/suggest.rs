use super::*;

#[test]
fn apply_suggested_prompt_replaces_empty_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.apply_suggested_prompt("Review the latest diff.".to_string());

    assert_eq!(session.input_manager.content(), "Review the latest diff.");
    assert!(session.suggested_prompt_state.active);
}
#[test]
fn apply_suggested_prompt_appends_to_existing_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Initial draft".to_string());

    session.apply_suggested_prompt("Review the latest diff.".to_string());

    assert_eq!(session.input_manager.content(), "Initial draft\n\nReview the latest diff.");
    assert!(session.suggested_prompt_state.active);
}
#[test]
fn suggested_prompt_state_clears_after_manual_edit() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.apply_suggested_prompt("Review the latest diff.".to_string());

    session.insert_char('!');

    assert!(!session.suggested_prompt_state.active);
}
#[test]
fn alt_p_requests_inline_prompt_suggestion() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT));

    assert!(matches!(
        event,
        Some(InlineEvent::RequestInlinePromptSuggestion(ref value))
            if value == "Review the current"
    ));
}
#[test]
fn tab_accepts_visible_inline_prompt_suggestion() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current".to_string());
    session.set_inline_prompt_suggestion("Review the current.diff".to_string(), true);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "Review the current.diff");
    assert!(session.inline_prompt_suggestion.suggestion.is_none());
}
#[test]
fn tab_accepts_inline_prompt_suggestion_with_trailing_space_prefix() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current.diff ".to_string());
    session.set_inline_prompt_suggestion("Review the current.diff and summarize it".to_string(), true);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "Review the current.diff and summarize it");
    assert!(session.inline_prompt_suggestion.suggestion.is_none());
}
#[test]
fn tab_cycles_primary_agent_when_no_inline_prompt_suggestion_is_visible() {
    // Without a suggestion, plain Tab submits like Ctrl+Enter.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current.diff".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "Review the current.diff"));
    assert_eq!(session.input_manager.content(), "");

    session.set_input("Review the current.diff".to_string());
    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(InlineEvent::CyclePrimaryAgent)));
    assert_eq!(session.input_manager.content(), "Review the current.diff");
}
#[test]
fn inline_prompt_suggestion_clears_after_cursor_movement() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current".to_string());
    session.set_inline_prompt_suggestion("Review the current.diff".to_string(), false);

    let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(session.inline_prompt_suggestion.suggestion.is_none());
}
