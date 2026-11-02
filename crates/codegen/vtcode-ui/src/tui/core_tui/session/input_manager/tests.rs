use super::*;

#[test]
fn new_input_manager_is_empty() {
    let manager = InputManager::new();
    assert_eq!(manager.content(), "");
    assert_eq!(manager.cursor(), 0);
}

#[test]
fn insert_text_updates_content_and_cursor() {
    let mut manager = InputManager::new();
    manager.insert_text("hello");
    assert_eq!(manager.content(), "hello");
    assert_eq!(manager.cursor(), 5);
}

#[test]
fn backspace_removes_character_before_cursor() {
    let mut manager = InputManager::new();
    manager.insert_text("hello");
    manager.backspace();
    assert_eq!(manager.content(), "hell");
    assert_eq!(manager.cursor(), 4);
}

#[test]
fn delete_removes_character_at_cursor() {
    let mut manager = InputManager::new();
    manager.insert_text("hello");
    manager.set_cursor(1);
    manager.delete();
    assert_eq!(manager.content(), "hllo");
}

#[test]
fn move_cursor_left_and_right() {
    let mut manager = InputManager::new();
    manager.insert_text("hello");
    manager.move_cursor_left();
    assert_eq!(manager.cursor(), 4);
    manager.move_cursor_right();
    assert_eq!(manager.cursor(), 5);
}

#[test]
fn clear_resets_state() {
    let mut manager = InputManager::new();
    manager.insert_text("hello");
    manager.clear();
    assert_eq!(manager.content(), "");
    assert_eq!(manager.cursor(), 0);
}

#[test]
fn history_navigation() {
    let mut manager = InputManager::new();
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("first".to_owned(), Vec::new()));
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("second".to_owned(), Vec::new()));

    assert_eq!(manager.go_to_previous_history().map(|entry| entry.content.clone()), Some("second".to_owned()));
    assert_eq!(manager.go_to_previous_history().map(|entry| entry.content.clone()), Some("first".to_owned()));
    assert_eq!(manager.go_to_previous_history().map(|entry| entry.content.clone()), None);
}

#[test]
fn history_navigation_saves_draft() {
    let mut manager = InputManager::new();
    manager.set_content("current".to_owned());
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("previous".to_owned(), Vec::new()));

    manager.go_to_previous_history();
    assert_eq!(manager.go_to_next_history().map(|entry| entry.content.clone()), Some("current".to_owned()));
}

#[test]
fn utf8_cursor_movement() {
    let mut manager = InputManager::new();
    manager.insert_text("你好");
    assert_eq!(manager.cursor(), 6); // 2 chars * 3 bytes

    manager.move_cursor_left();
    assert_eq!(manager.cursor(), 3);

    manager.move_cursor_right();
    assert_eq!(manager.cursor(), 6);
}

#[test]
fn history_navigation_restores_attachments() {
    let mut manager = InputManager::new();
    manager.set_content("check this".to_owned());
    manager.set_attachments(vec![ContentPart::image("encoded".to_owned(), "image/png".to_owned())]);
    manager.add_to_history(manager.current_history_entry());
    manager.clear();

    let entry = manager.go_to_previous_history().expect("history entry");
    manager.apply_history_entry(entry);

    assert_eq!(manager.content(), "check this");
    assert_eq!(manager.attachments().len(), 1);
}

#[test]
fn insert_text_replaces_selection() {
    let mut manager = InputManager::new();
    manager.insert_text("hello world");
    manager.set_cursor(5);
    manager.set_cursor_with_selection(11);

    manager.insert_text(" there");

    assert_eq!(manager.content(), "hello there");
    assert_eq!(manager.cursor(), "hello there".len());
    assert!(!manager.has_selection());
}

