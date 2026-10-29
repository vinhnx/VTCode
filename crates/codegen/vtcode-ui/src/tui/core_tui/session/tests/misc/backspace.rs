use super::*;

#[test]
fn backspace_after_large_paste_deletes_whole_block() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    assert!(session.input_manager.compact_paste_range().is_some());
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());

    session.delete_char();

    assert_eq!(session.input_manager.content(), "x");
    assert!(session.input_manager.compact_paste_range().is_none());
}
#[test]
fn backspace_after_paste_still_deletes_typed_chars_one_by_one() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    session.insert_char('!');
    session.insert_char('?');
    assert!(session.input_manager.content().ends_with("!?"));

    session.delete_char();
    assert!(session.input_manager.content().ends_with("!"));
    assert!(session.input_manager.compact_paste_range().is_some());

    session.delete_char();
    assert!(session.input_manager.content().ends_with(&pasted));
}
#[test]
fn backspace_with_active_selection_deletes_selection_not_whole_paste() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    let content_len = session.input_manager.content().len();
    // Select the last two characters (e.g. "10" of "line 10") so the active
    // selection ends exactly at the collapsed paste end.
    session.input_manager.set_cursor_with_selection(content_len - 2);
    assert!(session.input_manager.selection_range().is_some());

    session.delete_char();

    // The selection wins over the collapse: only the two selected chars are
    // removed, not the entire pasted block.
    assert_eq!(session.input_manager.content().len(), content_len - 2);
    assert!(!session.input_manager.content().ends_with("10"));
    assert!(session.input_manager.content().contains("line 0"));
}
#[test]
fn backspace_after_paste_deletes_whole_block_when_no_selection() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    assert!(session.input_manager.compact_paste_range().is_some());
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());
    assert!(session.input_manager.selection_range().is_none());

    session.delete_char();

    assert_eq!(session.input_manager.content(), "x");
    assert!(session.input_manager.compact_paste_range().is_none());
}
#[test]
fn delete_word_forward_before_paste_keeps_block_delete_aligned() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    session.insert_char(' ');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    let range = session.input_manager.compact_paste_range().expect("collapsed paste range");
    assert_eq!(range.start, 2);

    // Delete the typed word sitting before the collapsed block.
    session.input_manager.set_cursor(0);
    session.input_manager.delete_word_forward();

    // The range must shift by the removed bytes, not go stale.
    let shifted = session
        .input_manager
        .compact_paste_range()
        .expect("range survives edits entirely before it");
    assert_eq!(shifted.start, 1);
    assert_eq!(shifted.end, range.end - 1);

    // The block-delete invariant holds: cursor at the new end coincides with
    // the shifted range end, so Backspace removes the whole block.
    session.input_manager.set_cursor(session.input_manager.content().len());
    session.delete_char();

    assert_eq!(session.input_manager.content(), " ");
    assert!(session.input_manager.compact_paste_range().is_none());
}
#[test]
fn delete_word_forward_overlapping_paste_clears_compact_range() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    session.insert_char(' ');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    let range = session.input_manager.compact_paste_range().expect("collapsed paste range");

    // Cursor inside the collapsed block: an overlapping delete invalidates it
    // so a later Backspace never removes user-typed bytes.
    session.input_manager.set_cursor(range.start + 1);
    session.input_manager.delete_word_forward();

    assert!(session.input_manager.compact_paste_range().is_none());
}
#[test]
fn alt_backspace_deletes_previous_word() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("hello world".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    let result = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello ");
}
#[test]
fn alt_backspace_deletes_previous_cyrillic_word() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("привет мир".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    let result = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "привет ");
}
