use super::helpers::*;
use crate::tui::core_tui::session::clipboard_image::ClipboardImageError;
use crate::tui::core_tui::types::ContentPart;

fn image_part() -> ContentPart {
    ContentPart::image("png-data", "image/png")
}

fn set_image_input_enabled(session: &mut AppSession, enabled: bool) {
    session.handle_command(app_types::InlineCommand::SetImageInputEnabled(enabled));
}

fn image_paste_key(modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(KeyCode::Char('v'), modifiers)
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
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || {
            Ok(attachment.clone())
        });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "describe this");
    assert_eq!(session.core.input_manager.attachments(), &[attachment]);
    assert!(warning_text(&session).is_empty());
}

#[test]
fn alt_v_attaches_clipboard_image_when_enabled() {
    let mut session = app_session_with_input("describe this", "describe this".len());
    let attachment = image_part();
    set_image_input_enabled(&mut session, true);

    let event = session
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::ALT), || {
            Ok(attachment.clone())
        });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.attachments(), &[attachment]);
}

#[test]
fn unsupported_model_warning_does_not_read_or_attach() {
    let mut session = app_session_with_input("describe this", "describe this".len());
    let mut reader_called = false;
    set_image_input_enabled(&mut session, false);

    let event = session.process_key_with_clipboard_image_reader(
        image_paste_key(KeyModifiers::CONTROL),
        || {
            reader_called = true;
            Ok(image_part())
        },
    );

    assert!(event.is_none());
    assert!(!reader_called);
    assert_eq!(session.core.input_manager.content(), "describe this");
    assert!(session.core.input_manager.attachments().is_empty());
    assert_eq!(
        warning_text(&session),
        "The selected model does not support image input."
    );
}

#[test]
fn no_image_warning_leaves_text_and_attachments_unchanged() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    let existing_attachment = ContentPart::image("existing", "image/png");
    session
        .core
        .input_manager
        .set_attachments(vec![existing_attachment.clone()]);
    set_image_input_enabled(&mut session, true);

    let event = session
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || {
            Err(ClipboardImageError::NoImage)
        });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(
        session.core.input_manager.attachments(),
        &[existing_attachment]
    );
    assert_eq!(warning_text(&session), "No image found in clipboard.");
}

#[test]
fn clipboard_unavailable_warning_leaves_text_unchanged() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    set_image_input_enabled(&mut session, true);

    let event = session
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::ALT), || {
            Err(ClipboardImageError::ClipboardUnavailable)
        });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(
        warning_text(&session),
        "Clipboard image paste is unavailable in this terminal or desktop session."
    );
}

#[test]
fn wsl_fallback_failure_warning_leaves_text_unchanged() {
    let mut session = app_session_with_input("keep text", "keep text".len());
    set_image_input_enabled(&mut session, true);

    let event = session
        .process_key_with_clipboard_image_reader(image_paste_key(KeyModifiers::CONTROL), || {
            Err(ClipboardImageError::WslFallbackFailure)
        });

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "keep text");
    assert_eq!(
        warning_text(&session),
        "Could not read a clipboard image from Windows via PowerShell."
    );
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
