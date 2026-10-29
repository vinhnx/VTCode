pub(super) use super::render::highlight_segments;
pub(super) use super::*;
pub(super) use crate::tui::config::constants::ui;
pub(super) use crate::tui::ui::tui::{
    InlineEvent, InlineListItem, InlineListSearchConfig, InlineListSelection, OverlayEvent, OverlaySelectionChange,
    OverlaySubmission, WizardStep, types::WizardModalMode,
};
pub(super) use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(super) use ratatui::style::{Modifier, Style};

fn base_item(title: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_owned(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
        ..Default::default()
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
        fuzzy: false,
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
        is_help_modal: false,
        status: None,
    };

    if let Some(list) = modal.list.as_mut()
        && let Some(search) = modal.search.as_ref()
    {
        list.apply_search(&search.query, search.fuzzy);
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
        is_help_modal: false,
        status: None,
    }
}

fn searchless_approval_modal() -> ModalState {
    fn option(title: &str, selection: InlineListSelection) -> InlineListItem {
        InlineListItem {
            title: title.to_owned(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(selection),
            search_value: None,
            ..Default::default()
        }
    }
    ModalState {
        title: "Test".to_owned(),
        lines: vec![],
        footer_hint: None,
        hotkeys: Vec::new(),
        list: Some(ModalListState::new(
            vec![
                option("Approve once", InlineListSelection::Model(0)),
                option("Allow for session", InlineListSelection::Model(1)),
                InlineListItem {
                    title: String::new(),
                    subtitle: None,
                    badge: None,
                    indent: 0,
                    selection: None,
                    search_value: None,
                    ..Default::default()
                },
                option("Deny once", InlineListSelection::Model(2)),
            ],
            None,
        )),
        secure_prompt: None,
        restore_input: true,
        restore_cursor: true,
        search: None,
        status: None,
        is_help_modal: false,
    }
}

fn wizard_step_with_options(count: usize) -> WizardStep {
    WizardStep {
        title: "Q1".to_owned(),
        question: "Pick".to_owned(),
        items: (0..count)
            .map(|index| InlineListItem {
                title: format!("Choice {}", index + 1),
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "q1".to_owned(),
                    choice_id: format!("c{index}"),
                    text: None,
                }),
                ..base_item(&format!("Choice {}", index + 1))
            })
            .collect(),
        completed: false,
        answer: None,
        allow_freeform: false,
        freeform_label: None,
        freeform_placeholder: None,
        freeform_default: None,
    }
}

mod digits;
mod search;
mod tail;
mod wizard;
