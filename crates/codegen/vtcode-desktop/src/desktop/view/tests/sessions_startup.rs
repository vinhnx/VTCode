use super::*;

#[gpui_kit::test]
fn inactive_agent_events_post_system_notices_and_click_reopens_the_window(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, TurnFailedEvent, Usage, VersionedThreadEvent};

    let completed_turn = |turn_id: &str| {
        BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
            session_id: "session".into(),
            turn_id: Some(turn_id.into()),
            event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage {
                    input_tokens: 0,
                    cached_input_tokens: 0,
                    cache_creation_tokens: 0,
                    output_tokens: 0,
                },
                in_progress_exec_sessions: Vec::new(),
            })),
        })
    };
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        assert!(!window.is_window_active());
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.active_turn_id = Some("turn-1".into());
            view.running = true;
            view.event_tx.send(completed_turn("turn-1")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(!view.running);
        });
        window.render_frame(cx);
        assert!(window.notifications(cx).is_empty(), "background completion is system-only");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx.send(completed_turn("stale-turn")).unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
    })
    .unwrap();
    assert_eq!(cx.shown_system_notifications().len(), 1, "idle stale events do not post system notices");

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.active_turn_id = Some("turn-2".into());
            view.running = true;
            view.event_tx
                .send(BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "session".into(),
                    turn_id: Some("turn-2".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
                        completed_at: None,
                        message: "provider detail that should not appear in a system notification".into(),
                        usage: None,
                    })),
                }))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);

            view.active_turn_id = Some("turn-3".into());
            view.running = true;
            view.event_tx
                .send(BackendEvent::Interaction(vtcode_desktop_protocol::InteractionRequest {
                    session_id: "session".into(),
                    turn_id: "turn-3".into(),
                    interaction_id: "interaction-1".into(),
                    kind: vtcode_desktop_protocol::InteractionKind::Question,
                    wizard: false,
                    title: "private prompt detail".into(),
                    details: Vec::new(),
                    choices: Vec::new(),
                    steps: Vec::new(),
                    selected_choice_id: None,
                    selected_choice_ids: Vec::new(),
                }))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.interaction.is_some());
        });
        window.render_frame(cx);
        assert!(window.notifications(cx).is_empty(), "background notices do not duplicate an in-app toast");
    })
    .unwrap();

    let notifications = cx.delivered_system_notifications();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].title, "VT Code");
    assert_eq!(notifications[0].body, "An agent run is waiting for your input.");
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 3);
    assert_eq!(shown[0].body, "Your agent run has finished.");
    assert_eq!(shown[1].body, "Your agent run needs attention.");
    assert_eq!(shown[2].body, "An agent run is waiting for your input.");
    assert!(
        shown
            .iter()
            .all(|notice| { !notice.body.contains("provider detail") && !notice.body.contains("private prompt") })
    );

    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag: notifications[0].tag.clone(),
        action_id: None,
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, _| {
        assert!(window.is_window_active(), "the system notification activates its owning window");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.active_turn_id = Some("turn-4".into());
            view.running = true;
            view.event_tx.send(completed_turn("turn-4")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(!view.running);
        });
    })
    .unwrap();
    assert_eq!(cx.shown_system_notifications().len(), 3, "active runs do not post system notices");
}

