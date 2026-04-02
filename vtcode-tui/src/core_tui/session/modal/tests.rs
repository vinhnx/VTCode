use super::render::highlight_segments;
use super::*;
use crate::config::constants::ui;
use crate::ui::tui::{
    InlineEvent, InlineListItem, InlineListSearchConfig, InlineListSelection, OverlayEvent,
    OverlaySelectionChange, OverlaySubmission, WizardStep, types::WizardModalMode,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};

fn base_item(title: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_owned(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
    }
}

fn sample_list_modal() -> ModalState {
    let items = vec![
        InlineListItem {
            title: "First".to_owned(),
            selection: Some(InlineListSelection::Model(0)),
            search_value: Some("general".to_owned()),
            ..base_item("First")
        },
        InlineListItem {
            title: "Second".to_owned(),
            selection: Some(InlineListSelection::Model(1)),
            search_value: Some("other".to_owned()),
            ..base_item("Second")
        },
    ];

    let list_state = ModalListState::new(items, None);
    let search_state = ModalSearchState::from(InlineListSearchConfig {
        label: "Search".to_owned(),
        placeholder: None,
    });

    let mut modal = ModalState {
        title: "Test".to_owned(),
        lines: vec![],
        footer_hint: None,
        hotkeys: Vec::new(),
        list: Some(list_state),
        secure_prompt: None,
        restore_input: true,
        restore_cursor: true,
        search: Some(search_state),
    };

    if let Some(list) = modal.list.as_mut() {
        let query = modal
            .search
            .as_ref()
            .map(|state| state.query.clone())
            .unwrap_or_default();
        list.apply_search(&query);
    }

    modal
}

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

fn make_key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn single_custom_note_step(default: Option<&str>) -> WizardStep {
    WizardStep {
        title: "Q1".to_owned(),
        question: "Pick".to_owned(),
        items: vec![InlineListItem {
            title: "Custom note".to_owned(),
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: "q1".to_owned(),
                selected: vec![],
                other: Some(String::new()),
            }),
            ..base_item("Custom note")
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Custom note".to_string()),
        freeform_placeholder: Some("Type your response...".to_string()),
        freeform_default: default.map(ToOwned::to_owned),
    }
}

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

    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        steps,
        0,
        None,
        WizardModalMode::TabbedList,
    );

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

    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        steps,
        0,
        None,
        WizardModalMode::TabbedList,
    );

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());

    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => {
            assert_eq!(selections.len(), 1);
            assert!(matches!(
                selections[0],
                InlineListSelection::AskUserChoice { .. }
            ));
        }
        other => panic!("Expected submit, got: {:?}", other),
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

    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        steps,
        0,
        None,
        WizardModalMode::TabbedList,
    );

    let result = wizard.handle_mouse_click(1);
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::AskUserChoice { choice_id, .. } => {
                    assert_eq!(choice_id, "choice_b");
                }
                other => panic!("unexpected selection: {:?}", other),
            }
        }
        other => panic!("Expected submit, got: {:?}", other),
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

    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        steps,
        0,
        None,
        WizardModalMode::TabbedList,
    );

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

    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        steps,
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let result = wizard.handle_key_event(
        &make_key_with_modifiers(KeyCode::Char('n'), KeyModifiers::CONTROL),
        ModalKeyModifiers {
            control: true,
            alt: false,
            command: false,
        },
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

    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        steps,
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let result = wizard.handle_key_event(&make_key(KeyCode::Down), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));
    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));

    let result =
        wizard.handle_key_event(&make_key(KeyCode::Char('m')), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));
    let result =
        wizard.handle_key_event(&make_key(KeyCode::Char('e')), ModalKeyModifiers::default());
    assert!(matches!(result, ModalListKeyResult::Redraw));

    let result = wizard.handle_key_event(&make_key(KeyCode::Enter), ModalKeyModifiers::default());
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { other, .. } => {
                    assert_eq!(other.as_deref(), Some("me"));
                }
                other => panic!("unexpected selection: {:?}", other),
            }
        }
        other => panic!("Expected submit, got: {:?}", other),
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
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { other, .. } => {
                    assert_eq!(other.as_deref(), Some("10m"));
                }
                other => panic!("unexpected selection: {:?}", other),
            }
        }
        other => panic!("Expected submit, got: {:?}", other),
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
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { other, .. } => {
                    assert_eq!(other.as_deref(), Some(""));
                }
                other => panic!("unexpected selection: {:?}", other),
            }
        }
        other => panic!("Expected submit, got: {:?}", other),
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
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => match &selections[0] {
            InlineListSelection::RequestUserInputAnswer { other, .. } => {
                assert_eq!(other.as_deref(), Some("20m"));
            }
            other => panic!("unexpected selection: {:?}", other),
        },
        other => panic!("Expected submit, got: {:?}", other),
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
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => match &selections[0] {
            InlineListSelection::RequestUserInputAnswer { other, .. } => {
                assert_eq!(other.as_deref(), Some("10m"));
            }
            other => panic!("unexpected selection: {:?}", other),
        },
        other => panic!("Expected submit, got: {:?}", other),
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

    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        steps,
        0,
        None,
        WizardModalMode::MultiStep,
    );

    let result =
        wizard.handle_key_event(&make_key(KeyCode::Char('2')), ModalKeyModifiers::default());
    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections),
        ))) => {
            assert_eq!(selections.len(), 1);
            match &selections[0] {
                InlineListSelection::RequestUserInputAnswer { selected, .. } => {
                    assert_eq!(selected, &vec!["Choice B".to_owned()]);
                }
                other => panic!("unexpected selection: {:?}", other),
            }
        }
        other => panic!("Expected submit, got: {:?}", other),
    }
}

