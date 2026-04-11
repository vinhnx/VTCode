use super::helpers::*;
use super::super::*;

#[test]
fn wizard_multistep_submit_keeps_modal_open_until_last_step() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let steps = vec![
        request_user_input_step("q1", "Scope"),
        request_user_input_step("q2", "Priority"),
    ];

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Questions".to_string(),
            steps,
            current_step: 0,
            search: None,
            mode: WizardModalMode::MultiStep,
        })),
    });
    assert!(session.wizard_overlay().is_some());

    let first_submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(first_submit.is_none());
    assert!(
        session.wizard_overlay().is_some(),
        "wizard should remain open after intermediate step completion"
    );
    assert_eq!(
        session.wizard_overlay().map(|wizard| wizard.current_step),
        Some(1)
    );

    let final_submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        final_submit,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections)
        ))) if selections.len() == 2
    ));
    assert!(
        session.wizard_overlay().is_none(),
        "wizard should close after final submission"
    );
}

#[test]
fn wizard_multistep_defaulted_enter_advances_and_returns_default_answer() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let steps = vec![
        request_user_input_custom_step("q1", "Cadence", "10m"),
        request_user_input_step("q2", "Priority"),
    ];

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Questions".to_string(),
            steps,
            current_step: 0,
            search: None,
            mode: WizardModalMode::MultiStep,
        })),
    });
    assert!(session.wizard_overlay().is_some());

    let first_submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(first_submit.is_none());
    assert!(session.wizard_overlay().is_some());
    assert_eq!(
        session.wizard_overlay().map(|wizard| wizard.current_step),
        Some(1)
    );

    let final_submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match final_submit {
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => {
            assert_eq!(selections.len(), 2);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { other, .. } => {
                    assert_eq!(other.as_deref(), Some("10m"));
                }
                other => panic!("unexpected first selection: {:?}", other),
            }
        }
        other => panic!("Expected final wizard submission, got {:?}", other),
    }
    assert!(session.wizard_overlay().is_none());
}

#[test]
fn wizard_search_paste_updates_filter_in_session_handle_event() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let (tx, _rx) = mpsc::unbounded_channel();

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Question".to_string(),
            steps: vec![WizardStep {
                title: "Choose".to_string(),
                question: "Pick one".to_string(),
                items: vec![
                    InlineListItem {
                        title: "Scope".to_string(),
                        subtitle: None,
                        badge: None,
                        indent: 0,
                        selection: Some(InlineListSelection::SlashCommand("scope".to_string())),
                        search_value: Some("scope".to_string()),
                    },
                    InlineListItem {
                        title: "Priority".to_string(),
                        subtitle: None,
                        badge: None,
                        indent: 0,
                        selection: Some(InlineListSelection::SlashCommand("priority".to_string())),
                        search_value: Some("priority".to_string()),
                    },
                ],
                completed: false,
                answer: None,
                allow_freeform: false,
                freeform_label: None,
                freeform_placeholder: None,
                freeform_default: None,
            }],
            current_step: 0,
            search: Some(InlineListSearchConfig {
                label: "Filter".to_string(),
                placeholder: None,
            }),
            mode: WizardModalMode::MultiStep,
        })),
    });

    session.handle_event(CrosstermEvent::Paste("prio".to_string()), &tx, None);

    let wizard = session.wizard_overlay().expect("wizard should stay open");
    assert_eq!(
        wizard.search.as_ref().map(|search| search.query.as_str()),
        Some("prio")
    );
    assert_eq!(wizard.steps[0].list.visible_indices, vec![1]);
}

#[test]
fn wizard_tabbed_submit_closes_modal_immediately() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let steps = vec![request_user_input_step("q1", "Single choice")];

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Question".to_string(),
            steps,
            current_step: 0,
            search: None,
            mode: WizardModalMode::TabbedList,
        })),
    });
    assert!(session.wizard_overlay().is_some());

    let submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        submit,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections)
        ))) if selections.len() == 1
    ));
    assert!(session.wizard_overlay().is_none());
}