#[gpui_kit::test]
fn native_session_menu_uses_live_action_availability_and_existing_requests(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("menu-session".into());
            view.active_turn_id = Some("menu-turn".into());
            view.running = true;
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.is_action_available(menu_action(cx, "File", "New Session").as_ref(), cx));
        assert!(window.is_action_available(menu_action(cx, "File", "Close Session").as_ref(), cx));
        assert!(window.is_action_available(menu_action(cx, "View", "Cancel Run").as_ref(), cx));
        window.dispatch_action(menu_action(cx, "View", "Cancel Run"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("cancel request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
    assert_eq!(params, json!({ "sessionId": "menu-session", "turnId": "menu-turn", "clearQueue": false }));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.running = false;
            view.active_turn_id = None;
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.is_action_available(menu_action(cx, "File", "New Session").as_ref(), cx));
        assert!(window.is_action_available(menu_action(cx, "File", "Close Session").as_ref(), cx));
        assert!(!window.is_action_available(menu_action(cx, "View", "Cancel Run").as_ref(), cx));
        window.dispatch_action(menu_action(cx, "File", "Close Session"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("close request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    assert_eq!(params, json!({ "sessionId": "menu-session" }));
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn native_edit_and_settings_menus_operate_on_the_focused_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-l", cx);
        window.input("Menu editing keeps this exact text", cx);
        window.dispatch_action(menu_action(cx, "Edit", "Select All"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(menu_action(cx, "Edit", "Cut"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Menu editing keeps this exact text"));
        window.dispatch_action(menu_action(cx, "Edit", "Undo"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "Menu editing keeps this exact text");
        window.dispatch_action(menu_action(cx, "VT Code", "Settings…"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert_eq!(window.find("settings-panel").label(), Some("Settings"));
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).current_route() != Route::Settings);
        window.press("cmd-,", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("settings-panel").label(), Some("Settings"));
    })
    .unwrap();
}

#[test]
fn workspace_picker_only_allows_one_directory() {
    let options = workspace_picker_options();
    assert!(!options.files);
    assert!(options.directories);
    assert!(!options.multiple);
    assert!(options.prompt.is_some());
}

#[gpui_kit::test]
fn workspace_picker_task_is_owned_by_view_and_clears_pending_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.choose_workspace(window, cx);
            assert!(view.workspace_picker_open);
            assert!(view.workspace_picker_task.is_some());

            // Repeated activation while the native picker is pending must not
            // start another continuation owned by the same view.
            view.choose_workspace(window, cx);
            assert!(view.workspace_picker_open);
        });
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());

    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories);
        None
    });
    cx.run_until_parked();

    assert!(!cx.did_prompt_for_paths(), "one picker response resolves the only pending prompt");
    cx.update_entity(&view, |view: &mut DesktopView, _| {
        assert!(!view.workspace_picker_open);
        assert!(view.workspace_picker_task.is_some(), "the view retains the completed task until replacement");
    });
}

#[gpui_kit::test]
fn reopening_preserves_the_existing_window_and_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this unsent draft", window, cx));
            view.selected_session_id = Some("existing-session".into());
        });
    })
    .unwrap();
    cx.update(|cx| {
        let reopened = super::super::open_or_activate_main_window(cx, None).unwrap();
        assert_eq!(reopened, window);
        assert_eq!(cx.windows().len(), 1);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep this unsent draft");
        assert_eq!(view.read(cx).selected_session_id.as_deref(), Some("existing-session"));
    });
}

#[gpui_kit::test]
fn reopening_activates_the_focused_window_when_multiple_are_open(cx: &mut TestAppContext) {
    let (first_window, _first_view) = open_desktop(cx);
    let (second_window, _second_view) = open_desktop(cx);
    assert_ne!(first_window, second_window);
    assert_eq!(cx.windows().len(), 2);

    cx.update_window(second_window, |_, window, _| window.activate_window())
        .unwrap();
    cx.update(|cx| {
        assert_eq!(cx.active_window(), Some(second_window));
        assert_eq!(super::super::open_or_activate_main_window(cx, None).unwrap(), second_window);
        assert_eq!(cx.windows().len(), 2, "reopening activates a window instead of duplicating it");
    });

    cx.update_window(first_window, |_, window, _| window.remove_window()).unwrap();
    cx.update(|cx| {
        assert_eq!(cx.windows(), vec![second_window]);
        assert!(first_window.update(cx, |_, _, _| ()).is_err(), "closed handles reject later updates");
        second_window.update(cx, |_, window, cx| window.render_frame(cx)).unwrap();
    });
}