fn sample_list_modal_with_count(count: usize) -> ModalState {
    let items = (0..count)
        .map(|index| {
            let label = format!("Item {}", index + 1);
            InlineListItem {
                selection: Some(InlineListSelection::Model(index)),
                search_value: Some(label.to_ascii_lowercase()),
                ..base_item(&label)
            }
        })
        .collect::<Vec<_>>();

    ModalState {
        title: "Test".to_owned(),
        lines: vec![],
        footer_hint: None,
        hotkeys: Vec::new(),
        list: Some(ModalListState::new(items, None)),
        secure_prompt: None,
        restore_input: true,
        restore_cursor: true,
        search: None,
    }
}

#[test]
fn apply_search_retains_related_structure() {
    let divider = InlineListItem {
        title: ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(3),
        ..base_item("")
    };
    let header = InlineListItem {
        search_value: Some("General Models".to_owned()),
        ..base_item("Models")
    };
    let matching = InlineListItem {
        indent: 1,
        selection: Some(InlineListSelection::Model(0)),
        search_value: Some("general purpose".to_owned()),
        ..base_item("General Purpose")
    };
    let non_matching = InlineListItem {
        selection: Some(InlineListSelection::Model(1)),
        search_value: Some("specialized".to_owned()),
        ..base_item("Specialized")
    };

    let mut state = ModalListState::new(vec![divider, header, matching, non_matching], None);

    state.apply_search("general");

    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();

    let expected_divider = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(3);
    assert_eq!(
        visible_titles,
        vec![
            expected_divider,
            "Models".to_owned(),
            "General Purpose".to_owned(),
            "Specialized".to_owned()
        ]
    );
    assert_eq!(state.visible_selectable_count(), 2);
    assert_eq!(state.filter_query(), Some("general"));

    state.apply_search("");
    assert_eq!(state.visible_indices.len(), state.items.len());
    assert!(state.filter_query().is_none());
}

#[test]
fn highlight_segments_marks_matching_spans() {
    let segments = highlight_segments(
        "Hello",
        Style::default(),
        Style::default().add_modifier(Modifier::BOLD),
        &["el".to_owned()],
    );

    assert_eq!(segments.len(), 3);
    let first: &str = segments[0].content.as_ref();
    assert_eq!(first, "H");
    assert_eq!(segments[0].style, Style::default());
    let second: &str = segments[1].content.as_ref();
    assert_eq!(second, "el");
    assert_eq!(
        segments[1].style,
        Style::default().add_modifier(Modifier::BOLD)
    );
    let third: &str = segments[2].content.as_ref();
    assert_eq!(third, "lo");
    assert_eq!(segments[2].style, Style::default());
}

#[test]
fn list_modal_handles_search_typing() {
    let mut modal = sample_list_modal();
    let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    match result {
        ModalListKeyResult::Redraw => {}
        other => panic!("expected redraw, got {:?}", other),
    }

    let query = modal.search.unwrap().query.clone();
    assert_eq!(query, "g");
}

#[test]
fn list_modal_submit_emits_event() {
    let mut modal = sample_list_modal();
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    match result {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Selection(selection),
        ))) => {
            assert_eq!(selection, InlineListSelection::Model(0));
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[test]
fn list_modal_mouse_click_selects_then_submits() {
    let mut modal = sample_list_modal();

    let select = modal.handle_list_mouse_click(1);
    match select {
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
            OverlaySelectionChange::List(selection),
        ))) => {
            assert_eq!(selection, InlineListSelection::Model(1));
        }
        other => panic!("unexpected selection result: {:?}", other),
    }

    let submit = modal.handle_list_mouse_click(1);
    match submit {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Selection(selection),
        ))) => {
            assert_eq!(selection, InlineListSelection::Model(1));
        }
        other => panic!("unexpected submit result: {:?}", other),
    }
}

#[test]
fn list_modal_space_no_longer_submits_config_action() {
    let mut modal = ModalState {
        title: "Config".to_owned(),
        lines: vec![],
        footer_hint: None,
        hotkeys: Vec::new(),
        list: Some(ModalListState::new(
            vec![InlineListItem {
                title: "Permission mode".to_owned(),
                subtitle: Some("permissions.default_mode = auto".to_owned()),
                badge: Some("Toggle".to_owned()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "permissions.default_mode:cycle".to_owned(),
                )),
                search_value: None,
            }],
            None,
        )),
        secure_prompt: None,
        restore_input: true,
        restore_cursor: true,
        search: None,
    };

    let key = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    assert!(matches!(result, ModalListKeyResult::NotHandled));
}

