#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;
use crate::tui::core_tui::types::{ModalOverlayRequest, SecurePromptConfig};

fn show_list_modal(session: &mut Session, search: bool) {
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: "Pick".to_string(),
            lines: Vec::new(),
            footer_hint: None,
            items: vec![InlineListItem {
                title: "alpha".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::SlashCommand("a".to_string())),
                search_value: Some("alpha".to_string()),
                ..Default::default()
            }],
            selected: None,
            search: search.then(|| InlineListSearchConfig {
                label: "Filter".to_string(),
                placeholder: None,
                fuzzy: false,
            }),
            hotkeys: Vec::new(),
            status: None,
        })),
    });
}

fn focus_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT)
}

#[test]
fn focus_input_cancels_list_modal_with_cancel_event() {
    let mut session = session_with_input("draft", 5);
    show_list_modal(&mut session, false);
    assert!(session.has_active_overlay());

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(!session.has_active_overlay(), "modal should close");
    assert!(
        matches!(event, Some(InlineEvent::Overlay(OverlayEvent::Cancelled))),
        "modal cancel must emit like Esc, got {event:?}"
    );
    assert_eq!(session.input_manager.content(), "draft", "draft must survive");
}

#[test]
fn focus_input_closes_search_modal_in_one_press() {
    let mut session = session_with_input("draft", 5);
    show_list_modal(&mut session, true);
    session
        .modal_state_mut()
        .expect("modal open")
        .search
        .as_mut()
        .expect("search enabled")
        .query = "al".to_string();

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(!session.has_active_overlay(), "one press lands in input, not just clear search");
    assert!(matches!(event, Some(InlineEvent::Overlay(OverlayEvent::Cancelled))), "got {event:?}");
}

#[test]
fn focus_input_exits_reverse_search_and_restores_draft() {
    let mut session = session_with_input("draft", 5);
    let history = session.input_manager.history_texts();
    session.reverse_search_state.start_search(&session.input_manager, &history);
    assert!(session.reverse_search_state.active);

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(event.is_none());
    assert!(!session.reverse_search_state.active);
    assert_eq!(session.input_manager.content(), "draft");
}

#[test]
fn focus_input_cancels_text_only_modal_with_cancel_event() {
    let mut session = session_with_input("draft", 5);
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Modal(ModalOverlayRequest {
            title: "Notice".to_string(),
            lines: vec!["read me".to_string()],
            secure_prompt: None,
            is_help_modal: false,
        })),
    });
    assert!(session.has_active_overlay());

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(!session.has_active_overlay(), "modal should close");
    assert!(
        matches!(event, Some(InlineEvent::Overlay(OverlayEvent::Cancelled))),
        "core Esc emits Cancel for text-only modals, got {event:?}"
    );
    assert_eq!(session.input_manager.content(), "draft", "draft must survive");
}

#[test]
fn focus_input_clears_secure_scratch_like_escape() {
    let mut session = session_with_input("sk-secret", 9);
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Modal(ModalOverlayRequest {
            title: "API key".to_string(),
            lines: Vec::new(),
            secure_prompt: Some(SecurePromptConfig {
                label: "key".to_string(),
                placeholder: None,
                mask_input: true,
            }),
            is_help_modal: false,
        })),
    });
    assert!(session.has_active_overlay());

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(event.is_none(), "secure cancel is silent, got {event:?}");
    assert!(!session.has_active_overlay(), "secure prompt closed");
    assert!(session.input_manager.content().is_empty(), "secret must not strand in the draft");
}

#[test]
fn focus_input_cancels_plain_wizard_with_cancel_event() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Questions".to_string(),
            steps: vec![request_user_input_step("q1", "Scope")],
            current_step: 0,
            search: None,
            mode: WizardModalMode::MultiStep,
        })),
    });
    assert!(session.wizard_overlay().is_some());

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(session.wizard_overlay().is_none(), "wizard should close");
    assert!(
        matches!(event, Some(InlineEvent::Overlay(OverlayEvent::Cancelled))),
        "wizard cancel must emit, got {event:?}"
    );
}

#[test]
fn focus_input_cancels_worst_case_wizard_in_one_press() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Questions".to_string(),
            steps: vec![request_user_input_step("q1", "Scope")],
            current_step: 0,
            search: Some(InlineListSearchConfig {
                label: "Filter".to_string(),
                placeholder: None,
                fuzzy: false,
            }),
            mode: WizardModalMode::MultiStep,
        })),
    });
    // Deepest sub-state stack: active notes editor plus a search query needs
    // three Esc passes (notes, query, cancel) — one FocusInput must do all.
    let wizard = session.wizard_overlay_mut().expect("wizard open");
    wizard.search.as_mut().expect("search enabled").query = "sc".to_string();
    let step = wizard.current_step;
    wizard.steps[step].notes_active = true;
    wizard.steps[step].notes = "note".to_string();

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(session.wizard_overlay().is_none(), "wizard should close in one press");
    assert!(
        matches!(event, Some(InlineEvent::Overlay(OverlayEvent::Cancelled))),
        "wizard cancel must emit, got {event:?}"
    );
}

#[test]
fn focus_input_is_noop_at_input() {
    let mut session = session_with_input("hello", 2);

    let event = events::dispatch_rebindable_action(&mut session, Action::FocusInput);

    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "hello");
    assert!(!session.has_active_overlay());
}

#[test]
fn focus_input_key_closes_file_palette_and_history_picker() {
    let mut session = app_session_with_input("", 0);
    load_app_file_palette(&mut session, vec!["src/main.rs".to_string()], PathBuf::from("."));
    session.handle_command(app_types::InlineCommand::SetInput("@src".to_string()));
    assert!(session.file_palette_active);
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(session.history_picker_state.active);

    let event = session.process_key(focus_key());

    assert!(event.is_none());
    assert!(!session.file_palette_active, "file palette closed");
    assert!(!session.history_picker_state.active, "history picker closed");
    assert_eq!(session.core.input_manager.content(), "@src", "draft preserved");
}
