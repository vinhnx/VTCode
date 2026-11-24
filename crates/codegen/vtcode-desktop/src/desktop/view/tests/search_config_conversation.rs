//! Conversation-search coverage split out of `search_config.rs` to keep that
//! module under the repository's 1000-line ceiling.
use super::*;

#[gpui_kit::test]
fn conversation_search_filters_loaded_messages_and_jumps_to_the_selected_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "search-thread".to_string();
    let target_id = "message-08".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        for index in 0..40 {
            let id = format!("message-{index:02}");
            let (role, text) = if index == 8 {
                ("user", "Needle phrase in a user message")
            } else if index == 9 {
                ("tool", "A separate tool activity row")
            } else {
                ("assistant", "An ordinary conversation message")
            };
            view.transcript
                .push(TranscriptItem::new(id, role.into(), "VT Code".into(), text.into()));
        }
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        let open_search = super::super::PaletteAction::OpenConversationSearch { session_id: session_id.clone() };
        window.within("command-palette").input("Find in conversation", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert!(window.try_find(palette_action_id(&open_search)).is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Find in conversation"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        let target_action = super::super::PaletteAction::JumpToTranscriptItem {
            session_id: session_id.clone(),
            item_id: target_id.clone(),
            from_search: true,
        };
        window.within("command-palette").input("Needle phrase", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 1, "unmatched tool activity does not pollute the message results");
        assert!(window.try_find(palette_action_id(&target_action)).is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert_eq!(view.read(cx).message_scroller.read(cx).logical_scroll_top().item_ix, 8);
        assert!(window.try_find(format!("transcript-row-{target_id}")).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_includes_tool_activity_and_reveals_the_matching_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "tool-search-thread".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        for index in 0..40 {
            view.transcript.push(TranscriptItem::new(
                format!("search-context-{index}"),
                "assistant".into(),
                "VT Code".into(),
                "Intermediate conversation message".into(),
            ));
        }
        view.transcript.push(TranscriptItem::new(
            "tool-before".into(),
            "tool".into(),
            "Read file".into(),
            "completed: read notes.md".into(),
        ));
        view.transcript.push(TranscriptItem::new(
            "tool-target".into(),
            "tool".into(),
            "Run command".into(),
            "completed: archived output needle-marker".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("needle-marker", cx);
        window.render_frame(cx);

        let target_action = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| entry.label.contains("needle-marker"))
            .unwrap()
            .action
            .clone();
        assert!(matches!(target_action, super::super::PaletteAction::JumpToTranscriptItem { .. }));
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).matched_count(), 1);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert!(view_state.palette.is_none());
        assert!(view_state.transcript.get(40).unwrap().activity_expanded);
        assert!(view_state.transcript.get(41).unwrap().activity_expanded);
        assert!(view_state.transcript.get(41).unwrap().expanded);
        let scroll_top = view_state.message_scroller.read(cx).logical_scroll_top().item_ix;
        assert!(scroll_top > 0 && scroll_top <= 41, "tool result should move into the viewport: {scroll_top}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_finds_restored_tool_output_and_opens_it_in_the_inspector(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "output-search-thread".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        let mut item = TranscriptItem::new(
            "archived-command".into(),
            "tool".into(),
            "Run command".into(),
            "completed\n\ncargo check".into(),
        );
        item.output = Some("archived-only-output-marker\ncompiler finished".into());
        view.push_restored_activity(item);
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("archived-only-output-marker", cx);
        window.render_frame(cx);

        let search = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(search.read(cx).matched_count(), 1);
        let target_action = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| entry.label.contains("archived-only-output-marker"))
            .unwrap()
            .action
            .clone();
        assert!(matches!(target_action, super::super::PaletteAction::ShowActivityOutput { .. }));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert!(view_state.palette.is_none());
        assert!(view_state.inspector_open);
        assert_eq!(view_state.inspector_tab, super::super::inspector::InspectorTab::Output);
        assert_eq!(view_state.selected_output_id.as_deref(), Some("archived-command"));
        let output = super::super::inspector::output_for(
            &view_state.transcript,
            &view_state.restored_activity,
            view_state.selected_output_id.as_deref(),
        )
        .unwrap();
        assert!(output.text.contains("archived-only-output-marker"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_keeps_the_newest_capped_rows_in_each_group(cx: &mut TestAppContext) {
    use super::super::MAX_CONVERSATION_SEARCH_ROWS;

    let (window, view) = open_desktop(cx);
    let session_id = "capped-search-thread".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        // Transcript outputs are newer than restored history; both groups
        // must read newest-first so the cap keeps the freshest rows. Tools go
        // first so the message cap drops them as the oldest rows.
        for (id, output) in [("out-old", "older output marker"), ("out-new", "newer output marker")] {
            let mut item = TranscriptItem::new(id.into(), "tool".into(), "Run command".into(), "completed".into());
            item.output = Some(output.into());
            view.transcript.push(item);
        }
        for index in 0..MAX_CONVERSATION_SEARCH_ROWS + 50 {
            view.transcript.push(TranscriptItem::new(
                format!("message-{index:03}"),
                "user".into(),
                "You".into(),
                format!("searchable message number {index}"),
            ));
        }
        for (id, output) in [
            ("restored-old", "restored older marker"),
            ("restored-new", "restored newer marker"),
        ] {
            let mut item = TranscriptItem::new(id.into(), "tool".into(), "Run command".into(), "completed".into());
            item.output = Some(output.into());
            view.push_restored_activity(item);
        }
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);

        let groups = view.read(cx).palette_groups.clone();
        assert_eq!(groups.len(), 2, "messages and outputs both stay populated");

        let message_ids: Vec<String> = groups[0]
            .1
            .iter()
            .filter_map(|entry| match &entry.action {
                super::super::PaletteAction::JumpToTranscriptItem { item_id, .. } => Some(item_id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(message_ids.len(), MAX_CONVERSATION_SEARCH_ROWS);
        assert_eq!(message_ids.first().unwrap(), "message-249", "newest message first");
        assert_eq!(message_ids.last().unwrap(), "message-050", "cap drops the oldest");
        assert!(!message_ids.contains(&"message-000".to_string()));

        let output_ids: Vec<String> = groups[1]
            .1
            .iter()
            .filter_map(|entry| match &entry.action {
                super::super::PaletteAction::ShowActivityOutput { item_id, .. } => Some(item_id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(output_ids, vec!["out-new", "out-old", "restored-new", "restored-old"]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_loads_earlier_history_and_keeps_the_search_open(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{SessionHistoryMessage, SessionHistoryPage};

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    let session_id = "search-archive-thread".to_string();
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some(session_id.clone());
        view.history_start_index = 2;
        view.history_has_earlier = true;
        view.transcript.push(TranscriptItem::new(
            "current-message".into(),
            "assistant".into(),
            "VT Code".into(),
            "Current page only".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("conversation-search-load-earlier").role(), Some(gpui_kit::Role::Button));
        window.click("conversation-search-load-earlier", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "loading older messages keeps search open");
        assert!(view.read(cx).history_page_loading);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("earlier history request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_HISTORY);
    assert_eq!(params["sessionId"], session_id);
    assert_eq!(params["beforeIndex"], 2);
    let request_id = params["requestId"].as_str().unwrap().to_string();
    let page = SessionHistoryPage {
        session_id: session_id.clone(),
        request_id,
        start_index: 1,
        messages: vec![SessionHistoryMessage {
            event_item_id: None,
            phase: None,
            hidden: false,
            archive_index: Some(1),
            role: "tool".into(),
            text: "Archived tool entry contains the needle".into(),
        }],
        has_earlier: true,
    };
    view.update(cx, |view, cx| view.begin_session_history_page(json!(page), cx));
    cx.run_until_parked();

    let mut target_row_id = None;
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "loading an archive page keeps the search palette open");
        assert_eq!(view.read(cx).history_start_index, 1);
        assert!(window.try_find("conversation-search-load-earlier").is_some());
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("needle", cx);
        window.render_frame(cx);

        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        let target_action = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| {
                entry.label.contains("Archived tool entry")
                    && matches!(&entry.action, super::super::PaletteAction::JumpToTranscriptItem { .. })
            })
            .unwrap()
            .action
            .clone();
        let super::super::PaletteAction::JumpToTranscriptItem { item_id, .. } = &target_action else {
            panic!("message result action")
        };
        target_row_id = Some(format!("transcript-row-{item_id}"));
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert!(window.try_find(palette_action_id(&target_action)).is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    let target_row_id = target_row_id.unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert!(view_state.palette.is_none());
        assert_eq!(view_state.message_scroller.read(cx).logical_scroll_top().item_ix, 0);
        assert!(window.try_find(target_row_id.clone()).is_some());
        let tool = view_state.transcript.get(0).unwrap();
        assert!(tool.expanded);
        assert!(window.try_find(format!("tool-output-{}", tool.id)).is_none());
        let output_button = format!("tool-open-output-{}", tool.id);
        assert!(window.try_find(output_button.clone()).is_some());
        window.click(output_button, cx);
        window.render_frame(cx);
        assert_eq!(window.find("output-body").label(), Some("Archived tool entry contains the needle"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_shortcut_opens_the_loaded_message_palette(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("search-shortcut-thread".into());
        view.transcript.push(TranscriptItem::new(
            "message-shortcut".into(),
            "user".into(),
            "You".into(),
            "Searchable message".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-shift-f", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Find in conversation"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette_focus.contains_focused(window, cx));
    })
    .unwrap();
}

#[test]
fn conversation_search_snippet_is_single_line_and_bounded() {
    assert_eq!(super::super::transcript_search_snippet("\n  First line\nsecond line"), "First line");
    assert_eq!(super::super::transcript_search_snippet(&"x".repeat(98)), format!("{}…", "x".repeat(96)));
}
