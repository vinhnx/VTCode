use super::*;

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

    state.apply_search("general", false);

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

    state.apply_search("", false);
    assert_eq!(state.visible_indices.len(), state.items.len());
    assert!(state.filter_query().is_none());
}
#[test]
fn fuzzy_search_matches_subsequences_and_ranks_by_relevance() {
    let subsequence_match = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:model_provider".to_owned())),
        search_value: Some("model & provider choose the active provider".to_owned()),
        ..base_item("Model & Provider")
    };
    let contiguous_match = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:mp_grid".to_owned())),
        search_value: Some("mp grid".to_owned()),
        ..base_item("MP Grid")
    };
    let non_matching = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:editor".to_owned())),
        search_value: Some("editor".to_owned()),
        ..base_item("Editor")
    };

    let mut state = ModalListState::new(vec![subsequence_match, contiguous_match, non_matching], None);

    state.apply_search("mp", true);

    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();
    // "mp" is a contiguous match in "MP Grid" but only a gapped subsequence in
    // "Model & Provider", so the stronger match ranks first.
    assert_eq!(visible_titles, vec!["MP Grid".to_owned(), "Model & Provider".to_owned()]);
}
#[test]
fn fuzzy_search_matching_header_includes_whole_group() {
    let divider = InlineListItem {
        title: ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(3),
        ..base_item("")
    };
    let header = InlineListItem {
        search_value: Some("model & provider".to_owned()),
        ..base_item("Model & Provider")
    };
    let provider_child = InlineListItem {
        indent: 1,
        selection: Some(InlineListSelection::ConfigAction("settings:set:provider".to_owned())),
        search_value: Some("active provider".to_owned()),
        ..base_item("Provider")
    };
    let response_child = InlineListItem {
        indent: 1,
        selection: Some(InlineListSelection::ConfigAction("settings:set:response".to_owned())),
        search_value: Some("response behavior".to_owned()),
        ..base_item("Response Behavior")
    };
    let other_header = InlineListItem {
        selection: None,
        search_value: Some("tools & integrations".to_owned()),
        ..base_item("Tools & Integrations")
    };

    let mut state = ModalListState::new(vec![divider, header, provider_child, response_child, other_header], None);

    // "mdl" only fuzzy-matches the "Model & Provider" header.
    state.apply_search("mdl", true);

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
            "Model & Provider".to_owned(),
            "Provider".to_owned(),
            "Response Behavior".to_owned()
        ]
    );
}
#[test]
fn fuzzy_search_matching_child_shows_header_but_not_unmatched_siblings() {
    let header = InlineListItem {
        search_value: Some("model & provider".to_owned()),
        ..base_item("Model & Provider")
    };
    let matching_child = InlineListItem {
        indent: 1,
        selection: Some(InlineListSelection::ConfigAction("settings:set:provider".to_owned())),
        search_value: Some("active provider".to_owned()),
        ..base_item("Provider")
    };
    let other_child = InlineListItem {
        indent: 1,
        selection: Some(InlineListSelection::ConfigAction("settings:set:response".to_owned())),
        search_value: Some("response behavior".to_owned()),
        ..base_item("Response Behavior")
    };

    let mut state = ModalListState::new(vec![header, matching_child, other_child], None);

    state.apply_search("active", true);

    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();
    assert_eq!(visible_titles, vec!["Model & Provider".to_owned(), "Provider".to_owned()]);
}
#[test]
fn exact_mode_unchanged_when_fuzzy_disabled() {
    let subsequence_match = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:model_provider".to_owned())),
        search_value: Some("model & provider".to_owned()),
        ..base_item("Model & Provider")
    };
    let substring_match = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:mp_grid".to_owned())),
        search_value: Some("mp grid".to_owned()),
        ..base_item("MP Grid")
    };

    let mut state = ModalListState::new(vec![subsequence_match, substring_match], None);
    state.apply_search("mdl", false);

    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();
    // Subsequence queries do not match without the fuzzy flag.
    assert!(visible_titles.is_empty());

    state.apply_search("grid", false);
    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();
    assert_eq!(visible_titles, vec!["MP Grid".to_owned()]);
}
#[test]
fn fuzzy_search_multi_term_requires_every_term_to_match() {
    let active_provider = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:set:provider".to_owned())),
        search_value: Some("active provider".to_owned()),
        ..base_item("Provider")
    };
    let response_behavior = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:set:response".to_owned())),
        search_value: Some("response behavior".to_owned()),
        ..base_item("Response Behavior")
    };

    let mut state = ModalListState::new(vec![active_provider, response_behavior], None);
    state.apply_search("act prv", true);

    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();
    // Every term must fuzzy-match: "act prv" covers only "active provider";
    // "response behavior" fails the "act" term and is excluded.
    assert_eq!(visible_titles, vec!["Provider".to_owned()]);
}
#[test]
fn fuzzy_search_preserves_original_order_for_equal_scores() {
    let first = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:first".to_owned())),
        search_value: Some("group settings".to_owned()),
        ..base_item("First")
    };
    let second = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:second".to_owned())),
        search_value: Some("group settings".to_owned()),
        ..base_item("Second")
    };

    let mut state = ModalListState::new(vec![first, second], None);
    state.apply_search("grp", true);

    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();
    // Identical search values score identically; the stable sort must keep
    // the curated order instead of shuffling equally relevant items.
    assert_eq!(visible_titles, vec!["First".to_owned(), "Second".to_owned()]);

    // Clearing the query in fuzzy mode restores every item.
    state.apply_search("", true);
    assert_eq!(state.visible_indices.len(), state.items.len());
}
#[test]
fn fuzzy_search_attaches_divider_to_matching_singleton() {
    let unmatched = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:alpha".to_owned())),
        search_value: Some("alpha".to_owned()),
        ..base_item("Alpha")
    };
    let divider = InlineListItem {
        title: ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(3),
        ..base_item("")
    };
    let matched = InlineListItem {
        selection: Some(InlineListSelection::ConfigAction("settings:open:beta".to_owned())),
        search_value: Some("beta".to_owned()),
        ..base_item("Beta")
    };

    let mut state = ModalListState::new(vec![unmatched, divider, matched], None);
    state.apply_search("beta", true);

    let visible_titles: Vec<String> = state
        .visible_indices
        .iter()
        .map(|&idx| state.items[idx].title.clone())
        .collect();
    assert_eq!(visible_titles, vec![ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(3), "Beta".to_owned()]);
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
    assert_eq!(segments[1].style, Style::default().add_modifier(Modifier::BOLD));
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
        other => panic!("expected redraw, got {other:?}"),
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
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Selection(
            selection,
        )))) => {
            assert_eq!(selection, InlineListSelection::Model(0));
        }
        other => panic!("unexpected result: {other:?}"),
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
        other => panic!("unexpected selection result: {other:?}"),
    }

    let submit = modal.handle_list_mouse_click(1);
    match submit {
        ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Selection(
            selection,
        )))) => {
            assert_eq!(selection, InlineListSelection::Model(1));
        }
        other => panic!("unexpected submit result: {other:?}"),
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
                title: "Permission default".to_owned(),
                subtitle: Some("permissions.default = ask".to_owned()),
                badge: Some("Toggle".to_owned()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction("permissions.default:cycle".to_owned())),
                search_value: None,
                ..Default::default()
            }],
            None,
        )),
        secure_prompt: None,
        restore_input: true,
        restore_cursor: true,
        search: None,
        is_help_modal: false,
        status: None,
    };

    let key = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    let result = modal.handle_list_key_event(&key, ModalKeyModifiers::default());

    assert!(matches!(result, ModalListKeyResult::NotHandled));
}
#[test]
fn list_modal_alt_d_is_swallowed() {
    let mut modal = ModalState {
        title: "Config".to_owned(),
        lines: vec![],
        footer_hint: None,
        hotkeys: Vec::new(),
        list: Some(ModalListState::new(
            vec![InlineListItem {
                title: "Permission default".to_owned(),
                subtitle: Some("permissions.default = ask".to_owned()),
                badge: Some("Toggle".to_owned()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction("permissions.default:cycle".to_owned())),
                search_value: None,
                ..Default::default()
            }],
            None,
        )),
        secure_prompt: None,
        restore_input: true,
        restore_cursor: true,
        search: None,
        is_help_modal: false,
        status: None,
    };

    let result = modal.handle_list_key_event(
        &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
        ModalKeyModifiers { alt: true, ..ModalKeyModifiers::default() },
    );

    assert!(matches!(result, ModalListKeyResult::HandledNoRedraw));
}
#[test]
fn density_behavior_still_drives_summary_hints() {
    // Row spacing is now universal, but `density_behavior` still decides the
    // summary line: config lists always show the shared navigation hint,
    // other lists only echo an explicit footer.
    let config_list = ModalListState::new(
        vec![InlineListItem {
            title: "Permission default".to_owned(),
            subtitle: Some("permissions.default = ask".to_owned()),
            badge: Some("Toggle".to_owned()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction("permissions.default:cycle".to_owned())),
            search_value: None,
            ..Default::default()
        }],
        None,
    );
    let model_list = ModalListState::new(
        vec![InlineListItem {
            title: "gpt-5".to_owned(),
            subtitle: Some("High-quality general reasoning model".to_owned()),
            badge: Some("Default".to_owned()),
            indent: 0,
            selection: Some(InlineListSelection::Model(0)),
            search_value: Some("gpt-5".to_owned()),
            ..Default::default()
        }],
        None,
    );

    assert_eq!(config_list.non_filter_summary_text(None), Some(crate::design::keys::list_hint()));
    assert_eq!(model_list.non_filter_summary_text(None), None);
    assert_eq!(model_list.non_filter_summary_text(Some("Esc close")), Some("Esc close".to_string()));
}
