#![allow(missing_docs)]
use super::helpers::*;
use crate::tui::core_tui::session::clipboard_image::ClipboardImageError;
use crate::tui::core_tui::session::input_manager::InputHistoryEntry;
use crate::tui::core_tui::types::ContentPart;

fn image_part() -> ContentPart {
    image_part_with_data("png-data")
}

fn image_part_with_data(data: &str) -> ContentPart {
    ContentPart::image(data, "image/png")
}

fn set_image_input_enabled(session: &mut AppSession, enabled: bool) {
    session.handle_command(app_types::InlineCommand::SetImageInputEnabled(enabled));
}

fn image_paste_key(modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(KeyCode::Char('v'), modifiers)
}

fn paste_image(session: &mut AppSession, attachment: &ContentPart) {
    let event = session
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || Ok(attachment.clone()));
    assert!(event.is_none());
}

fn submit(session: &mut AppSession) -> app_types::SubmittedInput {
    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Some(app_types::InlineEvent::Submit(submitted)) = event else {
        panic!("expected submit event, got {event:?}");
    };
    submitted
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
fn ctrl_v_attaches_clipboard_image_when_enabled() {
    let mut session = app_session_with_input("describe this", "describe this".len());
    let attachment = image_part();
    set_image_input_enabled(&mut session, true);

    let event = session
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || Ok(attachment.clone()));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "describe this[Image #1]");
    assert_eq!(session.core.input_manager.attachments(), &[attachment]);
    assert!(warning_text(&session).is_empty());
}

#[test]
fn pasted_image_renders_immediately_as_inline_text_placeholder() {
    let mut session = app_session_with_input("", 0);
    set_image_input_enabled(&mut session, true);

    let event =
        session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || Ok(image_part()));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "[Image #1]");
    let data = session.core.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image #1]"));
    assert!(!rendered.contains("attachment"));
}

#[test]
fn pasted_images_insert_placeholders_at_cursor_and_keep_typed_order() {
    let mut session = app_session_with_input("", 0);
    set_image_input_enabled(&mut session, true);

    let first_paste =
        session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || Ok(image_part()));
    assert!(first_paste.is_none());
    let event = session.process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
    assert!(event.is_none());
    session.core.set_cursor(0);
    let second_paste =
        session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || Ok(image_part()));

    assert!(second_paste.is_none());
    assert_eq!(session.core.input_manager.content(), "[Image #2][Image #1]o");
    let data = session.core.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image #2][Image #1]o"));
    assert!(!rendered.contains("attachments"));
}

#[test]
fn pasted_image_keeps_large_text_paste_collapsed() {
    let mut session = app_session_with_input("before", "before".len());
    set_image_input_enabled(&mut session, true);
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.core.insert_paste_text(&pasted_text);
    session.core.insert_char(' ');
    for ch in "after".chars() {
        session.core.insert_char(ch);
    }
    paste_image(&mut session, &image_part());

    assert_eq!(session.core.input_manager.content(), format!("before{pasted_text} after[Image #1]"));
    let data = session.core.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("before"));
    assert!(rendered.contains("[Pasted Content"));
    assert!(rendered.contains("after[Image #1]"));
    assert!(!rendered.contains("line-1\nline-2"));
}

#[test]
fn consecutive_pasted_images_insert_consecutive_placeholders() {
    let mut session = app_session_with_input("", 0);
    set_image_input_enabled(&mut session, true);

    for _ in 0..2 {
        let event = session
            .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || Ok(image_part()));
        assert!(event.is_none());
    }

    assert_eq!(session.core.input_manager.content(), "[Image #1][Image #2]");
}