#[gpui_kit::test]
fn event_drain_task_is_owned_by_the_desktop_view(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.start_event_drain(cx));
    })
    .unwrap();

    cx.update_entity(&view, |view: &mut DesktopView, _| {
        assert!(view.event_drain_task.is_some());
    });
}

#[gpui_kit::test]
fn event_drain_wakes_on_events_and_reschedules_full_batches(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.start_event_drain(cx);
            for index in 0..MAX_UI_EVENTS_PER_TICK + 3 {
                view.event_tx
                    .try_send(BackendEvent::TurnStatus(
                        json!({ "sessionId": "session", "status": format!("event-{index}") }),
                    ))
                    .unwrap();
            }
        });
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_entity(&view, |view: &mut DesktopView, _| {
        assert_eq!(view.status, format!("event-{}", MAX_UI_EVENTS_PER_TICK + 2));
    });
}

#[gpui_kit::test]
fn reopening_after_last_window_closes_creates_one_new_window(cx: &mut TestAppContext) {
    let (closed, view) = open_desktop(cx);
    drop(view);
    cx.update_window(closed, |_, window, _| window.remove_window()).unwrap();
    cx.update(|cx| {
        assert!(cx.windows().is_empty());
        let reopened = super::super::open_or_activate_main_window(cx, None).unwrap();
        assert_ne!(reopened, closed);
        assert_eq!(cx.windows(), vec![reopened]);
        assert_eq!(super::super::open_or_activate_main_window(cx, None).unwrap(), reopened);
        assert_eq!(cx.windows().len(), 1);
    });
}

#[gpui_kit::test]
fn native_close_uses_menu_bar_mode_only_when_enabled(cx: &mut TestAppContext) {
    let (initial, view) = open_desktop(cx);
    drop(view);
    cx.update_window(initial, |_, window, _| window.remove_window()).unwrap();
    let window = cx.update(|cx| {
        assert!(super::super::menu_bar::set_enabled(true, cx));
        super::super::open_or_activate_main_window(cx, None).unwrap()
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window, cx);
    assert!(!visual.simulate_close(), "enabled mode retains the window");
    cx.update(|cx| assert!(super::super::menu_bar::set_enabled(false, cx)));
    assert!(visual.simulate_close(), "disabled mode allows the window to close");
}

#[gpui_kit::test]
fn menu_bar_mode_closes_one_window_when_another_remains(cx: &mut TestAppContext) {
    let (first, _) = open_desktop(cx);
    let (second, _) = open_desktop(cx);
    cx.update(|cx| assert!(super::super::menu_bar::set_enabled(true, cx)));
    cx.update_window(first, |_, window, cx| super::super::menu_bar::close_or_hide(window, cx))
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(cx.windows(), vec![second]);
        assert!(super::super::menu_bar::enabled(cx));
    });
}

#[gpui_kit::test]
fn reopening_restores_translucent_sidebar_root(cx: &mut TestAppContext) {
    let (closed, view) = open_desktop(cx);
    cx.update_window(closed, |_, window, cx| {
        view.update(cx, |view, cx| view.set_translucent_sidebar(true, window, cx));
        window.remove_window();
    })
    .unwrap();
    drop(view);
    let reopened = cx.update(|cx| super::super::open_or_activate_main_window(cx, None).unwrap());
    cx.update_window(reopened, |_, window, cx| {
        let root = window.root::<gpui_kit::base::Root>().flatten().unwrap();
        assert!(root.update(cx, |root, _| root.style().background.is_some()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn picker_completion_rejects_switching_after_turn_starts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|window, cx| DesktopView::new(window, cx, ThemeMode::Light));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let original_workspace = PathBuf::from("/original-workspace");
            view.workspace = Some(original_workspace.clone());
            view.running = true;
            view.select_workspace(std::env::temp_dir(), cx);
            assert_eq!(view.workspace.as_ref(), Some(&original_workspace));
            assert!(view.backend.is_none());
            assert!(view.error.as_deref().unwrap().contains("Cancel the active turn"));
        });
    });
}

