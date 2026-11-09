use super::*;

#[test]
fn idle_enter_submits_immediately() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "queued"));
}
#[test]
fn control_enter_submits_current_draft_immediately() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("process now".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "process now"));
}
#[test]
fn idle_control_enter_with_empty_input_processes_latest_queued_message() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_queued_input("first queued".to_string());
    session.push_queued_input("latest queued".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::ProcessLatestQueued)));
}
#[test]
fn control_l_submits_clear_command() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let event = session.process_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "/clear"));
}
#[test]
fn control_slash_toggles_inline_list_visibility() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
}
#[test]
fn control_i_toggles_inline_list_visibility() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
}
#[test]
fn tab_cycles_primary_agent_with_submission_text() {
    // Plain Tab now submits like Ctrl+Enter; Shift+Tab cycles.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "queued"));
    assert_eq!(session.input_manager.content(), "");

    session.set_input("queued".to_string());
    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
    assert!(matches!(cycle, Some(InlineEvent::CyclePrimaryAgent)));
    assert_eq!(session.input_manager.content(), "queued");
}
#[test]
fn busy_escape_interrupts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Interrupt)));

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(second, Some(InlineEvent::Interrupt)));
}
#[test]
fn repeated_control_c_exits_without_double_escape() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let first = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(matches!(first, Some(InlineEvent::Interrupt)));

    let second = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(matches!(second, Some(InlineEvent::Exit)));
}
#[test]
fn double_idle_escape_submits_rewind() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Cancel)));

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(&second, Some(InlineEvent::Submit(value)) if value == "/rewind"));

    // A third press starts a fresh single-press cancel cycle.
    let third = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(third, Some(InlineEvent::Cancel)));
}
#[test]
fn busy_stop_command_interrupts_immediately() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/stop".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(app_types::InlineEvent::Interrupt)));
}
#[test]
fn busy_pause_command_emits_pause_event() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/pause".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(app_types::InlineEvent::Pause)));
}
#[test]
fn busy_resume_command_emits_resume_event() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/resume".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(app_types::InlineEvent::Resume)));
}

fn push_scrolled_transcript(session: &mut Session) {
    session.apply_transcript_width(VIEW_WIDTH);
    session.apply_transcript_rows(6);
    for index in 0..10 {
        session.push_line(InlineMessageKind::Agent, vec![make_segment(&format!("line {index}"))]);
    }
    session.ensure_scroll_metrics();
    session.scroll_page_up();
    assert!(session.scroll_offset() > 0, "fixture must leave the bottom");
}

#[test]
fn scrolled_enter_with_draft_jumps_to_latest_before_submitting() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_scrolled_transcript(&mut session);
    session.set_input("keep this draft".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(event.is_none(), "first Enter jumps instead of submitting");
    assert_eq!(session.scroll_offset(), 0);
    assert_eq!(session.input_manager.content(), "keep this draft");

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(event, Some(InlineEvent::Submit(value)) if value == "keep this draft"),
        "second Enter submits"
    );
}

#[test]
fn app_scrolled_enter_with_draft_jumps_to_latest_before_submitting() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    push_scrolled_transcript(&mut session.core);
    session.core.set_input("app draft".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(event.is_none(), "first Enter jumps instead of submitting");
    assert_eq!(session.core.scroll_offset(), 0);
    assert_eq!(session.core.input_manager.content(), "app draft");

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(event, Some(app_types::InlineEvent::Submit(value)) if value == "app draft"),
        "second Enter submits"
    );
}

#[test]
fn scrolled_enter_with_empty_draft_rejoins_tail_silently() {
    // Pre-existing take behavior: submitting (even blank) re-follows the
    // tail. The jump gate only changes the non-empty case (draft kept).
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_scrolled_transcript(&mut session);
    session.set_input("   ".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(event.is_none(), "blank Enter submits nothing");
    assert_eq!(session.scroll_offset(), 0, "blank Enter rejoins the tail");
}

#[test]
fn scrolled_control_enter_keeps_immediate_submit() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_scrolled_transcript(&mut session);
    session.set_input("send now".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(
        matches!(event, Some(InlineEvent::Submit(value)) if value == "send now"),
        "explicit queue intent bypasses the jump"
    );
}