#[test]
fn pasted_image_is_included_in_submit_payload() {
    let mut session = app_session_with_input("", 0);
    let attachment = image_part();
    set_image_input_enabled(&mut session, true);

    let paste_event = session
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || Ok(attachment.clone()));

    assert!(paste_event.is_none());
    session.core.input_manager.insert_text(" and here?");

    let submit_event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let Some(app_types::InlineEvent::Submit(submitted)) = submit_event else {
        panic!("expected submit event with pasted image, got {submit_event:?}");
    };
    assert_eq!(submitted.text, "[Image #1] and here?");
    assert_eq!(submitted.attachments, vec![attachment]);
    assert!(session.core.input_manager.content().is_empty());
    assert!(session.core.input_manager.attachments().is_empty());
}

#[test]
fn orphaned_first_pasted_image_placeholder_deleted_is_not_submitted() {
    let mut session = app_session_with_input("", 0);
    let first_attachment = image_part_with_data("first-image");
    let second_attachment = image_part_with_data("second-image");
    set_image_input_enabled(&mut session, true);

    paste_image(&mut session, &first_attachment);
    paste_image(&mut session, &second_attachment);
    session
        .core
        .input_manager
        .set_content("[Image #2] describe remaining".to_owned());

    let submitted = submit(&mut session);

    assert_eq!(submitted.text, "[Image #2] describe remaining");
    assert_eq!(submitted.attachments, vec![second_attachment]);
}

#[test]
fn orphaned_second_pasted_image_placeholder_deleted_keeps_first_attachment() {
    let mut session = app_session_with_input("", 0);
    let first_attachment = image_part_with_data("first-image");
    let second_attachment = image_part_with_data("second-image");
    set_image_input_enabled(&mut session, true);

    paste_image(&mut session, &first_attachment);
    paste_image(&mut session, &second_attachment);
    session.core.input_manager.set_content("[Image #1] describe first".to_owned());

    let submitted = submit(&mut session);

    assert_eq!(submitted.text, "[Image #1] describe first");
    assert_eq!(submitted.attachments, vec![first_attachment]);
}

#[test]
fn reordered_pasted_image_placeholders_keep_original_attachment_order() {
    let mut session = app_session_with_input("", 0);
    let first_attachment = image_part_with_data("first-image");
    let second_attachment = image_part_with_data("second-image");
    set_image_input_enabled(&mut session, true);

    paste_image(&mut session, &first_attachment);
    paste_image(&mut session, &second_attachment);
    session.core.input_manager.set_content("[Image #2] then [Image #1]".to_owned());

    let submitted = submit(&mut session);

    assert_eq!(submitted.text, "[Image #2] then [Image #1]");
    assert_eq!(submitted.attachments, vec![first_attachment, second_attachment]);
}

#[test]
fn orphaned_restored_single_attachment_uses_visible_compacted_placeholder_provenance() {
    let mut session = app_session_with_input("", 0);
    let attachment = image_part_with_data("second-image");

    session.handle_command(app_types::InlineCommand::RestoreInputDraft(app_types::SubmittedInput::new(
        "[Image #2] describe remaining",
        vec![attachment],
    )));
    session.core.input_manager.set_content("describe remaining".to_owned());

    let submitted = submit(&mut session);

    assert_eq!(submitted.text, "describe remaining");
    assert!(submitted.attachments.is_empty());
}

#[test]
fn orphaned_history_restored_single_attachment_uses_visible_compacted_placeholder_provenance() {
    let mut session = app_session_with_input("", 0);
    let attachment = image_part_with_data("second-image");
    let entry =
        InputHistoryEntry::from_content_and_attachments("[Image #2] describe remaining".to_owned(), vec![attachment]);

    session.core.input_manager.apply_history_entry(entry);
    session.core.input_manager.set_content("describe remaining".to_owned());

    let submitted = submit(&mut session);

    assert_eq!(submitted.text, "describe remaining");
    assert!(submitted.attachments.is_empty());
}