#[gpui_kit::test]
fn event_drain_yields_to_input_with_a_busy_producer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|window, cx| DesktopView::new(window, cx, ThemeMode::Light));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            for index in 0..MAX_UI_EVENTS_PER_TICK + 3 {
                view.event_tx
                    .try_send(BackendEvent::TurnStatus(
                        json!({ "sessionId": "session", "status": format!("event-{index}") }),
                    ))
                    .unwrap();
            }
            let _batch_full = view.drain_events(window, cx);
            assert_eq!(view.status, format!("event-{}", MAX_UI_EVENTS_PER_TICK - 1));
            let _batch_full = view.drain_events(window, cx);
            assert_eq!(view.status, format!("event-{}", MAX_UI_EVENTS_PER_TICK + 2));
        });
    });
}

#[gpui_kit::test]
fn send_button_keeps_draft_and_transcript_when_queue_is_full(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    commands.try_send(BackendCommand::Stop).unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-a".to_string());
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this draft", window, cx));
        });
        window.render_frame(cx);
        window.click("send-prompt", cx);
        let view = view.read(cx);
        assert!(view.transcript.is_empty());
        assert!(!view.running);
        assert_eq!(view.composer.read(cx).value(), "Keep this draft");
        assert!(view.error.as_deref().unwrap().contains("queue is full"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn send_button_submits_exact_draft_and_clears_it_after_enqueue(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-b".to_string());
            view.composer
                .update(cx, |state, cx| state.set_value("Review **these** changes", window, cx));
        });
        window.render_frame(cx);
        window.click("send-prompt", cx);
        let view = view.read(cx);
        assert!(view.running);
        assert_eq!(view.composer.read(cx).value(), "");
        assert_eq!(view.transcript.get(0).unwrap().text, "Review **these** changes");
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params, json!({ "sessionId": "session-b", "prompt": "Review **these** changes", "attachments": [] }));
}

#[gpui_kit::test]
fn attachments_ride_the_turn_request_as_removable_chips(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("notes.md"), "attached notes").expect("write");

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(workspace.path().to_path_buf());
            view.selected_session_id = Some("session-att".to_string());
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "notes.md".to_string(),
                text: "attached notes".to_string(),
            });
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "scratch.txt".to_string(),
                text: "drop me".to_string(),
            });
            view.attachments.reverse();
            view.composer
                .update(cx, |state, cx| state.set_value("Summarize the attachment", window, cx));
        });
        window.render_frame(cx);
        // Stable path IDs still target the same chip after the attachment order changes.
        window.click(opaque_path_element_id("remove-attachment", "scratch.txt"), cx);
        assert_eq!(view.read(cx).attachments.len(), 1);

        window.click("send-prompt", cx);
        let view = view.read(cx);
        assert!(view.attachments.is_empty(), "attachments clear after submit");
        assert!(view.transcript.get(0).unwrap().text.contains("Attached: notes.md"));
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["attachments"], json!([{ "path": "notes.md", "text": "attached notes" }]));
}

