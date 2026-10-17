#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::helpers::*;
use crate::tui::core_tui::session::clipboard_image::ClipboardTextError;
use std::cell::Cell;
use std::rc::Rc;

fn raw_paste_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)
}

fn place_text(session: &mut AppSession, text: &str) {
    let event = session.process_key_with_clipboard_text_reader(raw_paste_key(), || Ok(text.to_string()));
    assert!(event.is_none());
}

fn warning_text(session: &AppSession) -> String {
    session
        .core
        .lines
        .iter()
        .filter(|line| line.kind == InlineMessageKind::Warning)
        .flat_map(|line| line.segments.iter())
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn shift_ctrl_v_pastes_large_clipboard_text_raw_without_collapsing() {
    let mut session = app_session_with_input("", 0);
    let pasted = (0..11).map(|i| format!("raw-{i}")).collect::<Vec<_>>().join("\n");
    place_text(&mut session, &pasted);

    // Raw paste tracks no compact block and stays expanded: full content
    // renders with no summary marker.
    assert_eq!(session.core.input_manager.content(), pasted);
    assert!(session.core.input_manager.compact_paste_range().is_none());
    assert!(!session.core.input_compact_mode);
    let data = session.core.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(!rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(rendered.contains("raw-10"), "got: {rendered}");
}

#[test]
fn shift_ctrl_v_expands_previously_collapsed_composer() {
    let mut session = app_session_with_input("", 0);
    let first = (0..11).map(|i| format!("first-{i}")).collect::<Vec<_>>().join("\n");
    session.core.insert_paste_text(&first);
    assert!(session.core.input_compact_mode);

    place_text(&mut session, "extra");

    assert!(!session.core.input_compact_mode);
    assert_eq!(session.core.input_manager.content(), format!("{first}extra"));
    let data = session.core.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(!rendered.contains("[Pasted Content"), "got: {rendered}");
    assert!(rendered.contains("extra"), "got: {rendered}");
}

#[test]
fn shift_ctrl_v_without_text_warns_and_keeps_draft() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    let event = session.process_key_with_clipboard_text_reader(raw_paste_key(), || Err(ClipboardTextError::NoText));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(warning_text(&session), "No text found in clipboard.");
}

#[test]
fn ctrl_v_without_shift_still_routes_to_image_path() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    // Image input disabled so the image branch warns without consulting any
    // clipboard reader; the flag proves the text reader is never consulted.
    session.handle_command(app_types::InlineCommand::SetImageInputEnabled(false));
    let text_reader_called = Rc::new(Cell::new(false));
    let flag = Rc::clone(&text_reader_called);
    let event = session.process_key_with_clipboard_text_reader(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
        move || {
            flag.set(true);
            Ok("should not be pasted".to_string())
        },
    );

    assert!(event.is_none());
    assert!(!text_reader_called.get());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(warning_text(&session), "The selected model does not support image input.");
}
