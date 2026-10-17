#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;
use crate::tui::core_tui::session::input_manager::InputHistoryEntry;

#[test]
fn disabled_input_ignores_control_j_but_preserves_interrupt() {
    let mut session = session_with_input("draft", 5);
    session.handle_command(InlineCommand::SetInputEnabled(false));
    let (sender, mut receiver) = mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)), &sender, None);

    assert_eq!(session.input_manager.content(), "draft");
    assert_eq!(session.cursor(), 5);
    assert!(receiver.try_recv().is_err());

    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), &sender, None);
    assert!(matches!(receiver.try_recv(), Ok(InlineEvent::Interrupt)));
}

#[test]
fn enabled_input_control_j_still_inserts_newline() {
    let mut session = session_with_input("draft", 5);

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert_eq!(session.input_manager.content(), "draft\n");
}

#[test]
fn overlay_owns_paste_after_input_is_reenabled() {
    for searchable in [false, true] {
        let mut session = session_with_input("draft", 5);
        session.handle_command(InlineCommand::ShowOverlay {
            request: Box::new(OverlayRequest::List(ListOverlayRequest {
                title: "Choose".to_string(),
                lines: Vec::new(),
                footer_hint: None,
                items: ["alpha", "beta"]
                    .into_iter()
                    .map(|title| InlineListItem {
                        title: title.to_string(),
                        subtitle: None,
                        badge: None,
                        indent: 0,
                        selection: Some(InlineListSelection::SlashCommand(title.to_string())),
                        search_value: Some(title.to_string()),
                        ..Default::default()
                    })
                    .collect(),
                selected: None,
                search: searchable.then(|| InlineListSearchConfig {
                    label: "Filter".to_string(),
                    placeholder: None,
                    fuzzy: false,
                }),
                hotkeys: Vec::new(),
                status: None,
            })),
        });
        session.handle_command(InlineCommand::SetInputEnabled(true));
        let (sender, mut receiver) = mpsc::unbounded_channel();

        session.handle_event(CrosstermEvent::Paste("beta".to_string()), &sender, None);

        assert_eq!(session.input_manager.content(), "draft");
        assert!(receiver.try_recv().is_err());
        let modal = session.modal_state().expect("overlay remains active");
        if searchable {
            assert_eq!(modal.search.as_ref().expect("search").query, "beta");
            assert_eq!(modal.list.as_ref().expect("list").visible_indices, vec![1]);
        }
    }
}

#[test]
fn modal_ctrl_c_is_not_swallowed_by_stale_composer_selection() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("selected draft".to_string());
    session.input_manager.set_cursor(0);
    session.input_manager.set_cursor_with_selection("selected draft".len());
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: "Choose".to_string(),
            lines: Vec::new(),
            footer_hint: None,
            items: vec![InlineListItem {
                title: "alpha".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::SlashCommand("alpha".to_string())),
                search_value: Some("alpha".to_string()),
                ..Default::default()
            }],
            selected: None,
            search: None,
            hotkeys: Vec::new(),
            status: None,
        })),
    });

    let event = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert!(!session.has_active_overlay());
    assert!(matches!(event, Some(InlineEvent::Interrupt)));
    assert_eq!(session.input_manager.content(), "selected draft");
}

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
    assert_eq!(session.input_manager.selection_range(), Some(("hello worl".len(), "hello world".len())));
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
fn super_a_clears_current_line() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SUPER);
    session.process_key(event);

    // Single-line input: Cmd+A clears the whole input (matches Cmd+Backspace).
    assert_eq!(session.input_manager.content(), "");
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
fn control_u_clears_current_line() {
    let mut session = session_with_input("hello world", 5);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    // Single-line input: clearing the line clears the whole input.
    assert_eq!(session.input_manager.content(), "");
    assert_eq!(session.cursor(), 0);
}

#[test]
fn control_u_clears_only_current_line_in_multiline() {
    let mut session = session_with_input("first\nsecond\nthird", 8);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "first\n\nthird");
    assert_eq!(session.cursor(), 6);
}

#[test]
fn control_a_moves_to_line_start_and_control_e_to_line_end() {
    let mut session = session_with_input("first\nsecond\nthird", 8);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert!(result.is_none());
    assert_eq!(session.cursor(), 6);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    assert!(result.is_none());
    assert_eq!(session.cursor(), 12);
}

#[test]
fn legacy_control_codes_match_cmd_line_editing() {
    // Terminals without the Kitty keyboard protocol send Cmd+Left/Right/
    // Backspace/Delete as bare C0 control bytes. `normalize_terminal_control_
    // event` folds them into Ctrl+<letter>; each must reach the same line-wise
    // handler as the SUPER path.
    //
    // Cmd+Left (0x01) → line start.
    let mut session = session_with_input("first\nsecond\nthird", 8);
    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{1}'), KeyModifiers::NONE));
    assert!(result.is_none());
    assert_eq!(session.cursor(), 6);

    // Cmd+Right (0x05) → line end.
    let mut session = session_with_input("first\nsecond\nthird", 8);
    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{5}'), KeyModifiers::NONE));
    assert!(result.is_none());
    assert_eq!(session.cursor(), 12);

    // Cmd+Delete (0x0B) → delete to line end.
    let mut session = session_with_input("first\nsecond\nthird", 8);
    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{b}'), KeyModifiers::NONE));
    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "first\nse\nthird");

    // Cmd+Backspace (0x15) → clear current line.
    let mut session = session_with_input("first\nsecond\nthird", 8);
    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{15}'), KeyModifiers::NONE));
    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "first\n\nthird");
}

#[test]
fn legacy_control_fifteen_clears_single_line_entirely() {
    let mut session = session_with_input("hello world", 5);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{15}'), KeyModifiers::NONE));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "");
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
fn ctrl_b_dispatches_background_operation() {
    let mut session = session_with_input("hello", 3);

    let event = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(result, Some(InlineEvent::BackgroundOperation)));
    assert_eq!(session.input_manager.cursor(), 3);
}

#[test]
fn ctrl_p_navigates_to_previous_history() {
    let mut session = session_with_input("", 0);

    // Add a history entry
    session
        .input_manager
        .add_to_history(InputHistoryEntry::from_content_and_attachments("previous command".to_string(), Vec::new()));

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
        .add_to_history(InputHistoryEntry::from_content_and_attachments("first command".to_string(), Vec::new()));
    session
        .input_manager
        .add_to_history(InputHistoryEntry::from_content_and_attachments("second command".to_string(), Vec::new()));

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
fn ctrl_t_transposes_cyrillic_characters_without_panicking() {
    // Multi-byte UTF-8 must not hit a "byte index is not a char boundary" panic.
    let mut session = session_with_input("яб", 0);

    let event = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "бя");
}

#[test]
fn ctrl_t_transposes_cyrillic_in_middle_without_panicking() {
    let mut session = session_with_input("абв", 2);

    let event = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "бав");
}

#[test]
fn alt_t_toggles_tool_display_mode() {
    let mut session = session_with_input("hello world", 6);

    let event = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(matches!(result, Some(InlineEvent::ToggleToolDisplayMode)));
    assert_eq!(session.input_manager.content(), "hello world");
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
fn alt_u_uppercases_cyrillic_word_without_panicking() {
    // Byte-vs-char index confusion would panic on multi-byte UTF-8.
    let mut session = session_with_input("привет мир", 0);

    let event = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT);
    let result = session.process_key(event);

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "ПРИВЕТ мир");
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
