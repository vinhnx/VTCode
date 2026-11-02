use super::*;

#[test]
fn shift_enter_inserts_newline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.input_manager.set_content("queued".to_string());

    let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "queued\n");
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());
}
#[test]
fn paste_preserves_all_newlines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = (0..15).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let (tx, _rx) = mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::Paste(pasted.clone()), &tx, None);

    assert_eq!(session.input_manager.content(), pasted);
}
#[test]
fn pasted_message_displays_full_content() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("paste-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.append_pasted_message(InlineMessageKind::User, pasted_text.clone(), pasted_lines.len());

    let user_line = session
        .lines
        .iter()
        .find(|line| line.kind == InlineMessageKind::User)
        .expect("user line should exist");
    let combined: String = user_line.segments.iter().map(|segment| segment.text.as_str()).collect();
    assert!(combined.contains("paste-1"));
    assert!(session.collapsed_pastes.is_empty());
}
#[test]
fn pasted_message_collapses_large_json_for_tool() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let mut json = String::from("{\n");
    let line_total = ui::INLINE_JSON_COLLAPSE_LINE_THRESHOLD + 5;
    for idx in 0..line_total {
        json.push_str(&format!("  \"key{idx}\": \"value{idx}\",\n"));
    }
    json.push_str("  \"end\": true\n}");
    let line_count = json.lines().count();

    session.append_pasted_message(InlineMessageKind::Tool, json.clone(), line_count);

    assert_eq!(session.collapsed_pastes.len(), 1);
    let collapsed_index = session.collapsed_pastes[0].line_index;
    let preview_line = session.lines.get(collapsed_index).expect("collapsed line exists");
    let preview_text: String = preview_line.segments.iter().map(|segment| segment.text.as_str()).collect();
    assert!(preview_text.contains("showing last"));
    assert!(preview_text.contains("\"end\": true"));

    assert!(session.expand_collapsed_paste_at_line_index(collapsed_index));
    assert!(session.collapsed_pastes.is_empty());

    let expanded_line = session.lines.get(collapsed_index).expect("expanded line exists");
    let expanded_text: String = expanded_line.segments.iter().map(|segment| segment.text.as_str()).collect();
    assert!(expanded_text.contains("\"key0\": \"value0\""));
    assert!(expanded_text.contains("\"end\": true"));
}
#[test]
fn input_compact_preview_for_large_paste() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.insert_paste_text(&pasted_text);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"));
}
#[test]
fn input_compact_preview_keeps_text_after_large_paste_visible() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("before".to_string());
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.insert_paste_text(&pasted_text);
    session.insert_char(' ');
    for ch in "after".chars() {
        session.insert_char(ch);
    }

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("before"));
    assert!(rendered.contains("[Pasted Content"));
    assert!(rendered.contains("after"));
    assert_eq!(session.input_manager.content(), format!("before{pasted_text} after"));
}
#[test]
fn shift_enter_after_large_paste_inserts_newline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello ".to_string());
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.insert_paste_text(&pasted_text);
    session.insert_char(' ');
    for ch in "and what are you talking about??".chars() {
        session.insert_char(ch);
    }

    let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), format!("hello {pasted_text} and what are you talking about??\n"));
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("talking about??\n"));
    assert_eq!(data.cursor_y, 1);
    assert!(session.desired_input_lines(VIEW_WIDTH) >= 2);
}
