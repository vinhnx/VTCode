use super::*;

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
        other => panic!("expected redraw to clear query first, got {other:?}"),
    }

    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    match result {
        ModalListKeyResult::Cancel(InlineEvent::Overlay(OverlayEvent::Cancelled)) => {}
        other => panic!("expected cancel event, got {other:?}"),
    }
}
#[test]
fn list_modal_tab_moves_forward() {
    let mut modal = sample_list_modal();
    let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
            InlineListSelection::Model(1)
        ))))
    ));
    let selection = modal.list.as_ref().and_then(|list| list.current_selection());
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
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
            InlineListSelection::Model(0)
        ))))
    ));
    let selection = modal.list.as_ref().and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}
#[test]
fn digit_key_selects_nth_option_without_submitting() {
    let mut modal = searchless_approval_modal();
    let key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    // Two-step: highlight moves (Emit), Enter still confirms.
    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
            InlineListSelection::Model(1)
        ))))
    ));
    let selection = modal.list.as_ref().and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(1)));
}
#[test]
fn digit_key_skips_separator_row() {
    let mut modal = searchless_approval_modal();
    let key = KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    // The separator carries no number: 3 lands on Deny, not the gap.
    assert!(matches!(
        result,
        ModalListKeyResult::Emit(InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(
            InlineListSelection::Model(2)
        ))))
    ));
    let selection = modal.list.as_ref().and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(2)));
}
#[test]
fn digit_for_current_selection_redraws_without_leaking() {
    let mut modal = searchless_approval_modal();
    let key = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    // Already there: no selection change, but the keypress is still
    // consumed — repeat presses must not fall through to the composer.
    assert!(matches!(result, ModalListKeyResult::Redraw));
    let selection = modal.list.as_ref().and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}
#[test]
fn out_of_range_and_zero_digits_are_swallowed() {
    for digit in ['0', '9'] {
        let mut modal = searchless_approval_modal();
        let key = KeyEvent::new(KeyCode::Char(digit), KeyModifiers::NONE);
        let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

        assert!(matches!(result, ModalListKeyResult::HandledNoRedraw), "digit {digit} must not leak");
        let selection = modal.list.as_ref().and_then(|list| list.current_selection());
        assert_eq!(selection, Some(InlineListSelection::Model(0)), "digit {digit} must not move selection");
    }
}
#[test]
fn digit_with_command_modifier_is_ignored() {
    let mut modal = searchless_approval_modal();
    let key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers { command: true, ..ModalKeyModifiers::default() });

    assert!(matches!(result, ModalListKeyResult::NotHandled));
    let selection = modal.list.as_ref().and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::Model(0)));
}
#[test]
fn digit_with_open_search_filters_instead_of_selecting() {
    let mut modal = sample_list_modal();
    let key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
    let _ = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    // Search owns digits while open: the query filters, selection is untouched.
    assert_eq!(modal.search.as_ref().map(|search| search.query.as_str()), Some("2"));
}
#[test]
fn numbered_shortcuts_gate_covers_empty_and_crowded_lists() {
    let empty = ModalListState::new(Vec::new(), None);
    assert!(!empty.numbered_shortcuts());

    let nine: Vec<InlineListItem> = (0..9)
        .map(|index| InlineListItem {
            title: format!("Option {index}"),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Model(index)),
            search_value: None,
            ..Default::default()
        })
        .collect();
    let nine = ModalListState::new(nine, None);
    assert!(nine.numbered_shortcuts());
    assert_eq!(nine.shortcut_number(8), Some(9));

    let mut ten = (0..10)
        .map(|index| InlineListItem {
            title: format!("Option {index}"),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Model(index)),
            search_value: None,
            ..Default::default()
        })
        .collect::<Vec<_>>();
    ten.push(InlineListItem {
        title: String::new(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
        ..Default::default()
    });
    let ten = ModalListState::new(ten, None);
    assert!(!ten.numbered_shortcuts(), "crowded lists offer no digits");
}
