#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;

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
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
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
    assert!(!session.core.build_input_widget_data(VIEW_WIDTH, 1).cursor_should_be_visible);

    let _ = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(!session.history_picker_state.active);
    assert!(session.core.input_enabled());
    assert!(session.core.build_input_widget_data(VIEW_WIDTH, 1).cursor_should_be_visible);
    assert_eq!(session.core.input_manager.content(), "draft command");
}

#[test]
fn history_picker_ctrl_c_dismisses_and_restores_draft() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("draft command".to_string());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(session.history_picker_state.active);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert!(event.is_none(), "Ctrl+C should dismiss without interrupting");
    assert!(!session.history_picker_state.active);
    assert!(session.history_picker_state.search_query.is_empty());
    assert_eq!(session.core.input_manager.content(), "draft command");
    assert!(session.core.input_enabled());
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
        .position(|line| line.contains('>'))
        .expect("search field should render");
    let item_index = lines
        .iter()
        .rposition(|line| line.contains("git status"))
        .expect("history match should render");

    assert!(search_index < item_index);
}

#[test]
fn history_picker_seeds_filter_from_composer_draft() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("cargo test".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.core.set_input("git status".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // A draft in the composer pre-filters history as soon as Ctrl+R opens.
    session.core.set_input("git".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

    assert!(session.history_picker_state.active);
    assert_eq!(session.history_picker_state.search_query, "git");
    assert!(!session.history_picker_state.matches.is_empty(), "seeded query should still match history");
    assert!(
        session
            .history_picker_state
            .matches
            .iter()
            .all(|history_match| history_match.content.to_lowercase().contains("git")),
        "every match must satisfy the seeded draft"
    );
}

#[test]
fn arrow_up_stays_within_multiline_without_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first message".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.set_input("second".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    session.set_input("line1\nline2\nline3".to_string());
    assert_eq!(session.input_manager.cursor_row(), 2);

    // First two Ups move the cursor, consume the key, emit no history event.
    let up_cursor_1 = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_cursor_1.is_none());
    assert_eq!(session.input_manager.content(), "line1\nline2\nline3");
    assert_eq!(session.input_manager.cursor_row(), 1);
    assert!(session.input_manager.history_index().is_none());

    let up_cursor_2 = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_cursor_2.is_none());
    assert_eq!(session.input_manager.cursor_row(), 0);

    // At first line, Up is a no-op: multiline never traverses history.
    let up_edge = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_edge.is_none());
    assert_eq!(session.input_manager.content(), "line1\nline2\nline3");
    assert_eq!(session.input_manager.cursor_row(), 0);
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn arrow_down_stays_within_multiline_without_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("only entry".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    session.set_input("aaa\nbbb".to_string());
    session.set_cursor(0);
    assert_eq!(session.input_manager.cursor_row(), 0);

    let down_cursor = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down_cursor.is_none());
    assert_eq!(session.input_manager.content(), "aaa\nbbb");
    assert_eq!(session.input_manager.cursor_row(), 1);
    assert!(session.input_manager.history_index().is_none());

    // At last line, Down is a no-op: multiline never traverses history.
    let down_edge = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down_edge.is_none());
    assert_eq!(session.input_manager.content(), "aaa\nbbb");
    assert_eq!(session.input_manager.cursor_row(), 1);
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn multiline_up_down_with_active_history_stays_in_buffer() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.set_input("second".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Enter history from empty single-line draft.
    let up = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(matches!(up, Some(InlineEvent::HistoryPrevious)));
    assert_eq!(session.input_manager.content(), "second");
    assert!(session.input_manager.history_index().is_some());

    // Typing a newline keeps history navigation state (insert does not reset)
    // and makes the buffer multiline.
    session.insert_char('\n');
    assert!(!session.input_manager.is_single_line());
    assert_eq!(session.input_manager.content(), "second\n");

    // Asymmetric pair: Up at first row vs Down at last row both stay in buffer.
    session.set_cursor(0);
    let up_stays = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_stays.is_none());
    assert_eq!(session.input_manager.content(), "second\n");

    session.set_cursor(session.input_manager.content().len());
    let down_stays = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down_stays.is_none());
    assert_eq!(session.input_manager.content(), "second\n");

    // History navigation stays armed while the buffer is multiline; Ctrl+P/N
    // remain the escape hatch back into history traversal.
    assert!(session.input_manager.history_index().is_some());
    let ctrl_p = session.process_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert!(ctrl_p.is_none());
    assert_eq!(session.input_manager.content(), "first");
}

