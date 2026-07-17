#![allow(missing_docs)]
use super::super::*;
use super::helpers::*;
use crate::tui::core_tui::session::input_manager::InputHistoryEntry;

#[test]
fn move_left_word_from_end_moves_to_word_start() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    session.move_left_word();
    assert_eq!(session.input_manager.cursor(), 6);

    session.move_left_word();
    assert_eq!(session.input_manager.cursor(), 0);
}

#[test]
fn move_left_word_skips_trailing_whitespace() {
    let text = "hello  world";
    let mut session = session_with_input(text, text.len());

    session.move_left_word();
    assert_eq!(session.input_manager.cursor(), 7);
}

#[test]
fn move_left_word_cjk_advances_one_segment_at_a_time() {
    let text = "你好世界";
    let mut session = session_with_input(text, text.len());

    session.move_left_word();
    assert_eq!(session.cursor(), 9);

    session.move_left_word();
    assert_eq!(session.cursor(), 6);

    session.move_left_word();
    assert_eq!(session.cursor(), 3);

    session.move_left_word();
    assert_eq!(session.cursor(), 0);
}

#[test]
fn move_left_word_mixed_ascii_and_cjk_uses_unicode_boundaries() {
    let text = "hello你好";
    let mut session = session_with_input(text, text.len());

    session.move_left_word();
    assert_eq!(session.cursor(), 8);

    session.move_left_word();
    assert_eq!(session.cursor(), 5);

    session.move_left_word();
    assert_eq!(session.cursor(), 0);
}

#[test]
fn shift_left_selects_input_range() {
    let mut session = session_with_input("hello world", "hello world".len());

    let result = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));

    assert!(result.is_none());
    assert_eq!(
        session.input_manager.selection_range(),
        Some(("hello worl".len(), "hello world".len()))
    );
}

#[test]
fn typing_replaces_selected_input_range() {
    let mut session = session_with_input("hello world", "hello world".len());
    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));

    let result = session.process_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello wor!");
    assert_eq!(session.cursor(), "hello wor!".len());
    assert_eq!(session.input_manager.selection_range(), None);
}

#[test]
fn backspace_deletes_selected_input_range() {
    let mut session = session_with_input("hello world", "hello world".len());
    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));

    let result = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello wor");
    assert_eq!(session.cursor(), "hello wor".len());
    assert_eq!(session.input_manager.selection_range(), None);
}

#[test]
fn alt_arrow_left_moves_cursor_by_word() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let event = KeyEvent::new(KeyCode::Left, KeyModifiers::ALT);
    session.process_key(event);

    assert_eq!(session.cursor(), 6);
}

#[test]
fn alt_b_moves_cursor_by_word() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let event = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT);
    session.process_key(event);

    assert_eq!(session.cursor(), 6);
}

#[test]
fn move_right_word_advances_to_word_boundaries() {
    let text = "hello  world";
    let mut session = session_with_input(text, 0);

    session.move_right_word();
    assert_eq!(session.cursor(), 5);

    session.move_right_word();
    assert_eq!(session.cursor(), 12);

    session.move_right_word();
    assert_eq!(session.cursor(), text.len());
}

#[test]
fn move_right_word_from_whitespace_moves_to_next_word_start() {
    let text = "hello  world";
    let mut session = session_with_input(text, 5);

    session.move_right_word();
    assert_eq!(session.cursor(), 12);
}

#[test]
fn move_right_word_cjk_advances_one_segment_at_a_time() {
    let text = "你好世界";
    let mut session = session_with_input(text, 0);

    session.move_right_word();
    assert_eq!(session.cursor(), 3);

    session.move_right_word();
    assert_eq!(session.cursor(), 6);

    session.move_right_word();
    assert_eq!(session.cursor(), 9);

    session.move_right_word();
    assert_eq!(session.cursor(), 12);
}

#[test]
fn move_word_navigation_preserves_separator_breaks_within_unicode_segments() {
    let mut session = session_with_input("can't 32.3 foo.bar", 5);

    session.move_left_word();
    assert_eq!(session.cursor(), 4);

    session.move_left_word();
    assert_eq!(session.cursor(), 3);

    session.input_manager.set_cursor(10);
    session.move_left_word();
    assert_eq!(session.cursor(), 9);

    session.input_manager.set_cursor(18);
    session.move_left_word();
    assert_eq!(session.cursor(), 15);
}

#[test]
fn super_arrow_right_moves_cursor_to_end() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let event = KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER);
    let result = session.process_key(event);

    assert_eq!(session.cursor(), text.len());
    // Ensure Command+Right does NOT launch editor
    assert!(!matches!(result, Some(InlineEvent::LaunchEditor { .. })));
}

