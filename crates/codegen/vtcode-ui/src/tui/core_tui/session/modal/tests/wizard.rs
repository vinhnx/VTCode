use super::*;

#[test]
fn wizard_tabbed_list_allows_tab_switching_without_completion() {
    let steps = vec![
        WizardStep {
            title: "Tab A".to_owned(),
            question: "Pick A".to_owned(),
            items: vec![InlineListItem {
                title: "A1".to_owned(),
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "a".to_owned(),
                    choice_id: "a1".to_owned(),
                    text: None,
                }),
                ..base_item("A1")
            }],
            completed: false,
            answer: None,
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
        WizardStep {
            title: "Tab B".to_owned(),
            question: "Pick B".to_owned(),
            items: vec![InlineListItem {
                title: "B1".to_owned(),
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "b".to_owned(),
                    choice_id: "b1".to_owned(),
                    text: None,
                }),
                ..base_item("B1")
            }],
            completed: false,
            answer: None,
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
    ];

    let mut wizard = WizardModalState::new("Pick".to_owned(), steps, 0, None, WizardModalMode::TabbedList);

    assert_eq!(wizard.current_step, 0);

    let result = wizard.handle_key_event(&make_key(KeyCode::Right), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));
    assert_eq!(wizard.current_step, 1);

    let result = wizard.handle_key_event(&make_key(KeyCode::Left), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));
    assert_eq!(wizard.current_step, 0);
}
#[test]
fn wizard_tabbed_list_enter_submits_single_selection() {
    let steps = vec![WizardStep {
        title: "Tab".to_owned(),
        question: "Pick".to_owned(),
        items: vec![InlineListItem {
            title: "Choice".to_owned(),
            selection: Some(InlineListSelection::AskUserChoice {
                tab_id: "tab".to_owned(),
                choice_id: "choice".to_owned(),
                text: None,
            }),
            ..base_item("Choice")
        }],
        completed: false,
        answer: None,
        allow_freeform: false,
        freeform_label: None,
        freeform_placeholder: None,
        freeform_default: None,
    }];

    let mut wizard = WizardModalState::new("Pick".to_owned(), steps, 0, None, WizardModalMode::TabbedList);

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());

    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => {
            assert_eq!(selections.len(), 1);
            assert!(matches!(selections[0], InlineListSelection::AskUserChoice { .. }));
        }
        other => panic!("Expected submit, got: {other:?}"),
    }
}
#[test]
fn wizard_tabbed_list_mouse_click_submits_on_first_click() {
    let steps = vec![WizardStep {
        title: "Tab".to_owned(),
        question: "Pick".to_owned(),
        items: vec![
            InlineListItem {
                title: "Choice A".to_owned(),
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "tab".to_owned(),
                    choice_id: "choice_a".to_owned(),
                    text: None,
                }),
                ..base_item("Choice A")
            },
            InlineListItem {
                title: "Choice B".to_owned(),
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "tab".to_owned(),
                    choice_id: "choice_b".to_owned(),
                    text: None,
                }),
                ..base_item("Choice B")
            },
        ],
        completed: false,
        answer: None,
        allow_freeform: false,
        freeform_label: None,
        freeform_placeholder: None,
        freeform_default: None,
    }];

    let mut wizard = WizardModalState::new("Pick".to_owned(), steps, 0, None, WizardModalMode::TabbedList);

    let result = wizard.handle_mouse_click(1);
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::AskUserChoice { choice_id, .. } => {
                    assert_eq!(choice_id, "choice_b");
                }
                other => panic!("unexpected selection: {other:?}"),
            }
        }
        other => panic!("Expected submit, got: {other:?}"),
    }
}
#[test]
fn wizard_tabbed_list_mouse_click_activates_custom_note_editor() {
    let steps = vec![WizardStep {
        title: "Tab".to_owned(),
        question: "Pick".to_owned(),
        items: vec![
            InlineListItem {
                title: "Choice A".to_owned(),
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "tab".to_owned(),
                    choice_id: "choice_a".to_owned(),
                    text: None,
                }),
                ..base_item("Choice A")
            },
            InlineListItem {
                title: "Other".to_owned(),
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: "tab".to_owned(),
                    selected: vec![],
                    other: Some(String::new()),
                }),
                ..base_item("Other")
            },
        ],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Other".to_owned()),
        freeform_placeholder: Some("Type your response...".to_owned()),
        freeform_default: None,
    }];

    let mut wizard = WizardModalState::new("Pick".to_owned(), steps, 0, None, WizardModalMode::TabbedList);

    let result = wizard.handle_mouse_click(1);
    assert!(matches!(result, ModalListKeyResult::Redraw));
    assert!(wizard.steps[0].notes_active);
    assert_eq!(wizard.steps[0].list.list_state.selected(), Some(1));
}
#[test]
fn wizard_multistep_ctrl_n_advances_without_completion() {
    let steps = vec![
        WizardStep {
            title: "Q1".to_owned(),
            question: "Pick".to_owned(),
            items: vec![InlineListItem {
                title: "Choice".to_owned(),
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: "q1".to_owned(),
                    selected: vec!["Choice".to_owned()],
                    other: None,
                }),
                ..base_item("Choice")
            }],
            completed: false,
            answer: None,
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
        WizardStep {
            title: "Q2".to_owned(),
            question: "Pick".to_owned(),
            items: vec![InlineListItem {
                title: "Choice".to_owned(),
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: "q2".to_owned(),
                    selected: vec!["Choice".to_owned()],
                    other: None,
                }),
                ..base_item("Choice")
            }],
            completed: false,
            answer: None,
            allow_freeform: false,
            freeform_label: None,
            freeform_placeholder: None,
            freeform_default: None,
        },
    ];

    let mut wizard = WizardModalState::new("Pick".to_owned(), steps, 0, None, WizardModalMode::MultiStep);

    let result = wizard.handle_key_event(
        &make_key_with_modifiers(KeyCode::Char('n'), KeyModifiers::CONTROL),
        ModalKeyModifiers { control: true, alt: false, command: false },
    );
    assert!(matches!(result, ModalListKeyResult::Redraw));
    assert_eq!(wizard.current_step, 1);
    assert!(!wizard.steps[0].completed);
}
#[test]
fn wizard_inline_custom_note_sets_other_answer_and_submits_on_enter() {
    let steps = vec![WizardStep {
        title: "Q1".to_owned(),
        question: "Pick".to_owned(),
        items: vec![
            InlineListItem {
                title: "Option A".to_owned(),
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: "q1".to_owned(),
                    selected: vec!["Option A".to_owned()],
                    other: None,
                }),
                ..base_item("Option A")
            },
            InlineListItem {
                title: "Custom note".to_owned(),
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: "q1".to_owned(),
                    selected: vec![],
                    other: Some(String::new()),
                }),
                ..base_item("Custom note")
            },
        ],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Custom note".to_string()),
        freeform_placeholder: Some("Type your response...".to_string()),
        freeform_default: None,
    }];

    let mut wizard = WizardModalState::new("Pick".to_owned(), steps, 0, None, WizardModalMode::MultiStep);

    let result = wizard.handle_key_event(&make_key(KeyCode::Down), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));
    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));

    let result = wizard.handle_key_event(&make_key(KeyCode::Char('m')), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));
    let result = wizard.handle_key_event(&make_key(KeyCode::Char('e')), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { other, .. } => {
                    assert_eq!(other.as_deref(), Some("me"));
                }
                other => panic!("unexpected selection: {other:?}"),
            }
        }
        other => panic!("Expected submit, got: {other:?}"),
    }
}
#[test]
fn wizard_inline_custom_note_uses_default_on_empty_enter() {
    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        vec![single_custom_note_step(Some("10m"))],
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { other, .. } => {
                    assert_eq!(other.as_deref(), Some("10m"));
                }
                other => panic!("unexpected selection: {other:?}"),
            }
        }
        other => panic!("Expected submit, got: {other:?}"),
    }
}
#[test]
fn wizard_inline_custom_note_accepts_empty_string_default_on_enter() {
    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        vec![single_custom_note_step(Some(""))],
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { other, .. } => {
                    assert_eq!(other.as_deref(), Some(""));
                }
                other => panic!("unexpected selection: {other:?}"),
            }
        }
        other => panic!("Expected submit, got: {other:?}"),
    }
}
#[test]
fn wizard_inline_custom_note_without_default_still_requires_input() {
    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        vec![single_custom_note_step(None)],
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));
    assert!(wizard.notes_active());
}
#[test]
fn wizard_inline_custom_note_typed_text_overrides_default() {
    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        vec![single_custom_note_step(Some("10m"))],
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let _ = wizard.handle_key_event(&make_key(KeyCode::Char('2')), ModalKeyModifiers::default());
    let _ = wizard.handle_key_event(&make_key(KeyCode::Char('0')), ModalKeyModifiers::default());
    let _ = wizard.handle_key_event(&make_key(KeyCode::Char('m')), ModalKeyModifiers::default());

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => match &selections[0] {
            InlineListSelection::RequestUserInputAnswer { other, .. } => {
                assert_eq!(other.as_deref(), Some("20m"));
            }
            other => panic!("unexpected selection: {other:?}"),
        },
        other => panic!("Expected submit, got: {other:?}"),
    }
}
#[test]
fn wizard_multistep_mouse_click_uses_default_submission_path() {
    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        vec![single_custom_note_step(Some("10m"))],
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let result = wizard.handle_mouse_click(0);
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => match &selections[0] {
            InlineListSelection::RequestUserInputAnswer { other, .. } => {
                assert_eq!(other.as_deref(), Some("10m"));
            }
            other => panic!("unexpected selection: {other:?}"),
        },
        other => panic!("Expected submit, got: {other:?}"),
    }
}
#[test]
fn wizard_multistep_numeric_select_submits() {
    let steps = vec![WizardStep {
        title: "Q1".to_owned(),
        question: "Pick".to_owned(),
        items: vec![
            InlineListItem {
                title: "Choice A".to_owned(),
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: "q1".to_owned(),
                    selected: vec!["Choice A".to_owned()],
                    other: None,
                }),
                ..base_item("Choice A")
            },
            InlineListItem {
                title: "Choice B".to_owned(),
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: "q1".to_owned(),
                    selected: vec!["Choice B".to_owned()],
                    other: None,
                }),
                ..base_item("Choice B")
            },
        ],
        completed: false,
        answer: None,
        allow_freeform: false,
        freeform_label: None,
        freeform_placeholder: None,
        freeform_default: None,
    }];

    let mut wizard = WizardModalState::new("Pick".to_owned(), steps, 0, None, WizardModalMode::MultiStep);

    let result = wizard.handle_key_event(&make_key(KeyCode::Char('2')), ModalKeyModifiers::default());
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Wizard(
            selections,
        )))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { selected, .. } => {
                    assert_eq!(selected, &vec!["Choice B".to_owned()]);
                }
                other => panic!("unexpected selection: {other:?}"),
            }
        }
        other => panic!("Expected submit, got: {other:?}"),
    }
}
