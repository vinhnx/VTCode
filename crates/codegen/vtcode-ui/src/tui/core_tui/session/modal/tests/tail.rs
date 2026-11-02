use super::*;

#[test]
fn wizard_numbered_shortcuts_require_multistep_searchless_capped_step() {
    let multistep = WizardModalState::new(
        "Pick".to_owned(),
        vec![wizard_step_with_options(2)],
        0,
        None,
        WizardModalMode::MultiStep,
    );
    assert!(multistep.numbered_shortcuts());

    let crowded = WizardModalState::new(
        "Pick".to_owned(),
        vec![wizard_step_with_options(10)],
        0,
        None,
        WizardModalMode::MultiStep,
    );
    assert!(!crowded.numbered_shortcuts(), "crowded steps offer no digits");

    let tabbed = WizardModalState::new(
        "Pick".to_owned(),
        vec![wizard_step_with_options(2)],
        0,
        None,
        WizardModalMode::TabbedList,
    );
    assert!(!tabbed.numbered_shortcuts(), "tabbed wizards navigate by tabs, not digits");

    let searching = WizardModalState::new(
        "Pick".to_owned(),
        vec![wizard_step_with_options(2)],
        0,
        Some(InlineListSearchConfig {
            label: "Search".to_owned(),
            placeholder: None,
            fuzzy: false,
        }),
        WizardModalMode::MultiStep,
    );
    assert!(!searching.numbered_shortcuts(), "digits filter while search is open");
}
#[test]
fn wizard_crowded_step_swallows_digits_instead_of_submitting() {
    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        vec![wizard_step_with_options(10)],
        0,
        None,
        WizardModalMode::MultiStep,
    );
    let result = wizard.handle_key_event(&make_key(KeyCode::Char('1')), ModalKeyModifiers::default());

    assert!(matches!(result, ModalListKeyResult::HandledNoRedraw), "dead digits must not submit");
}
#[test]
fn wizard_tabbed_list_leaves_digits_unbound() {
    let mut wizard = WizardModalState::new(
        "Pick".to_owned(),
        vec![wizard_step_with_options(2)],
        0,
        None,
        WizardModalMode::TabbedList,
    );
    let result = wizard.handle_key_event(&make_key(KeyCode::Char('1')), ModalKeyModifiers::default());

    assert!(matches!(result, ModalListKeyResult::NotHandled));
}
#[test]
fn list_modal_control_navigation_moves_selection() {
    let mut modal = sample_list_modal();
    let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    let _ = modal.handle_list_key_event(&tab, ModalKeyModifiers::default());

    let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
    let result = modal.handle_list_key_event(&ctrl_p, ModalKeyModifiers { control: true, alt: false, command: false });

    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
            InlineListSelection::Model(0)
        ))))
    ));
    let selection = modal.list.as_ref().and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}
#[test]
fn list_search_preserves_selection_when_item_matches() {
    let mut modal = sample_list_modal();
    let list = modal.list.as_mut().expect("list state");
    list.select_next();

    let previous = list.current_selection();
    list.apply_search("other", false);

    assert_eq!(list.current_selection(), previous);
}
#[test]
fn list_search_resets_selection_when_item_removed() {
    let mut modal = sample_list_modal();
    let list = modal.list.as_mut().expect("list state");
    list.select_next();

    list.apply_search("general", false);

    assert_eq!(list.current_selection(), Some(InlineListSelection::Model(0)));
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
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
            InlineListSelection::Model(3)
        ))))
    ));

    let selection = modal.list.as_ref().and_then(|state| state.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(3)));

    let page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&page_up, ModalKeyModifiers::default());
    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
            InlineListSelection::Model(0)
        ))))
    ));

    let selection = modal.list.as_ref().and_then(|state| state.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}
#[test]
fn apply_search_matches_title_and_description_without_search_value() {
    let mut list = ModalListState::new(
        vec![
            InlineListItem {
                title: "Tool Display Mode".to_string(),
                subtitle: Some("expanded compact".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::Model(0)),
                search_value: None,
                ..Default::default()
            },
            InlineListItem {
                title: "Other".to_string(),
                subtitle: Some("unrelated".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::Model(1)),
                search_value: None,
                ..Default::default()
            },
        ],
        None,
    );
    list.apply_search("display", false);
    assert_eq!(list.visible_indices, vec![0]);
    list.apply_search("compact", false);
    assert_eq!(list.visible_indices, vec![0]);
}
#[test]
fn apply_search_prefers_explicit_search_value_over_title() {
    let mut list = ModalListState::new(
        vec![InlineListItem {
            title: "Hidden title terms".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Model(0)),
            search_value: Some("only-this".to_string()),
            ..Default::default()
        }],
        None,
    );
    list.apply_search("hidden", false);
    assert!(list.visible_indices.is_empty(), "search_value is the sole corpus when set");
    list.apply_search("only-this", false);
    assert_eq!(list.visible_indices, vec![0]);
}