#[test]
fn backspace_deletes_selected_range() {
    let mut manager = InputManager::new();
    manager.insert_text("hello world");
    manager.set_cursor(0);
    manager.set_cursor_with_selection(5);

    manager.backspace();

    assert_eq!(manager.content(), " world");
    assert_eq!(manager.cursor(), 0);
    assert!(!manager.has_selection());
}

#[test]
fn move_cursor_left_collapses_selection_to_start() {
    let mut manager = InputManager::new();
    manager.insert_text("hello world");
    manager.set_cursor(0);
    manager.set_cursor_with_selection(5);

    manager.move_cursor_left();

    assert_eq!(manager.cursor(), 0);
    assert!(!manager.has_selection());
}

#[test]
fn multiline_cursor_up_down_stops_at_edges() {
    let mut manager = InputManager::new();
    manager.set_content("first\nsecond\nthird".to_owned());
    assert_eq!(manager.line_count(), 3);
    // set_content moves to end (last line).
    assert_eq!(manager.cursor_row(), 2);

    assert!(manager.move_cursor_up());
    assert_eq!(manager.cursor_row(), 1);
    assert!(manager.move_cursor_up());
    assert_eq!(manager.cursor_row(), 0);
    assert!(!manager.move_cursor_up());

    assert!(manager.move_cursor_down());
    assert_eq!(manager.cursor_row(), 1);
    assert!(manager.move_cursor_down());
    assert_eq!(manager.cursor_row(), 2);
    assert!(!manager.move_cursor_down());
}

#[test]
fn single_line_cursor_up_down_does_not_move() {
    let mut manager = InputManager::new();
    manager.set_content("solo".to_owned());
    assert_eq!(manager.line_count(), 1);
    assert!(!manager.move_cursor_up());
    assert!(!manager.move_cursor_down());
}

#[test]
fn undo_redo() {
    let mut manager = InputManager::new();
    manager.insert_text("hello");
    assert_eq!(manager.content(), "hello");

    manager.undo();
    assert_eq!(manager.content(), "");
    assert_eq!(manager.cursor(), 0);

    manager.redo();
    assert_eq!(manager.content(), "hello");
}

#[test]
fn prepend_archived_history_inserts_at_front() {
    let mut manager = InputManager::new();
    let now = Utc::now();
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("session-prompt".to_owned(), Vec::new()));

    // Input is newest-first; archived-1 is newer than archived-2.
    manager.prepend_archived_history(vec![
        InputHistoryEntry::from_content_and_timestamp("archived-1".to_owned(), now),
        InputHistoryEntry::from_content_and_timestamp("archived-2".to_owned(), now),
    ]);

    // After prepend: oldest archived at front, newest archived, then session.
    let texts = manager.history_texts();
    assert_eq!(texts, vec!["archived-2", "archived-1", "session-prompt"]);
}

#[test]
fn prepend_archived_history_deduplicates() {
    let mut manager = InputManager::new();
    let now = Utc::now();
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("duplicate".to_owned(), Vec::new()));

    manager.prepend_archived_history(vec![
        InputHistoryEntry::from_content_and_timestamp("unique".to_owned(), now),
        InputHistoryEntry::from_content_and_timestamp("duplicate".to_owned(), now),
    ]);

    let texts = manager.history_texts();
    // "duplicate" was already in session history, so the archived copy is skipped.
    assert_eq!(texts, vec!["unique", "duplicate"]);
    assert_eq!(texts.iter().filter(|t| *t == "duplicate").count(), 1);
}

#[test]
fn prepend_archived_history_skips_empty() {
    let mut manager = InputManager::new();
    let now = Utc::now();

    manager.prepend_archived_history(vec![
        InputHistoryEntry::from_content_and_timestamp("".to_owned(), now),
        InputHistoryEntry::from_content_and_timestamp("real".to_owned(), now),
    ]);

    assert_eq!(manager.history_texts(), vec!["real"]);
}

