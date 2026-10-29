use super::*;

#[test]
fn ctrl_o_emits_copy_command() {
    let mut session = build_session();
    session
        .core
        .push_line(InlineMessageKind::Agent, vec![text_segment("agent reply")]);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert!(
        matches!(event, Some(InlineEvent::Submit(ref cmd)) if cmd == "/copy"),
        "Ctrl+O should emit Submit(\"/copy\"), got {event:?}"
    );
}
#[test]
fn ctrl_t_opens_and_closes_tool_output_viewer_in_fullscreen() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec!["• Ran echo hello".to_string(), "  └ hello".to_string()],
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    assert!(session.process_key(key).is_none());
    assert!(session.tool_output_viewer_state().is_some());

    assert!(session.process_key(key).is_none());
    assert!(session.tool_output_viewer_state().is_none());
}
#[test]
fn ctrl_t_opens_tool_output_viewer_outside_fullscreen() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.core.input_manager.set_content("abc".to_string());
    session.core.input_manager.set_cursor(1);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.core.input_manager.content(), "abc");
    assert!(session.tool_output_viewer_state().is_some());
}
#[test]
fn raw_ctrl_t_opens_tool_output_viewer() {
    let mut session = Session::new(InlineTheme::default(), None, 24);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{14}'), KeyModifiers::empty()));

    assert!(result.is_none());
    assert!(session.tool_output_viewer_state().is_some());
}
#[test]
fn unbound_ctrl_t_keeps_readline_transpose_outside_fullscreen() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), Vec::new());
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.input_manager.set_content("abc".to_string());
    session.core.input_manager.set_cursor(1);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.core.input_manager.content(), "bac");
    assert!(session.tool_output_viewer_state().is_none());
}
#[test]
fn unbound_raw_ctrl_t_keeps_readline_transpose_outside_fullscreen() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), Vec::new());
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.input_manager.set_content("abc".to_string());
    session.core.input_manager.set_cursor(1);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{14}'), KeyModifiers::empty()));

    assert!(result.is_none());
    assert_eq!(session.core.input_manager.content(), "bac");
    assert!(session.tool_output_viewer_state().is_none());
}
#[test]
fn unbound_transcript_review_does_not_restore_ctrl_t_viewer_alias() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), Vec::new());
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.set_fullscreen_active(true);
    add_compact_activity(&mut session, 40, "printf unbound");

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert!(session.tool_output_viewer_state().is_none());

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render compact activity");
    assert!(session.compact_activity_hit_regions.is_empty());
}
#[test]
fn transcript_review_binding_can_open_outside_fullscreen() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), vec!["ctrl+x".to_string()]);
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );

    assert!(!session.core.fullscreen.active);
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert!(session.tool_output_viewer_state().is_some());
}
#[test]
fn configured_core_action_dispatches_through_app_session() {
    let mut bindings = HashMap::new();
    bindings.insert("open_model_picker".to_string(), vec!["ctrl+x".to_string()]);
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );

    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
        Some(InlineEvent::Submit(ref command)) if command == "/model"
    ));
}
#[test]
fn raw_control_exit_is_normalized_before_app_dispatch() {
    let mut session = build_session();

    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Char('\u{4}'), KeyModifiers::NONE)),
        Some(InlineEvent::Exit)
    ));
}
#[test]
fn repeated_tui_ctrl_c_exits_through_the_event_path() {
    let mut session = build_session();
    let (events, mut received) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), &events, None);
    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('\u{3}'), KeyModifiers::NONE)), &events, None);

    assert!(matches!(received.try_recv(), Ok(InlineEvent::Interrupt)));
    assert!(matches!(received.try_recv(), Ok(InlineEvent::Exit)));
}
#[test]
fn double_idle_escape_submits_rewind_in_the_app_session() {
    let mut session = build_session();

    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(InlineEvent::Cancel)
    ));
    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(InlineEvent::Submit(value)) if value == "/rewind"
    ));
    // The double press consumed the armed timer, so the next press is a
    // fresh single-press cancel.
    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(InlineEvent::Cancel)
    ));
}
#[test]
fn transcript_review_render_mode_is_rebindable_inside_viewer() {
    let mut bindings = HashMap::new();
    bindings.insert("toggle_transcript_render_mode".to_string(), vec!["alt+x".to_string()]);
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.set_fullscreen_active(true);

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .is_none()
    );
    let initial_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(initial_status.contains("rich"));

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT))
            .is_none()
    );
    let raw_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(raw_status.contains("raw"));
}
#[test]
fn transcript_render_binding_does_not_consume_normal_input() {
    let mut session = build_session();

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .is_none()
    );
    assert_eq!(session.core.input_manager.content(), "r");
    assert!(session.tool_output_viewer_state().is_none());
}
#[test]
fn alt_o_compatibility_alias_opens_transcript_review() {
    let mut session = build_session();
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT))
            .is_none()
    );
    assert!(session.tool_output_viewer_state().is_some());
}