#[test]
fn super_a_moves_cursor_to_start() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SUPER);
    session.process_key(event);

    assert_eq!(session.cursor(), 0);
}

#[test]
fn super_e_moves_cursor_to_end() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::SUPER);
    let result = session.process_key(event);

    // Should move to end and return None (no event)
    assert!(result.is_none());
    assert_eq!(session.cursor(), text.len());
}

#[test]
fn control_a_moves_cursor_to_start() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let result = session.process_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.cursor(), 0);
}

#[test]
fn control_m_submits_model_command() {
    let mut session = session_with_input("draft prompt", "draft prompt".len());

    let result = session.process_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL));

    assert!(matches!(result, Some(InlineEvent::Submit(value)) if value == "/model"));
    assert_eq!(session.input_manager.content(), "draft prompt");
}

#[test]
fn control_w_deletes_previous_word() {
    let mut session = session_with_input("hello world", "hello world".len());

    let result = session.process_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello ");
    assert_eq!(session.cursor(), "hello ".len());
}

#[test]
fn control_w_deletes_previous_cjk_segment() {
    let mut session = session_with_input("你好世界", "你好世界".len());

    let result = session.process_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "你好世");
    assert_eq!(session.cursor(), 9);
}

#[test]
fn control_u_deletes_to_start_of_line() {
    let mut session = session_with_input("hello world", 5);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), " world");
    assert_eq!(session.cursor(), 0);
}

#[test]
fn control_k_deletes_to_end_of_line() {
    let mut session = session_with_input("hello world", 5);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello");
    assert_eq!(session.cursor(), 5);
}

#[test]
fn control_alt_e_does_not_launch_editor() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL | KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(!matches!(result, Some(InlineEvent::LaunchEditor { .. })));
}

#[test]
fn control_super_e_does_not_launch_editor() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL | KeyModifiers::SUPER);
    let result = session.process_key(event);

    // Should not launch editor when both Control and Super (Cmd) are pressed
    assert!(!matches!(result, Some(InlineEvent::LaunchEditor { .. })));
}

// Readline keybinding tests

#[test]
fn ctrl_f_moves_forward_one_character() {
    let mut session = session_with_input("hello", 0);

    let event = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.cursor(), 1);
}

#[test]
fn ctrl_b_moves_backward_one_character() {
    let mut session = session_with_input("hello", 3);

    let event = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.cursor(), 2);
}

#[test]
fn ctrl_p_navigates_to_previous_history() {
    let mut session = session_with_input("", 0);

    // Add a history entry
    session
        .input_manager
        .add_to_history(InputHistoryEntry::from_content_and_attachments(
            "previous command".to_string(),
            Vec::new(),
        ));

    let event = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "previous command");
}

#[test]
fn ctrl_n_navigates_to_next_history() {
    let mut session = session_with_input("", 0);

    // Add history entries
    session
        .input_manager
        .add_to_history(InputHistoryEntry::from_content_and_attachments(
            "first command".to_string(),
            Vec::new(),
        ));
    session
        .input_manager
        .add_to_history(InputHistoryEntry::from_content_and_attachments(
            "second command".to_string(),
            Vec::new(),
        ));

    // Go to previous history (should be "second command")
    let event_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
    session.process_key(event_p);
    assert_eq!(session.input_manager.content(), "second command");

    // Go to next history (should be "first command")
    let event_n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
    session.process_key(event_n);
    assert_eq!(session.input_manager.content(), "first command");

    // Go to next history again (should be empty draft)
    let event_n2 = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
    let result = session.process_key(event_n2);
    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn ctrl_t_transposes_characters() {
    let mut session = session_with_input("abc", 1);

    let event = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "bac");
}

#[test]
fn alt_t_transposes_words() {
    let mut session = session_with_input("hello world", 6);

    let event = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "world hello");
}

#[test]
fn alt_u_uppercases_word() {
    let mut session = session_with_input("hello world", 0);

    let event = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "HELLO world");
}

#[test]
fn alt_l_lowercases_word() {
    let mut session = session_with_input("HELLO WORLD", 0);

    let event = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello WORLD");
}

#[test]
fn alt_c_capitalizes_word() {
    let mut session = session_with_input("hello world", 0);

    let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "Hello world");
}

#[test]
fn alt_d_deletes_word_forward() {
    let mut session = session_with_input("hello world", 0);

    let event = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), " world");
}

#[test]
fn alt_backslash_deletes_whitespace_around_cursor() {
    let mut session = session_with_input("hello   world", 5);

    let event = KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "helloworld");
}
