use super::*;

#[test]
fn busy_plain_enter_with_attachments_queues_single_turn() {
    // Queued attachments must stay with their message as one turn; the old
    // Steer path rejected attachments with a warning + draft restore.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("see image".to_string());
    session
        .input_manager
        .set_attachments(vec![ContentPart::image("encoded", "image/png")]);

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "see image");
            assert!(value.has_attachments(), "attachment must stay queued with its message");
            assert!(!value.batchable, "attachment queue entries must dispatch alone");
        }
        other => panic!("expected QueueSubmit with attachment, got: {other:?}"),
    }
    assert_eq!(session.queued_inputs, vec!["see image".to_string()]);
}
#[test]
fn app_busy_slash_command_copy_submits_immediately() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/copy".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&event, Some(AppInlineEvent::Submit(value)) if value.text == "/copy"),
        "expected immediate Submit, got: {event:?}"
    );
}
#[test]
fn app_busy_control_enter_queues_slash_command_as_non_batchable() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/model gpt-4o".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(
        matches!(&event, Some(AppInlineEvent::QueueSubmit(value)) if value.text == "/model gpt-4o" && !value.batchable),
        "expected non-batchable QueueSubmit for a slash command, got: {event:?}"
    );
}
#[test]
fn restore_input_draft_command_preserves_attachments_in_order() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let first_attachment = ContentPart::image("first-image", "image/png");
    let second_attachment = ContentPart::image("second-image", "image/png");

    session.handle_command(InlineCommand::RestoreInputDraft(SubmittedInput::new(
        "keep searching in docs/",
        vec![first_attachment.clone(), second_attachment.clone()],
    )));

    assert_eq!(session.input_manager.content(), "keep searching in docs/");
    assert_eq!(session.input_manager.attachments(), &[first_attachment, second_attachment]);
}
#[test]
fn app_restore_input_draft_command_preserves_attachments_in_order() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let first_attachment = ContentPart::image("first-image", "image/png");
    let second_attachment = ContentPart::image("second-image", "image/png");

    session.handle_command(app_types::InlineCommand::RestoreInputDraft(SubmittedInput::new(
        "keep searching in docs/",
        vec![first_attachment.clone(), second_attachment.clone()],
    )));

    assert_eq!(session.core.input_manager.content(), "keep searching in docs/");
    assert_eq!(session.core.input_manager.attachments(), &[first_attachment, second_attachment]);
}
#[test]
fn busy_tab_does_not_cycle_primary_agent() {
    // Busy Tab queues like Ctrl+Enter instead of cycling.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("queue this next".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::QueueSubmit(value)) if value == "queue this next"));
    assert_eq!(session.input_manager.content(), "");
}
#[test]
fn busy_tab_shows_mode_switch_notice() {
    // Mode-switch notice now lives on Shift+Tab (BackTab); plain Tab queues.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);

    let _ = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    let transcript = visible_transcript(&mut session);
    assert!(
        transcript.iter().any(|line| line.contains("Mode switching is disabled")),
        "expected a mode-switch busy notice, got: {transcript:?}"
    );
}
#[test]
fn command_backspace_clears_single_line_entire_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello world".to_string());
    session.input_manager.set_cursor(5);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
}
#[test]
fn command_a_clears_single_line_entire_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
}
#[test]
fn command_backspace_clears_only_current_line_in_multiline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    // Cursor inside "second" (offset 8).
    session.input_manager.set_cursor(8);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "first\n\nthird");
    assert_eq!(session.input_manager.cursor(), 6);
}
#[test]
fn command_a_clears_only_current_line_in_multiline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    session.input_manager.set_cursor(1);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "\nsecond\nthird");
}
#[test]
fn command_left_and_right_move_within_current_line() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    session.input_manager.set_cursor(8);

    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 6);

    let _ = session.process_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 12);
}
#[test]
fn command_left_and_right_single_line_match_buffer_edges() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());
    session.input_manager.set_cursor(2);

    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 0);

    let _ = session.process_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 5);
}
#[test]
fn command_backspace_clears_compact_paste_block_atomically() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 2;
    let pasted = (0..line_total).map(|i| format!("paste-{i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);
    assert!(session.input_manager.compact_paste_range().is_some());
    // Cursor in the middle of the collapsed block.
    let mid = session.input_manager.content().len() / 2;
    session.input_manager.set_cursor(mid);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
    assert!(session.input_manager.compact_paste_range().is_none());
}
#[test]
fn command_backspace_clears_single_image_block() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("[Image #1]".to_string());
    session
        .input_manager
        .set_attachments(vec![ContentPart::image("encoded", "image/png")]);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
    assert!(session.input_manager.attachments().is_empty());
}
#[test]
fn double_escape_clears_single_line_entire_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    assert_eq!(session.input_manager.content(), "hello");

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    assert_eq!(session.input_manager.content(), "");
}
#[test]
fn double_escape_clears_only_current_line_in_multiline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    session.input_manager.set_cursor(8);

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    assert_eq!(session.input_manager.content(), "first\nsecond\nthird");

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    assert_eq!(session.input_manager.content(), "first\n\nthird");
}
#[test]
fn single_escape_followed_by_other_key_does_not_clear() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    // Second Esc is a fresh first press, so content remains.
    assert_eq!(session.input_manager.content(), "hello");
}
#[test]
fn tab_enqueues_like_control_enter_when_idle() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("queue me".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "queue me"));
    assert_eq!(session.input_manager.content(), "");
}
#[test]
fn tab_enqueues_like_control_enter_when_busy() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("busy draft".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::QueueSubmit(value)) if value == "busy draft"));
}
#[test]
fn shift_tab_cycles_forward_and_backtab_cycles_previous() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let forward = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
    assert!(matches!(forward, Some(InlineEvent::CyclePrimaryAgent)));

    let previous = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert!(matches!(previous, Some(InlineEvent::CyclePrimaryAgentPrevious)));
}
#[test]
fn app_tab_enqueues_like_control_enter_when_idle() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("app draft".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(
        matches!(&event, Some(AppInlineEvent::Submit(value)) if value.text == "app draft"),
        "expected Submit, got: {event:?}"
    );
    assert_eq!(session.core.input_manager.content(), "");
}
#[test]
fn app_double_escape_clears_current_line_in_multiline() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("first\nsecond\nthird".to_string());
    session.core.input_manager.set_cursor(8);

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    assert_eq!(session.core.input_manager.content(), "first\nsecond\nthird");

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    assert_eq!(session.core.input_manager.content(), "first\n\nthird");
}
#[test]
fn app_legacy_control_codes_match_cmd_line_editing() {
    // Legacy terminals encode Cmd+Left/Right/Backspace as 0x01/0x05/0x15.
    // They must reach the same line-wise handlers as the SUPER path in the app
    // session (the interactive TUI entry point).
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("first\nsecond\nthird".to_string());
    session.core.input_manager.set_cursor(8);

    let left = session.process_key(KeyEvent::new(KeyCode::Char('\u{1}'), KeyModifiers::NONE));
    assert!(left.is_none());
    assert_eq!(session.core.input_manager.cursor(), 6);

    let right = session.process_key(KeyEvent::new(KeyCode::Char('\u{5}'), KeyModifiers::NONE));
    assert!(right.is_none());
    assert_eq!(session.core.input_manager.cursor(), 12);

    let clear = session.process_key(KeyEvent::new(KeyCode::Char('\u{15}'), KeyModifiers::NONE));
    assert!(clear.is_none());
    assert_eq!(session.core.input_manager.content(), "first\n\nthird");
}
#[test]
fn modified_enter_preserves_command_drafts_and_literal_backslashes() {
    for running in [false, true] {
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::ALT] {
            for draft in ["/stop", "/plan", "/help", "/stop\\", "/plan\\", "/help\\", "literal\\"] {
                let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
                if running {
                    session.handle_command(InlineCommand::SetActivityState(ActivityState::Building));
                }
                session.input_manager.set_content(draft.to_string());
                let event = session.process_key(KeyEvent::new(KeyCode::Enter, modifiers));
                assert!(event.is_none(), "{draft} {modifiers:?} running={running}");
                assert_eq!(session.input_manager.content(), format!("{draft}\n"));
                assert!(session.queued_inputs.is_empty());
            }
        }
    }
}
#[test]
fn composer_enter_and_control_j_keep_existing_newline_and_submit_behavior() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("literal\\".to_string());
    assert!(session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).is_none());
    assert_eq!(session.input_manager.content(), "literal\n");
    session.input_manager.set_content("/help".to_string());
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert_eq!(session.input_manager.content(), "/help\n");
    for modifiers in [
        KeyModifiers::NONE,
        KeyModifiers::CONTROL,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ] {
        session.input_manager.set_content("send this".to_string());
        assert!(
            matches!(session.process_key(KeyEvent::new(KeyCode::Enter, modifiers)), Some(InlineEvent::Submit(input)) if input.text == "send this")
        );
    }
}
