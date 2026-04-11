use super::helpers::*;
use super::super::*;

#[test]
fn arrow_keys_navigate_input_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("first message".to_string());
    let submit_first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(submit_first, Some(InlineEvent::Submit(value)) if value == "first message"));

    session.set_input("second".to_string());
    let submit_second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(submit_second, Some(InlineEvent::Submit(value)) if value == "second"));

    assert_eq!(session.input_manager.history().len(), 2);
    assert!(session.input_manager.content().is_empty());

    let up_latest = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(matches!(up_latest, Some(InlineEvent::HistoryPrevious)));
    assert_eq!(session.input_manager.content(), "second");

    let up_previous = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(matches!(up_previous, Some(InlineEvent::HistoryPrevious)));
    assert_eq!(session.input_manager.content(), "first message");

    let down_forward = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(down_forward, Some(InlineEvent::HistoryNext)));
    assert!(session.input_manager.content().is_empty());
    assert!(session.input_manager.history_index().is_none());

    let down_restore = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert!(down_restore.is_none());
    assert!(session.input_manager.content().is_empty());
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn down_keeps_history_navigation_when_history_is_active() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(
            app_types::LocalAgentKind::Delegated,
        )],
    });
    session.close_transient();

    session.core.set_input("first".to_string());
    let first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(first, Some(app_types::InlineEvent::Submit(value)) if value == "first"));

    session.core.set_input("second".to_string());
    let second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(second, Some(app_types::InlineEvent::Submit(value)) if value == "second"));

    let up = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(matches!(up, Some(app_types::InlineEvent::HistoryPrevious)));

    let down = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(down, Some(app_types::InlineEvent::HistoryNext)));
    assert!(!session.local_agents_visible());
}

#[test]
fn history_picker_trigger_auto_shows_inline_lists() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
    assert!(session.history_picker_state.active);
}

#[test]
fn history_picker_restores_base_input_and_draft_on_cancel() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("draft command".to_string());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

    assert!(session.history_picker_state.active);
    assert!(!session.core.input_enabled());
    assert!(
        !session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );

    let _ = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(!session.history_picker_state.active);
    assert!(session.core.input_enabled());
    assert!(
        session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );
    assert_eq!(session.core.input_manager.content(), "draft command");
}

#[test]
fn history_picker_renders_search_field_above_results() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("cargo test".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.core.set_input("git status".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    let lines = rendered_app_session_lines(&mut session, 20);
    let search_index = lines
        .iter()
        .position(|line| line.contains("Search history: [gi"))
        .expect("search history field should render");
    let item_index = lines
        .iter()
        .rposition(|line| line.contains("git status"))
        .expect("history match should render");

    assert!(search_index < item_index);
}

