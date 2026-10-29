use super::*;

#[test]
fn meta_click_emits_open_file_event_for_transcript_path() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    // Some terminals (Ghostty, iTerm2) report Cmd as META instead of SUPER
    left_click_session(&mut fixture.session, &tx, area.x, area.y, KeyModifiers::META);

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
}
#[cfg(target_os = "macos")]
#[test]
fn ctrl_click_does_not_emit_open_file_event_on_macos() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture.session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::CONTROL,
        }),
        &tx,
        None,
    );

    rx.try_recv().unwrap_err();
    assert_eq!(fixture.session.mouse_drag_target, MouseDragTarget::None);
    assert!(!fixture.session.mouse_selection.is_selecting);
    assert!(!fixture.session.mouse_selection.has_selection);
}
#[cfg(target_os = "macos")]
#[test]
fn command_key_press_then_plain_click_emits_open_file_event_on_macos() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture
        .session
        .handle_event(CrosstermEvent::Key(command_modifier_press_event()), &tx, None);
    left_click_session(&mut fixture.session, &tx, area.x, area.y, KeyModifiers::NONE);

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));

    fixture
        .session
        .handle_event(CrosstermEvent::Key(command_modifier_release_event()), &tx, None);
    left_click_session(&mut fixture.session, &tx, area.x, area.y, KeyModifiers::NONE);

    rx.try_recv().unwrap_err();
}
#[cfg(target_os = "macos")]
#[test]
fn meta_key_press_then_plain_click_emits_open_file_event_on_macos() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture
        .session
        .handle_event(CrosstermEvent::Key(meta_modifier_press_event()), &tx, None);
    left_click_session(&mut fixture.session, &tx, area.x, area.y, KeyModifiers::NONE);

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
}
#[test]
fn app_session_modifier_click_emits_open_file_event_for_transcript_path() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    left_click_app_session(&mut fixture.session, &tx, area.x, area.y, open_file_click_modifiers());

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    assert_eq!(fixture.session.core.mouse_drag_target, MouseDragTarget::None);
    assert!(!fixture.session.core.mouse_selection.is_selecting);
    assert!(!fixture.session.core.mouse_selection.has_selection);
}
#[test]
fn app_session_double_click_emits_open_file_event_for_transcript_path() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    };
    left_click_app_session(&mut fixture.session, &tx, click.column, click.row, click.modifiers);
    left_click_app_session(&mut fixture.session, &tx, click.column, click.row, click.modifiers);

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    rx.try_recv().unwrap_err();
}
#[test]
fn app_session_double_click_emits_open_url_event_for_modal_auth_link() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://auth.openai.com/oauth/authorize?client_id=test".to_string();
    session.show_transient(wizard_auth_transient(&url));

    let _ = rendered_app_session_lines(&mut session, 20);
    let target = session
        .core
        .modal_link_targets()
        .first()
        .expect("expected modal url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: target.area.x,
        row: target.area.y,
        modifiers: KeyModifiers::NONE,
    };
    left_click_app_session(&mut session, &tx, click.column, click.row, click.modifiers);
    left_click_app_session(&mut session, &tx, click.column, click.row, click.modifiers);

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
    rx.try_recv().unwrap_err();
}
#[cfg(target_os = "macos")]
#[test]
fn app_session_command_key_press_then_plain_click_emits_open_file_event_on_macos() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture
        .session
        .handle_event(CrosstermEvent::Key(command_modifier_press_event()), &tx, None);
    left_click_app_session(&mut fixture.session, &tx, area.x, area.y, KeyModifiers::NONE);

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
}
#[cfg(target_os = "macos")]
#[test]
fn app_session_ctrl_click_on_link_is_consumed_without_selection() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture.session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::CONTROL,
        }),
        &tx,
        None,
    );

    rx.try_recv().unwrap_err();
    assert_eq!(fixture.session.core.mouse_drag_target, MouseDragTarget::None);
    assert!(!fixture.session.core.mouse_selection.is_selecting);
    assert!(!fixture.session.core.mouse_selection.has_selection);
}
#[test]
fn scroll_between_clicks_clears_double_click_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("hello world")]);

    let (transcript_area, rendered) = rendered_transcript_lines(&mut session, VIEW_ROWS * 2);
    let row = rendered
        .iter()
        .position(|line| line.contains("hello world"))
        .expect("expected hello world to be rendered");
    let column =
        rendered[row].find("hello").expect("expected hello word in rendered line") as u16 + transcript_area.x + 1;
    let row = transcript_area.y + row as u16;

    let (tx, _rx) = mpsc::unbounded_channel();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let release = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let scroll = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };

    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(scroll), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);

    assert!(!session.mouse_selection.has_selection);
}