#[gpui_kit::test]
fn selecting_a_model_switches_configuration_and_preserves_the_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(std::env::temp_dir());
            view.selected_session_id = Some("session-old".to_string());
            view.open_tabs = vec!["session-old".to_string()];
            view.transcript.push(TranscriptItem::new(
                "kept-message".into(),
                "assistant".into(),
                "VT Code".into(),
                "Conversation remains visible.".into(),
            ));
            view.models = vec![
                vtcode_desktop_protocol::ModelInfo {
                    provider: "ollama".to_string(),
                    id: "gpt-oss:20b".to_string(),
                    display_name: "GPT OSS 20B".to_string(),
                    context_window: Some(131_072),
                    current: true,
                    recommended_rank: None,
                    capabilities: None,
                },
                vtcode_desktop_protocol::ModelInfo {
                    provider: "anthropic".to_string(),
                    id: "claude-sonnet-5".to_string(),
                    display_name: "Claude Sonnet 5".to_string(),
                    context_window: Some(200_000),
                    current: false,
                    recommended_rank: Some(0),
                    capabilities: None,
                },
            ];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        view.update(cx, |view, cx| view.run_palette_action(super::super::PaletteAction::ChooseModel, window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
        assert_eq!(
            view.read(cx)
                .palette
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_index()
                .unwrap()
                .section,
            0,
            "the active provider and model are placed first and selected"
        );
        let groups = &view.read(cx).palette_groups;
        assert_eq!(groups[0].1[0].label, "GPT OSS 20B");
        assert!(groups[0].1[0].description.as_ref().unwrap().contains("gpt-oss:20b"));
        assert_eq!(groups[1].1[0].label, "Claude Sonnet 5");
        window.input("claude-sonnet-5", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).matched_count(), 1);
        assert!(
            window
                .find(palette_action_id(&super::super::PaletteAction::SelectModel {
                    provider: "anthropic".into(),
                    model: "claude-sonnet-5".into(),
                }))
                .label()
                .unwrap()
                .contains("200K context")
        );
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).status, "Switching model…");
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected config select request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SELECT);
    assert_eq!(params, json!({ "provider": "anthropic", "model": "claude-sonnet-5" }));

    // The service answers with the new snapshot; the client keeps the thread
    // and refreshes the catalog.
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SELECT.to_string(),
                    result: Some(json!({
                        "provider": "anthropic",
                        "model": "claude-sonnet-5",
                        "authenticated": true,
                        "credentialSetupCommand": "vtcode login anthropic",
                        "sandbox": "Ask before protected actions",
                        "contextWindow": 200_000,
                    })),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.selected_session_id.as_deref(), Some("session-old"), "model switch preserves the thread");
            assert_eq!(view.open_tabs, vec!["session-old".to_string()], "tabs survive a model switch");
            assert_eq!(view.transcript.get(0).unwrap().text, "Conversation remains visible.");
            assert!(view.transcript.get(1).is_none(), "no transcript row is added or dropped");
            assert_eq!(
                view.configuration.as_ref().map(|configuration| configuration.model.clone()),
                Some("claude-sonnet-5".to_string())
            );
            assert_eq!(view.status, "Switched to claude-sonnet-5.");
        });
    })
    .unwrap();
    let BackendCommand::Request { method, params: _ } = requests.try_recv().unwrap() else {
        panic!("expected catalog refresh");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_MODELS);
}

#[gpui_kit::test]
fn enter_submits_the_draft_and_shift_enter_keeps_editing(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-c".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        // Type the draft so the caret sits after the text, like real input.
        window.input("Keyboard first", cx);

        // Shift+Enter inserts a newline and never submits.
        window.press("shift-enter", cx);
        {
            let view = view.read(cx);
            assert!(!view.running, "shift-enter must not submit the draft");
            assert_eq!(view.composer.read(cx).value(), "Keyboard first\n");
        }

        // Plain Enter submits through the propagated action.
        window.press("enter", cx);
        {
            let view = view.read(cx);
            assert!(view.running, "enter must submit the draft");
            assert_eq!(view.composer.read(cx).value(), "");
            assert_eq!(view.transcript.get(0).unwrap().text, "Keyboard first\n");
        }
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params, json!({ "sessionId": "session-c", "prompt": "Keyboard first\n", "attachments": [] }));
}

