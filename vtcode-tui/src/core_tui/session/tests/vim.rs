use super::helpers::*;
use super::super::*;

#[test]
fn vim_mode_does_not_consume_control_shortcuts() {
    let mut session = app_session_with_input("hello", 5);
    enable_vim_normal_mode_app(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

    assert!(event.is_none());
    assert!(session.history_picker_state.active);
}

#[test]
fn vim_dd_deletes_the_current_logical_line() {
    let mut session = session_with_input("one\ntwo\nthree", 4);
    enable_vim_normal_mode(&mut session);

    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)));
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)));

    assert_eq!(session.input_manager.content(), "one\nthree");
    assert_eq!(session.cursor(), 4);
}

#[test]
fn vim_dot_repeats_last_delete_char_change() {
    let mut session = session_with_input("abc", 0);
    enable_vim_normal_mode(&mut session);

    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)));
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)));

    assert_eq!(session.input_manager.content(), "c");
}

#[test]
fn vim_dot_repeats_change_word_edits() {
    let mut session = session_with_input("alpha beta", 0);
    enable_vim_normal_mode(&mut session);

    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)));
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)));
    session.insert_paste_text("A");
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));

    session.set_cursor("A".len());
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)));

    assert_eq!(session.input_manager.content(), "AA");
}

#[test]
fn vim_dot_repeats_change_line_edits() {
    let mut session = session_with_input("one\ntwo", 0);
    enable_vim_normal_mode(&mut session);

    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)));
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)));
    session.insert_paste_text("ONE");
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));

    session.set_cursor("ONE\n".len());
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)));

    assert_eq!(session.input_manager.content(), "ONE\nONE");
}

#[test]
fn vim_dot_repeats_change_text_object_edits() {
    let mut session = session_with_input("\"alpha\" \"beta\"", 1);
    enable_vim_normal_mode(&mut session);

    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)));
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE)));
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('"'), KeyModifiers::NONE)));
    session.insert_paste_text("A");
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));

    session.set_cursor("\"A\" \"".len());
    assert!(session.handle_vim_key(&KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)));

    assert_eq!(session.input_manager.content(), "\"A\" \"A\"");
}

#[test]
fn appearance_updates_do_not_reset_session_local_vim_mode() {
    let mut session = session_with_input("hello", 5);
    session.handle_command(InlineCommand::SetVimModeEnabled(true));
    assert!(session.vim_state.enabled());

    let mut appearance = session.appearance.clone();
    appearance.vim_mode = false;
    session.handle_command(InlineCommand::SetAppearance { appearance });

    assert!(session.vim_state.enabled());
}