#[test]
fn ctrl_p_navigates_history_even_when_multiline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("alpha".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.set_input("beta".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    session.set_input("line1\nline2".to_string());
    assert!(!session.input_manager.is_single_line());

    let ctrl_p = session.process_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert!(ctrl_p.is_none());
    assert_eq!(session.input_manager.content(), "beta");
}

#[test]
fn wrapped_single_line_up_moves_within_visual_rows() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.set_input("second".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // 90 ascii chars at width 30 (empty prompt prefix) wrap to 3 visual rows.
    let draft = "a".repeat(90);
    session.set_input(draft.clone());
    session.set_input_area(Some(Rect::new(0, 0, 30, 8)));
    let geometry = session.input_visual_geometry().expect("visual geometry");
    assert_eq!((geometry.total_rows, geometry.cursor_row), (3, 2));
    assert!(session.is_multi_row_composer());

    // Up from the last visual row moves within the buffer, never history.
    let up_1 = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_1.is_none());
    assert_eq!(session.input_manager.content(), draft);
    assert!(session.input_manager.history_index().is_none());
    assert_eq!(session.cursor(), 60);

    let up_2 = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_2.is_none());
    assert_eq!(session.cursor(), 30);

    // Up at the first visual row is a no-op, still no history.
    let up_edge = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_edge.is_none());
    assert_eq!(session.cursor(), 30);
    assert_eq!(session.input_manager.content(), draft);
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn wrapped_single_line_down_moves_within_visual_rows() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("only entry".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // 90 ascii chars at width 30 (empty prompt prefix) wrap to 3 visual rows.
    let draft = "b".repeat(90);
    session.set_input(draft.clone());
    session.set_input_area(Some(Rect::new(0, 0, 30, 8)));
    assert!(session.is_multi_row_composer());

    session.set_cursor(0);
    let down_1 = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down_1.is_none());
    assert_eq!(session.input_manager.content(), draft);
    assert!(session.input_manager.history_index().is_none());
    assert_eq!(session.cursor(), 30);

    // Down drains to the end of the buffer without touching history.
    for _ in 0..5 {
        let event = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(event.is_none());
        assert_eq!(session.input_manager.content(), draft);
        assert!(session.input_manager.history_index().is_none());
    }
    assert_eq!(session.cursor(), draft.len());

    // Down at the last visual row is a no-op, still no history.
    let down_edge = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down_edge.is_none());
    assert_eq!(session.cursor(), draft.len());
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn single_visual_row_up_still_navigates_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("alpha".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.set_input("beta".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Short draft at a wide area renders as a single visual row.
    session.set_input("hi".to_string());
    session.set_input_area(Some(Rect::new(0, 0, 200, 8)));
    assert_eq!(session.input_visual_geometry().map(|geometry| geometry.total_rows), Some(1));
    assert!(!session.is_multi_row_composer());

    let up = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(matches!(up, Some(InlineEvent::HistoryPrevious)));
    assert_eq!(session.input_manager.content(), "beta");
}

#[test]
fn ctrl_p_navigates_history_even_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("alpha".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.set_input("beta".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    session.set_input("c".repeat(90));
    session.set_input_area(Some(Rect::new(0, 0, 30, 8)));
    assert!(session.is_multi_row_composer());

    let ctrl_p = session.process_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert!(ctrl_p.is_none());
    assert_eq!(session.input_manager.content(), "beta");
}

#[test]
fn app_session_wrapped_single_line_up_down_never_traverses_history() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("first".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.core.set_input("second".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let draft = "d".repeat(90);
    session.core.set_input(draft.clone());
    session.core.set_input_area(Some(Rect::new(0, 0, 30, 8)));
    assert!(session.is_multi_row_composer());

    let up = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up.is_none());
    assert_eq!(session.core.input_manager.content(), draft);
    assert!(session.core.input_manager.history_index().is_none());

    session.core.set_cursor(0);
    let down = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down.is_none());
    assert_eq!(session.core.input_manager.content(), draft);
    assert!(session.core.input_manager.history_index().is_none());
}

#[test]
fn app_session_multiline_up_down_never_traverses_history() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("first".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.core.set_input("second".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    session.core.set_input("line1\nline2".to_string());
    assert!(!session.core.input_manager.is_single_line());

    let up_move = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_move.is_none());
    assert_eq!(session.core.input_manager.content(), "line1\nline2");

    let up_edge = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(up_edge.is_none());
    assert_eq!(session.core.input_manager.content(), "line1\nline2");
    assert!(session.core.input_manager.history_index().is_none());

    let down_move = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down_move.is_none());
    assert_eq!(session.core.input_manager.content(), "line1\nline2");

    let down_edge = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down_edge.is_none());
    assert_eq!(session.core.input_manager.content(), "line1\nline2");
    assert!(session.core.input_manager.history_index().is_none());
}

#[test]
fn single_line_up_down_still_navigates_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("alpha".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.set_input("beta".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(session.input_manager.is_single_line());
    let up = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(matches!(up, Some(InlineEvent::HistoryPrevious)));
    assert_eq!(session.input_manager.content(), "beta");

    // History walks newest-first: Down from newest steps to the older entry,
    // then to the saved empty draft.
    let down = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(down, Some(InlineEvent::HistoryNext)));
    assert_eq!(session.input_manager.content(), "alpha");

    let down_draft = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(down_draft, Some(InlineEvent::HistoryNext)));
    assert!(session.input_manager.content().is_empty());
}

#[test]
fn shift_up_does_not_consume_multiline_cursor_move() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("line1\nline2\nline3".to_string());
    assert_eq!(session.input_manager.cursor_row(), 2);

    // Shift+Up must not be consumed as a plain cursor move (which would
    // discard selection semantics); it keeps the prior fallback behavior.
    let shifted = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
    assert!(shifted.is_none());
    assert_eq!(session.input_manager.content(), "line1\nline2\nline3");
    assert_eq!(session.input_manager.cursor_row(), 2);
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn history_picker_collapses_multiline_with_indicator_and_accepts_full() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let multiline = "cargo test --marker\nUNIQUE_SECOND_LINE_MARKER\nthird";
    session.core.set_input(multiline.to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let lines = rendered_app_session_lines(&mut session, 24);
    let collapsed = lines
        .iter()
        .find(|line| line.contains("cargo test"))
        .expect("collapsed multiline match should render");
    assert!(collapsed.contains("+2 lines"), "collapsed row should indicate extra lines: {collapsed}");
    assert!(
        !lines.iter().any(|line| line.contains("UNIQUE_SECOND_LINE_MARKER")),
        "second line must not leak as separate row"
    );

    // Navigate (single entry already selected) and accept full content.
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!session.history_picker_state.active);
    assert_eq!(session.core.input_manager.content(), multiline);
}