#[test]
fn up_arrow_cycles_session_first_then_archived() {
    let mut manager = InputManager::new();
    let now = Utc::now();
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("session-2".to_owned(), Vec::new()));
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("session-1".to_owned(), Vec::new()));

    // archived-newer is the most recent archived entry.
    manager.prepend_archived_history(vec![
        InputHistoryEntry::from_content_and_timestamp("archived-newer".to_owned(), now),
        InputHistoryEntry::from_content_and_timestamp("archived-older".to_owned(), now),
    ]);

    // History: [archived-newer, archived-older, session-2, session-1]
    // Up arrow from fresh state: most recent session entry first, then
    // older session entries, then archived (newest archived first).
    assert_eq!(manager.go_to_previous_history().map(|e| e.content.clone()), Some("session-1".to_owned()));
    assert_eq!(manager.go_to_previous_history().map(|e| e.content.clone()), Some("session-2".to_owned()));
    assert_eq!(manager.go_to_previous_history().map(|e| e.content.clone()), Some("archived-newer".to_owned()));
    assert_eq!(manager.go_to_previous_history().map(|e| e.content.clone()), Some("archived-older".to_owned()));
    assert!(manager.go_to_previous_history().is_none());
}

#[test]
fn down_restores_draft_from_archived_entries() {
    let mut manager = InputManager::new();
    let now = Utc::now();
    manager.set_content("my original prompt".to_owned());
    manager.add_to_history(InputHistoryEntry::from_content_and_attachments("session-1".to_owned(), Vec::new()));

    manager.prepend_archived_history(vec![
        InputHistoryEntry::from_content_and_timestamp("archived-1".to_owned(), now),
        InputHistoryEntry::from_content_and_timestamp("archived-2".to_owned(), now),
    ]);

    // Navigate backward into archived entries.
    manager.go_to_previous_history(); // session-1
    let entry = manager.go_to_previous_history().map(|e| e.content.clone());
    // archived-1 is the newest archived (adjacent to session entries).
    assert_eq!(entry, Some("archived-1".to_owned()));

    // Press Down from an archived entry — should restore the draft
    // instead of cycling to the next archived item.
    let restored = manager.go_to_next_history().map(|e| e.content.clone());
    assert_eq!(restored, Some("my original prompt".to_owned()));
    // History navigation should be fully exited.
    assert!(manager.history_index().is_none());
}

#[test]
fn single_line_range_covers_entire_buffer() {
    let mut manager = InputManager::new();
    manager.set_content("hello".to_owned());
    assert!(manager.is_single_line());
    assert_eq!(manager.current_line_byte_range(), (0, 5));
}

#[test]
fn multiline_current_line_range_follows_cursor_row() {
    let mut manager = InputManager::new();
    manager.set_content("first\nsecond\nthird".to_owned());
    assert!(!manager.is_single_line());
    // set_content lands at end (third line).
    assert_eq!(manager.current_line_byte_range(), (13, 18));

    manager.set_cursor(0);
    assert_eq!(manager.current_line_byte_range(), (0, 5));

    manager.set_cursor(8);
    assert_eq!(manager.current_line_byte_range(), (6, 12));
}

#[test]
fn move_to_start_and_end_of_line_single_line_matches_buffer_edges() {
    let mut manager = InputManager::new();
    manager.set_content("hello".to_owned());
    manager.set_cursor(2);

    manager.move_cursor_to_start_of_line();
    assert_eq!(manager.cursor(), 0);

    manager.move_cursor_to_end_of_line();
    assert_eq!(manager.cursor(), 5);
}

#[test]
fn move_to_start_and_end_of_line_multiline_stays_within_line() {
    let mut manager = InputManager::new();
    manager.set_content("first\nsecond\nthird".to_owned());

    manager.set_cursor(8);
    manager.move_cursor_to_start_of_line();
    assert_eq!(manager.cursor(), 6);

    manager.move_cursor_to_end_of_line();
    assert_eq!(manager.cursor(), 12);
}