#[test]
fn list_modal_alt_d_does_not_toggle_density_for_config_actions() {
    let mut modal = ModalState {
        title: "Config".to_owned(),
        lines: vec![],
        footer_hint: None,
        hotkeys: Vec::new(),
        list: Some(ModalListState::new(
            vec![InlineListItem {
                title: "Permission mode".to_owned(),
                subtitle: Some("permissions.default_mode = auto".to_owned()),
                badge: Some("Toggle".to_owned()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "permissions.default_mode:cycle".to_owned(),
                )),
                search_value: None,
            }],
            None,
        )),
        secure_prompt: None,
        restore_input: true,
        restore_cursor: true,
        search: None,
    };

    assert!(
        !modal
            .list
            .as_ref()
            .expect("config list should exist")
            .compact_rows()
    );

    let result = modal.handle_list_key_event(
        &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
        ModalKeyModifiers {
            alt: true,
            ..ModalKeyModifiers::default()
        },
    );

    assert!(matches!(result, ModalListKeyResult::HandledNoRedraw));
    assert!(
        !modal
            .list
            .as_ref()
            .expect("config list should exist")
            .compact_rows()
    );
}

#[test]
fn model_list_defaults_to_comfortable_density() {
    let list = ModalListState::new(
        vec![InlineListItem {
            title: "gpt-5".to_owned(),
            subtitle: Some("High-quality general reasoning model".to_owned()),
            badge: Some("Default".to_owned()),
            indent: 0,
            selection: Some(InlineListSelection::Model(0)),
            search_value: Some("gpt-5".to_owned()),
        }],
        None,
    );

    assert!(
        !list.compact_rows(),
        "model picker should default to comfortable row density"
    );
    assert!(
        list.supports_density_toggle(),
        "model picker density should remain adjustable"
    );
}

#[test]
fn list_modal_cancel_emits_event() {
    let mut modal = sample_list_modal();
    if let Some(search) = modal.search.as_mut() {
        search.query = "value".to_owned();
    }

    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    match result {
        ModalListKeyResult::Redraw => {}
        other => panic!("expected redraw to clear query first, got {:?}", other),
    }

    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    match result {
        ModalListKeyResult::Cancel(InlineEvent::Overlay(OverlayEvent::Cancelled)) => {}
        other => panic!("expected cancel event, got {:?}", other),
    }
}

#[test]
fn list_modal_tab_moves_forward() {
    let mut modal = sample_list_modal();
    let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
            OverlaySelectionChange::List(InlineListSelection::Model(1))
        )))
    ));
    let selection = modal
        .list
        .as_ref()
        .and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(1)));
}

#[test]
fn list_modal_backtab_moves_backward() {
    let mut modal = sample_list_modal();
    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    let _ = modal.handle_list_key_event(&down, ModalKeyModifiers::default());

    let key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
            OverlaySelectionChange::List(InlineListSelection::Model(0))
        )))
    ));
    let selection = modal
        .list
        .as_ref()
        .and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}

#[test]
fn list_modal_control_navigation_moves_selection() {
    let mut modal = sample_list_modal();
    let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    let _ = modal.handle_list_key_event(&tab, ModalKeyModifiers::default());

    let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
    let result = modal.handle_list_key_event(
        &ctrl_p,
        ModalKeyModifiers {
            control: true,
            alt: false,
            command: false,
        },
    );

    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
            OverlaySelectionChange::List(InlineListSelection::Model(0))
        )))
    ));
    let selection = modal
        .list
        .as_ref()
        .and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}

#[test]
fn list_search_preserves_selection_when_item_matches() {
    let mut modal = sample_list_modal();
    let list = modal.list.as_mut().expect("list state");
    list.select_next();

    let previous = list.current_selection();
    list.apply_search("other");

    assert_eq!(list.current_selection(), previous);
}

#[test]
fn list_search_resets_selection_when_item_removed() {
    let mut modal = sample_list_modal();
    let list = modal.list.as_mut().expect("list state");
    list.select_next();

    list.apply_search("general");

    assert_eq!(
        list.current_selection(),
        Some(InlineListSelection::Model(0))
    );
}

#[test]
fn list_modal_page_navigation_respects_viewport() {
    let mut modal = sample_list_modal_with_count(6);
    let list = modal.list.as_mut().expect("list state");
    list.set_viewport_rows(3);

    let page_down = KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&page_down, ModalKeyModifiers::default());
    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
            OverlaySelectionChange::List(InlineListSelection::Model(3))
        )))
    ));

    let selection = modal
        .list
        .as_ref()
        .and_then(|state| state.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(3)));

    let page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&page_up, ModalKeyModifiers::default());
    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(
            OverlaySelectionChange::List(InlineListSelection::Model(0))
        )))
    ));

    let selection = modal
        .list
        .as_ref()
        .and_then(|state| state.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}
