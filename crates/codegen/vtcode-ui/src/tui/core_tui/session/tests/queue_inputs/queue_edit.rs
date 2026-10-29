use super::*;

#[test]
fn alt_up_edits_latest_queued_input() {
    with_terminal_env(None, Some("xterm-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        set_queued_inputs(&mut session, vec!["first".to_string(), "second".to_string()]);

        let event = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert!(matches!(event, Some(InlineEvent::EditQueue)));
        assert_eq!(session.input_manager.content(), "second");
    });
}
#[test]
fn shift_left_edits_latest_queued_input_in_tmux() {
    with_terminal_env(Some("/tmp/tmux-1000/default,123,0"), Some("tmux-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        set_queued_inputs(&mut session, vec!["first".to_string(), "second".to_string()]);

        let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(matches!(event, Some(InlineEvent::EditQueue)));
        assert_eq!(session.input_manager.content(), "second");
    });
}
#[test]
fn app_session_shift_left_edits_latest_queued_input_in_tmux() {
    with_terminal_env(Some("/tmp/tmux-1000/default,123,0"), Some("tmux-256color"), || {
        let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

        set_app_session_queued_inputs(&mut session, vec!["first".to_string(), "second".to_string()]);

        let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(matches!(event, Some(app_types::InlineEvent::EditQueue)));
        assert_eq!(session.core.input_manager.content(), "second");
    });
}
#[test]
fn consecutive_duplicate_submissions_not_stored_twice() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("repeat".to_string());
    let first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Submit(value)) if value == "repeat"));

    session.set_input("repeat".to_string());
    let second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(second, Some(InlineEvent::Submit(value)) if value == "repeat"));

    assert_eq!(session.input_manager.history().len(), 1);
}
#[test]
fn queued_inputs_overlay_bottom_rows() {
    with_terminal_env(None, Some("xterm-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        set_queued_inputs(
            &mut session,
            vec![
                "first queued message".to_string(),
                "second queued message".to_string(),
                "third queued message".to_string(),
            ],
        );

        assert_footer_contains(&mut session, 10, "\u{21B3} [3/3] third queued message");
        assert_footer_contains(&mut session, 10, "\u{21B3} [2/3] second queued message");
        assert_footer_contains(&mut session, 10, &queue_edit_hint());
    });
}
#[test]
fn queued_inputs_overlay_shows_shift_left_hint_in_tmux() {
    with_terminal_env(Some("/tmp/tmux-1000/default,123,0"), Some("tmux-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        set_queued_inputs(&mut session, vec!["first queued message".to_string(), "second queued message".to_string()]);

        assert_footer_contains(&mut session, 10, &tmux_queue_edit_hint());
    });
}
#[test]
fn running_activity_not_overlaid_above_queue_lines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_queued_inputs(&mut session, vec!["first queued message".to_string(), "second queued message".to_string()]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let mut visible = vec![TranscriptLine::default(); 6];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(|line| line_text(&line.line)).collect();

    assert!(
        !rendered.iter().any(|line| line.contains("Running command: test")),
        "running status should not be overlaid in transcript"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("\u{21B3} [2/2] second queued message")),
        "latest queued message should remain visible"
    );
    assert!(
        rendered.iter().any(|line| line.contains("\u{21B3} [1/2] first queued message")),
        "older queued message should remain visible"
    );
}
#[test]
fn running_activity_not_overlaid_without_queue() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: grep".to_string()),
        right: None,
    });

    let mut visible = vec![TranscriptLine::default(); 3];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(|line| line_text(&line.line)).collect();

    assert!(
        !rendered.iter().any(|line| line.contains("Running tool: grep")),
        "running status should render only in bottom input status row"
    );
}
