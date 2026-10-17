//! Clipboard paste handling: text, image, and raw-text shortcuts.

use super::*;

pub(crate) fn handle_paste(session: &mut Session, content: &str) -> Option<InlineEvent> {
    // Secure prompt modal: auto-submit pasted content directly without requiring Enter.
    // This saves the API key to .env immediately and dismisses the modal.
    if let Some(modal) = session.modal_state_mut()
        && modal.secure_prompt.is_some()
        && modal.list.is_none()
    {
        let submitted = content.trim().to_string();
        if submitted.is_empty() {
            return None;
        }
        // Close the modal immediately so the UI does not show a stale overlay
        // while the event is in-flight to the interaction loop.
        session.close_overlay();
        session.mark_dirty();
        return Some(InlineEvent::Submit(submitted.into()));
    }

    if let Some(viewer) = session.tool_output_viewer_state_mut()
        && viewer.search_active()
    {
        viewer.insert_search_text(content);
        session.mark_dirty();
    } else if session.history_picker_visible() {
        let history = input_history_entries(session);
        session.history_picker_state.search_query.push_str(content);
        session.history_picker_state.update_search(&history);
        session.mark_dirty();
    } else if let Some(modal) = session.modal_state_mut()
        && let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut())
    {
        search.insert(content);
        list.apply_search(&search.query, search.fuzzy);
        session.mark_dirty();
    } else if let Some(wizard) = session.wizard_overlay_mut()
        && let Some(search) = wizard.search.as_mut()
    {
        search.insert(content);
        if let Some(step) = wizard.steps.get_mut(wizard.current_step) {
            step.list.apply_search(&search.query, search.fuzzy);
        }
        session.mark_dirty();
    } else if let Some(wizard) = session.wizard_overlay_mut()
        && let Some(step) = wizard.steps.get_mut(wizard.current_step)
        && (step.notes_active || modal::inline_editor_for_step(step).is_some())
    {
        // Mirror typed input: pasted text lands in the custom-note editor when
        // it is active (or when the custom-note item is selected).
        step.notes_active = true;
        let mut state = InputState::new();
        state.set_value(step.notes.clone());
        state.end();
        for ch in content.chars().filter(|ch| !matches!(ch, '\n' | '\r')) {
            state.insert_char(ch);
        }
        step.notes = state.value().to_owned();
        session.mark_dirty();
    } else if session.core.input_enabled()
        && !session.visible_transient_surface().is_some_and(|surface| {
            matches!(surface.focus_policy(), TransientFocusPolicy::Modal | TransientFocusPolicy::CapturedInput)
        })
    {
        session.insert_paste_text(content);
        session.update_input_triggers();
        session.mark_dirty();
    }
    None
}

pub(crate) fn copy_selected_input_if_requested(session: &mut Session, key: &KeyEvent, has_command: bool) -> bool {
    // Composer selection must not pre-empt modal/picker Ctrl+C handling.
    // While the history picker (or any modal surface) owns input, the
    // composer selection is stale — let the overlay dismiss path run so
    // Ctrl+C closes the popup instead of being swallowed as a copy.
    // Mirrors the `!input_enabled()` gate in core `session/events.rs`.
    if !session.core.input_enabled() {
        return false;
    }
    if has_command && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
        if session.core.copy_input_selection_to_clipboard() {
            session.mark_dirty();
        }
        return true;
    }

    let is_copy_shortcut = match key.code {
        KeyCode::Char('c') | KeyCode::Char('C') => key.modifiers.contains(KeyModifiers::CONTROL),
        KeyCode::Char('\u{3}') => true,
        _ => false,
    };

    if !is_copy_shortcut {
        return false;
    }

    if session.core.copy_input_selection_to_clipboard() {
        session.mark_dirty();
        return true;
    }

    false
}

pub(crate) fn image_paste_warning(error: ClipboardImageError) -> &'static str {
    match error {
        ClipboardImageError::NoImage => "No image found in clipboard.",
        ClipboardImageError::ClipboardUnavailable => {
            "Clipboard image paste is unavailable in this terminal or desktop session."
        }
        ClipboardImageError::UnsupportedModel => "The selected model does not support image input.",
        ClipboardImageError::WslFallbackFailure => "Could not read a clipboard image from Windows via PowerShell.",
    }
}

pub(crate) fn push_warning_line(session: &mut Session, text: &'static str) {
    session.push_line(
        InlineMessageKind::Warning,
        vec![InlineSegment {
            text: text.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.core.request_transcript_clear();
    session.mark_dirty();
}

pub(crate) fn is_image_paste_shortcut(
    key: &KeyEvent,
    has_control: bool,
    has_alt: bool,
    has_command: bool,
    has_shift: bool,
) -> bool {
    matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
        && !has_command
        && !has_shift
        && ((has_control && !has_alt) || (has_alt && !has_control))
}

pub(crate) fn raw_text_paste_warning(error: ClipboardTextError) -> &'static str {
    match error {
        ClipboardTextError::NoText => "No text found in clipboard.",
        ClipboardTextError::ClipboardUnavailable => {
            "Clipboard text paste is unavailable in this terminal or desktop session."
        }
        ClipboardTextError::WslFallbackFailure => "Could not read clipboard text from Windows via PowerShell.",
    }
}

/// Shift+Ctrl+V (or Shift+Alt+V) pastes clipboard text verbatim: full raw
/// content with no collapse marker, even for large pastes.
pub(crate) fn is_raw_text_paste_shortcut(
    key: &KeyEvent,
    has_control: bool,
    has_alt: bool,
    has_command: bool,
    has_shift: bool,
) -> bool {
    matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
        && has_shift
        && !has_command
        && ((has_control && !has_alt) || (has_alt && !has_control))
}

pub(crate) fn handle_raw_text_paste_shortcut_with(
    session: &mut Session,
    mut text_reader: impl FnMut() -> Result<String, ClipboardTextError>,
) {
    match text_reader() {
        Ok(text) => {
            session.core.insert_raw_paste_text(&text);
            session.update_input_triggers();
            session.mark_dirty();
        }
        Err(error) => push_warning_line(session, raw_text_paste_warning(error)),
    }
}

pub(crate) fn handle_image_paste_shortcut_with(
    session: &mut Session,
    mut image_reader: impl FnMut() -> Result<ContentPart, ClipboardImageError>,
) {
    if !session.core.image_input_enabled() {
        push_warning_line(session, image_paste_warning(ClipboardImageError::UnsupportedModel));
        return;
    }

    match image_reader() {
        Ok(image) => {
            if let Some(attachment_number) = session.core.input_manager.push_attachment(image) {
                session.core.input_manager.insert_text(&format!("[Image #{attachment_number}]"));
            }
            session
                .core
                .set_input_compact_mode(session.core.input_compact_placeholder().is_some());
            session.update_input_triggers();
            session.mark_dirty();
        }
        Err(error) => push_warning_line(session, image_paste_warning(error)),
    }
}