#[test]
fn orphaned_restored_non_contiguous_placeholders_keep_visible_attachment() {
    let mut session = app_session_with_input("", 0);
    let second_attachment = image_part_with_data("second-image");
    let third_attachment = image_part_with_data("third-image");

    session.handle_command(app_types::InlineCommand::RestoreInputDraft(app_types::SubmittedInput::new(
        "[Image #2][Image #3] describe remaining",
        vec![second_attachment.clone(), third_attachment],
    )));
    session
        .core
        .input_manager
        .set_content("[Image #2] describe remaining".to_owned());

    let submitted = submit(&mut session);

    assert_eq!(submitted.text, "[Image #2] describe remaining");
    assert_eq!(submitted.attachments, vec![second_attachment]);
}

#[test]
fn orphaned_later_paste_after_restored_placeholder_keeps_restored_attachment() {
    let mut session = app_session_with_input("", 0);
    let restored_attachment = image_part_with_data("restored-image");
    let pasted_attachment = image_part_with_data("pasted-image");
    set_image_input_enabled(&mut session, true);

    session.handle_command(app_types::InlineCommand::RestoreInputDraft(app_types::SubmittedInput::new(
        "[Image #2] restored",
        vec![restored_attachment.clone()],
    )));
    paste_image(&mut session, &pasted_attachment);

    assert_eq!(session.core.input_manager.content(), "[Image #2] restored[Image #3]");

    session.core.input_manager.set_content("[Image #2] restored".to_owned());

    let submitted = submit(&mut session);

    assert_eq!(submitted.text, "[Image #2] restored");
    assert_eq!(submitted.attachments, vec![restored_attachment]);
}

#[test]
fn alt_v_attaches_clipboard_image_when_enabled() {
    let mut session = app_session_with_input("describe this", "describe this".len());
    let attachment = image_part();
    set_image_input_enabled(&mut session, true);

    let event =
        session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::ALT), || Ok(attachment.clone()));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "describe this[Image #1]");
    assert_eq!(session.core.input_manager.attachments(), &[attachment]);
}

#[test]
fn unsupported_model_warning_does_not_read_or_attach() {
    let mut session = app_session_with_input("describe this", "describe this".len());
    let mut reader_called = false;
    set_image_input_enabled(&mut session, false);

    let event = session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || {
        reader_called = true;
        Ok(image_part())
    });

    assert!(event.is_none());
    assert!(!reader_called);
    assert_eq!(session.core.input_manager.content(), "describe this");
    assert!(session.core.input_manager.attachments().is_empty());
    assert_eq!(warning_text(&session), "The selected model does not support image input.");
}

#[test]
fn no_image_warning_leaves_text_and_attachments_unchanged() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    let existing_attachment = ContentPart::image("existing", "image/png");
    session.core.input_manager.set_attachments(vec![existing_attachment.clone()]);
    set_image_input_enabled(&mut session, true);

    let event = session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || {
        Err(ClipboardImageError::NoImage)
    });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(session.core.input_manager.attachments(), &[existing_attachment]);
    assert_eq!(warning_text(&session), "No image found in clipboard.");
}

#[test]
fn clipboard_unavailable_warning_leaves_text_unchanged() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    set_image_input_enabled(&mut session, true);

    let event = session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::ALT), || {
        Err(ClipboardImageError::ClipboardUnavailable)
    });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(warning_text(&session), "Clipboard image paste is unavailable in this terminal or desktop session.");
}

#[test]
fn wsl_fallback_failure_warning_leaves_text_unchanged() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    set_image_input_enabled(&mut session, true);

    let event = session.process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || {
        Err(ClipboardImageError::WslFallbackFailure)
    });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(warning_text(&session), "Could not read a clipboard image from Windows via PowerShell.");
}

#[test]
fn bracketed_text_paste_still_inserts_text() {
    let mut session = app_session_with_input("prefix", "prefix".len());
    set_image_input_enabled(&mut session, true);
    let (tx, _rx) = mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::Paste(" pasted text".to_string()), &tx, None);

    assert_eq!(session.core.input_manager.content(), "prefix pasted text");
    assert!(session.core.input_manager.attachments().is_empty());
}