#[gpui_kit::test]
fn first_submit_names_the_session_and_close_deselects_it(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.sessions.insert(
                0,
                vtcode_desktop_protocol::SessionSummary {
                    session_id: "session-e".to_string(),
                    title: None,
                    workspace: std::env::temp_dir().display().to_string(),
                    completed: false,
                    resumable: true,
                    size_bytes: 0,
                    updated_at: None,
                },
            );
            view.selected_session_id = Some("session-e".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("Fix the login timeout\n\nand add a regression test", window, cx);
            });
        });
        window.render_frame(cx);
        window.click("send-prompt", cx);
        {
            let view = view.read(cx);
            assert_eq!(
                view.sessions[0].title.as_deref(),
                Some("Fix the login timeout"),
                "sidebar entry is named from the first prompt"
            );
        }

        // A later archive refresh without a persisted title keeps the local one.
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_LIST.to_string(),
                    result: Some(serde_json::json!([{
                        "sessionId": "session-e",
                        "title": None::<String>,
                        "workspace": std::env::temp_dir().display().to_string(),
                        "completed": false,
                        "resumable": true,
                    }])),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.sessions[0].title.as_deref(), Some("Fix the login timeout"));
        });

        // Close is gated on the turn ending, like the button state.
        view.update(cx, |view, _| {
            view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
                session_id: "session-e".to_string(),
                turn_id: Some("turn-9".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::TurnCompleted(
                    vtcode_exec_events::TurnCompletedEvent {
                        completed_at: None,
                        usage: vtcode_exec_events::Usage {
                            input_tokens: 0,
                            cached_input_tokens: 0,
                            cache_creation_tokens: 0,
                            output_tokens: 0,
                        },
                        in_progress_exec_sessions: Vec::new(),
                    },
                )),
            });
            assert!(!view.running, "turn completion re-enables session controls");
            assert!(view.sessions[0].completed, "the selected session row reflects completion locally");
        });

        window.click("close-session", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params: _ } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected close request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    assert_eq!(params, json!({ "sessionId": "session-e" }));
}

#[gpui_kit::test]
fn malformed_catalog_responses_preserve_the_last_valid_sessions_and_models(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session = vtcode_desktop_protocol::SessionSummary {
        session_id: "saved-session".to_string(),
        title: Some("Keep this session".to_string()),
        workspace: std::env::temp_dir().display().to_string(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    };
    let model = vtcode_desktop_protocol::ModelInfo {
        provider: "openai".to_string(),
        id: "gpt-test".to_string(),
        display_name: "Test model".to_string(),
        context_window: None,
        current: true,
        recommended_rank: None,
        capabilities: None,
    };

    view.update(cx, |view, _| {
        view.sessions = vec![session.clone()];
        view.models = vec![model.clone()];
    });

    for result in [
        json!({ "unexpected": "object" }),
        json!([{
            "sessionId": "",
            "title": "Invalid row",
            "workspace": std::env::temp_dir().display().to_string(),
            "completed": false,
            "resumable": true,
        }]),
    ] {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_LIST.to_string(),
                    result: Some(result),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.sessions.as_slice(), std::slice::from_ref(&session));
            assert_eq!(view.error.as_deref(), Some("The local service returned an invalid session list."));
        });
    }

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::CONFIG_MODELS.to_string(),
                result: Some(json!({ "unexpected": "object" })),
                error: None,
            },
            cx,
        );
        assert_eq!(view.models.as_slice(), std::slice::from_ref(&model));
        assert_eq!(view.error.as_deref(), Some("The local service returned an invalid model catalog."));
    });
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
}

#[gpui_kit::test]
fn window_title_follows_the_selected_thread_title(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let summary = |session_id: &str, title: Option<&str>| vtcode_desktop_protocol::SessionSummary {
        session_id: session_id.into(),
        title: title.map(str::to_string),
        workspace: std::env::temp_dir().display().to_string(),
        completed: false,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    };
    view.update(cx, |view, _| {
        assert_eq!(view.window_title(), "VT Code");
        view.sessions = vec![
            summary("titled", Some("Fix the login timeout")),
            summary("untitled", None),
        ];
        view.selected_session_id = Some("titled".into());
        assert_eq!(view.window_title(), "Fix the login timeout");
        view.selected_session_id = Some("untitled".into());
        assert_eq!(view.window_title(), "VT Code");
        view.sessions[1].title = Some("   ".into());
        assert_eq!(view.window_title(), "VT Code", "blank titles fall back to the app name");
        view.selected_session_id = Some("missing".into());
        assert_eq!(view.window_title(), "VT Code");
    });
}
