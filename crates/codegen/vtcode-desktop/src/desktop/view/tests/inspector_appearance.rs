use super::*;

#[gpui_kit::test]
fn interaction_acknowledgements_preserve_retry_drafts_and_cannot_dismiss_new_questions(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;
    use vtcode_desktop_protocol::{InteractionChoice, InteractionKind, InteractionRequest, InteractionStep};

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let question = |id: &str| {
        BackendEvent::Interaction(InteractionRequest {
            session_id: "session".into(),
            turn_id: "session-start".into(),
            interaction_id: id.into(),
            kind: InteractionKind::Question,
            wizard: false,
            title: "Choose an approach".into(),
            details: Vec::new(),
            choices: Vec::new(),
            selected_choice_id: None,
            selected_choice_ids: Vec::new(),
            steps: vec![InteractionStep {
                title: "Approach".into(),
                question: "How should we proceed?".into(),
                allow_freeform: true,
                choices: vec![InteractionChoice {
                    id: "accept".into(),
                    label: "Proceed".into(),
                    description: None,
                    recommended: false,
                }],
                selected_choice_id: None,
            }],
        })
    };
    let reply = |id: &str, cancelled, error: Option<&str>| BackendEvent::InteractionResponded {
        identity: InteractionResponseIdentity {
            session_id: "session".into(),
            turn_id: "session-start".into(),
            interaction_id: id.into(),
            cancelled,
        },
        error: error.map(str::to_owned),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.event_tx.send(question("question-a")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            view.question_answer.update(cx, |input, cx| {
                input.set_value("x".repeat(vtcode_desktop_protocol::MAX_FRAME_BYTES), window, cx)
            });
        });
        window.render_frame(cx);
        window.click("interaction-question-choice-0", cx);
        window.click("continue-question", cx);
        assert!(requests.try_recv().is_err(), "oversized answers stay local and retryable");
        assert!(view.read(cx).error.as_deref().unwrap().contains("request limit"));
        view.update(cx, |view, cx| {
            view.question_answer
                .update(cx, |input, cx| input.set_value("Keep this answer", window, cx));
        });
        window.click("continue-question", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_ok());
        assert!(window.find("interaction-response-status").visible());
        assert_eq!(window.find("continue-question").disabled(), Some(true));
        assert_eq!(window.find("interaction-question-choice-0").disabled(), Some(true));
        assert_eq!(window.find("cancel-interaction").disabled(), Some(true));
        view.update(cx, |view, cx| {
            view.submit_interaction(vec!["accept".into()], false, window, cx);
            view.event_tx.send(question("question-a")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert_eq!(
                view.question_answer.read(cx).value(),
                "Keep this answer",
                "duplicate notifications preserve drafts"
            );
            view.event_tx
                .send(reply("question-a", false, Some("Runtime is temporarily busy")))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
        assert!(requests.try_recv().is_err(), "pending decisions cannot be submitted twice");
        window.render_frame(cx);
        assert_ne!(window.find("continue-question").disabled(), Some(true));
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(true));
        assert_eq!(view.read(cx).question_answer.read(cx).value(), "Keep this answer");
        window.click("continue-question", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().expect("retry response") else {
            panic!("request")
        };
        assert_eq!(params["interactionId"], "question-a");
        assert_eq!(params["answerTexts"], json!(["Keep this answer"]));

        view.update(cx, |view, cx| {
            view.event_tx.send(question("question-b")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(
                view.question_answer.read(cx).value().is_empty(),
                "replacement questions never inherit private answers"
            );
            view.question_answer
                .update(cx, |input, cx| input.set_value("New answer", window, cx));
        });
        window.render_frame(cx);
        window.click("interaction-question-choice-0", cx);
        window.click("continue-question", cx);
        assert!(requests.try_recv().is_ok());
        view.update(cx, |view, cx| {
            view.event_tx.send(reply("question-a", false, None)).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert_eq!(view.pending_interaction_response.as_deref(), Some("question-b"));
            assert_eq!(view.question_answer.read(cx).value(), "New answer");
            view.focus_handle.focus(window, cx);
            view.event_tx.send(reply("question-b", false, None)).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.interaction.is_none());
            assert!(view.pending_interaction_response.is_none());
            assert!(view.question_answer.read(cx).value().is_empty());
            assert_eq!(view.status, "Decision accepted");
            assert!(view.focus_handle.is_focused(window), "acceptance cannot steal focus from another area");
            view.event_tx.send(question("question-c")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            view.question_answer
                .update(cx, |input, cx| input.set_value("Private unsubmitted draft", window, cx));
        });
        window.render_frame(cx);
        window.click("cancel-interaction", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().expect("cancel response") else {
            panic!("request")
        };
        assert_eq!(params["cancelled"], true);
        assert_eq!(params["answerTexts"], json!([]));
        assert!(params["text"].is_null(), "cancellation never transmits the private answer draft");
        view.update(cx, |view, cx| {
            view.interaction_focus.focus(window, cx);
            view.event_tx.send(reply("question-c", true, None)).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.interaction.is_none());
            assert_eq!(view.status, "Interaction cancelled");
            assert!(view.composer.read(cx).focus_handle(cx).is_focused(window));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn rejected_draft_survives_same_session_reload_and_blocks_context_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let other_workspace = tempfile::TempDir::new().unwrap();
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let summary = json!({ "sessionId": "session", "title": "Current", "workspace": "/workspace", "completed": false, "resumable": true });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.sessions = vec![serde_json::from_value(summary.clone()).unwrap(), serde_json::from_value(json!({
                "sessionId": "other", "title": "Other", "workspace": "/workspace", "completed": true, "resumable": true,
            })).unwrap()];
            view.composer
                .update(cx, |state, cx| state.set_value("Recover this request", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "source.rs".into(),
                text: "original source".into(),
            });
            view.submit_prompt(window, cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: None,
                    error: Some("selected session runtime has stopped; reload the session to continue".into()),
                },
                cx,
            );
        });
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("submission")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        window.render_frame(cx);
        assert_eq!(window.find("new-session").disabled(), Some(true));
        assert_eq!(window.find("close-session").disabled(), Some(true));
        assert_eq!(window.find("session-other").disabled(), Some(true));
        view.update(cx, |view, cx| {
            let workspace_before = view.workspace.clone();
            view.create_session(cx);
            view.close_session(cx);
            let _ = view.load_session("other".into(), cx);
            view.select_model("other-provider".into(), "other-model".into(), cx);
            view.select_workspace(other_workspace.path().to_path_buf(), cx);
            assert_eq!(view.workspace, workspace_before);
            assert!(view.session_request.is_none());
            assert!(view.retained_submission.is_some());
        });
        assert!(requests.try_recv().is_err(), "guards protect keyboard and palette paths too");
        window.render_frame(cx);
        window.click("session-session", cx);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("reload")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
        assert_eq!(params, json!({ "sessionId": "session" }));
        window.render_frame(cx);
        assert_eq!(window.find("restore-submission").disabled(), Some(true));
        view.update(cx, |view, cx| {
            view.restore_submission(window, cx);
            assert!(view.composer.read(cx).value().is_empty());
            assert!(view.attachments.is_empty());
            let draft = view.retained_submission.as_ref().unwrap();
            assert_eq!(draft.prompt, "Recover this request");
            assert_eq!(draft.attachments[0].text, "original source");
        });
    })
    .unwrap();
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(
                    json!({ "session": summary, "messages": [{ "role": "assistant", "text": "Restored history" }] }),
                ),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).transcript.get(0).unwrap().text, "Restored history");
        assert!(view.read(cx).retained_submission.is_some());
        window.render_frame(cx);
        window.click("restore-submission", cx);
        let view = view.read(cx);
        assert_eq!(view.composer.read(cx).value(), "Recover this request");
        assert_eq!(view.attachments.len(), 1);
        assert_eq!(view.attachments[0].path, "source.rs");
        assert_eq!(view.attachments[0].text, "original source");
        assert!(view.retained_submission.is_none());
        assert!(view.session_change_allowed(None));
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "reload and restoration never auto-resend");
}

#[gpui_kit::test]
fn initialization_gates_commands_until_the_negotiated_protocol_is_validated(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.initialize_backend(BackendHandle { commands });
            view.composer.update(cx, |state, cx| state.set_value("Wait for connection", window, cx));
            view.submit_prompt(window, cx);
            view.create_session(cx);
            assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::INITIALIZE));
            assert_eq!(view.composer.read(cx).value(), "Wait for connection");
            assert!(!view.running);
            assert!(view.service_info.is_none());
            let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else { panic!("initialize") };
            assert_eq!(method, vtcode_desktop_protocol::method::INITIALIZE);
            assert_eq!(params["protocol"]["minimum"], json!(vtcode_desktop_protocol::PROTOCOL_VERSION));
            assert!(requests.try_recv().is_err(), "no discovery or user commands before the handshake");
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::INITIALIZE.into(),
                result: Some(json!({ "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION, "workspace": "/workspace", "serverVersion": "test" })),
                error: None,
            }, cx);
            assert!(view.service_info.is_some());
            assert!(view.session_request.is_none());
            assert!(view.session_change_idle());
            for expected in [
                vtcode_desktop_protocol::method::CONFIG_DISCOVER,
                vtcode_desktop_protocol::method::CONFIG_SETTINGS_READ,
                vtcode_desktop_protocol::method::CONFIG_MODELS,
                vtcode_desktop_protocol::method::SESSION_LIST,
                vtcode_desktop_protocol::method::WEBMCP_STATUS,
                vtcode_desktop_protocol::method::UPDATE_STATUS,
            ] {
                let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else { panic!("discovery") };
                assert_eq!(method, expected);
            }
            assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            assert!(view.update_request_pending);
            assert!(requests.try_recv().is_err());
        });
    }).unwrap();
}

#[gpui_kit::test]
fn rejected_or_malformed_initialization_disconnects_without_discovery(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for (result, error, expected_error) in [
        (None, Some("unsupported protocol version".to_string()), "unsupported protocol version"),
        (Some(json!({})), None, "invalid or incompatible initialization"),
        (
            Some(
                json!({ "protocol": { "major": 99, "minor": 0 }, "workspace": "/workspace", "serverVersion": "test" }),
            ),
            None,
            "invalid or incompatible initialization",
        ),
    ] {
        let (commands, mut requests) = tokio::sync::mpsc::channel(4);
        cx.update_window(window, |_, _, cx| {
            view.update(cx, |view, cx| {
                view.initialize_backend(BackendHandle { commands });
                let _initialize = requests.try_recv().unwrap();
                view.handle_backend_event(
                    BackendEvent::Response {
                        method: vtcode_desktop_protocol::method::INITIALIZE.into(),
                        result,
                        error,
                    },
                    cx,
                );
                assert!(view.backend.is_none());
                assert!(view.session_request.is_none());
                assert!(view.service_info.is_none());
                assert!(view.error.as_deref().unwrap().contains(expected_error));
                assert!(requests.try_recv().is_err(), "failed initialization must not enqueue discovery");
                view.handle_backend_event(BackendEvent::Exited { status: Some(0), authentication_required: false }, cx);
                assert!(
                    view.error.as_deref().unwrap().contains(expected_error),
                    "shutdown preserves the handshake error"
                );
            });
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn palette_filters_unavailable_commands_as_session_state_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_command_palette(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.input("Close session", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 0, "no selected session to close");
        assert!(palette.read(cx).selected_index().is_none());
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("selected-session".into());
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(palette.read(cx).matched_count(), 1, "the same query becomes actionable after selection");
        assert_eq!(
            window
                .find(palette_action_id(&super::super::PaletteAction::CloseSession))
                .label(),
            Some("Close session")
        );
        view.update(cx, |view, cx| {
            view.running = true;
            view.run_palette_action(super::super::PaletteAction::CloseSession, window, cx);
            assert!(view.palette.is_some(), "deferred confirmation must recheck availability");
        });
        window.render_frame(cx);
        assert_eq!(palette.read(cx).matched_count(), 0, "active work prevents session changes");
        assert!(palette.read(cx).selected_index().is_none());
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "an unavailable confirmation cannot send a session-close request");
}

#[gpui_kit::test]
fn inspector_tabs_expose_selection_and_keyboard_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 900.);
    // Narrow the rail so the scroll-reveal assertions below hold by
    // construction; the default width fits every tab (see the fit test).
    view.update(cx, |view, _| view.inspector_width = 240.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.within("inspector-tabs").find(0usize).role(), Some(gpui_kit::Role::Tab));
        assert_eq!(window.within("inspector-tabs").find(0usize).selected(), Some(true));
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);
        assert_eq!(window.within("inspector-tabs").find(1usize).selected(), Some(true));
        for (key, expected) in [
            ("right", crate::desktop::inspector::InspectorTab::Activity),
            ("end", crate::desktop::inspector::InspectorTab::Utilities),
            ("home", crate::desktop::inspector::InspectorTab::Changes),
            ("left", crate::desktop::inspector::InspectorTab::Utilities),
            ("right", crate::desktop::inspector::InspectorTab::Changes),
        ] {
            window.press(key, cx);
            window.render_frame(cx);
            assert_eq!(view.read(cx).inspector_tab, expected, "{key}");
            if key == "end" || key == "left" {
                let scroll = &view.read(cx).inspector_tabs_scroll;
                let visible = horizontal_scroll_item_is_visible(scroll, 4);
                let offset = scroll.offset().x;
                assert!(visible, "keyboard selection reveals the final inspector tab; scroll offset={offset:?}");
                assert!(offset < px(0.), "narrow inspector tab row must scroll to its final tab");
            }
        }
        window.press("cmd-l", cx);
        window.input("composer text", cx);
        window.press("left", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        assert_eq!(view.read(cx).composer.read(cx).value(), "composer text");
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_tabs_fit_without_scrolling_at_default_width(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1280., 900.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            (view.read(cx).inspector_width - 440.).abs() < f32::EPSILON,
            "baseline pins the default width this fit is measured against"
        );
        let bar = window.find("inspector-tabs-focus").bounds();
        for index in 0..crate::desktop::inspector::InspectorTab::ALL.len() {
            let tab = window.within("inspector-tabs").find(index).bounds();
            assert!(
                tab.left() >= bar.left() && tab.right() <= bar.right(),
                "tab {index} overflows the rail at default width: {tab:?} vs {bar:?}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn hiding_a_focused_inspector_restores_the_desktop_focus_path(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).inspector_focus.is_focused(window));

        view.update(cx, |view, cx| view.toggle_inspector(cx));
        window.render_frame(cx);

        assert!(!view.read(cx).inspector_open);
        assert!(view.read(cx).focus_handle.is_focused(window));
        window.press("cmd-l", cx);
        window.render_frame(cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_programmatic_selection_reveals_selected_tab(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 900.);
    // Narrow the rail so the scroll-reveal assertions below hold by
    // construction; the default width fits every tab (see the fit test).
    view.update(cx, |view, _| view.inspector_width = 240.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_utilities(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Utilities);
        assert!(
            horizontal_scroll_item_is_visible(&view.read(cx).inspector_tabs_scroll, 4),
            "workflow selection reveals the selected inspector tab"
        );
        assert!(
            view.read(cx).inspector_tabs_scroll.offset().x < px(0.),
            "workflow selection must scroll the narrow inspector tab row"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_model_picker_keeps_current_model_without_resetting_a_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.inspector_open = false;
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("keep-session".into());
            // Custom active models need not appear in the built-in catalog.
            view.configuration = Some(
                serde_json::from_value(json!({
                    "provider": "provider-a", "model": "custom-current-id", "authenticated": true,
                    "credentialSetupCommand": "", "contextWindow": 123456,
                }))
                .unwrap(),
            );
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "provider-b".into(),
                id: "other-id".into(),
                display_name: "Other model".into(),
                context_window: Some(7000),
                current: false,
                recommended_rank: None,
                capabilities: None,
            }];
            view.composer
                .update(cx, |state, cx| state.set_value("Keep my draft", window, cx));
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
        let current_model_item = window.find(palette_action_id(&super::super::PaletteAction::SelectModel {
            provider: "provider-a".into(),
            model: "custom-current-id".into(),
        }));
        let current_model_label = current_model_item.label().unwrap();
        assert!(current_model_label.contains("123.45K context"));
        assert!(current_model_label.contains("Current"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_none());
        assert_eq!(view.read(cx).selected_session_id.as_deref(), Some("keep-session"));
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep my draft");
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "choosing the current model must not restart the session");
}

#[gpui_kit::test]
fn model_picker_revalidates_a_deferred_selection_against_the_current_catalog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("original-session".into());
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "provider-b".into(),
                id: "outdated-model".into(),
                display_name: "Temporary model".into(),
                context_window: None,
                current: false,
                recommended_rank: None,
                capabilities: None,
            }];
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
        // The kit defers confirmation, so a catalog refresh can win this race.
        view.update(cx, |view, cx| {
            view.models.clear();
            cx.notify();
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_some());
        assert_eq!(view.read(cx).selected_session_id.as_deref(), Some("original-session"));
        assert!(view.read(cx).session_request.is_none());
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "a removed model cannot be selected by a stale callback");
}

#[gpui_kit::test]
fn navigation_can_collapse_and_restore_without_losing_the_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("toggle-sidebar").label(), Some("Toggle navigation"));
        assert_eq!(window.find("toggle-sidebar").checked(), Some(true));
        window.press("cmd-l", cx);
        window.input("keep this draft", cx);
        window.press("cmd-b", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).sidebar_open);
        assert_eq!(window.find("toggle-sidebar").checked(), Some(false));
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
        window.click("toggle-sidebar", cx);
        window.render_frame(cx);
        assert!(view.read(cx).sidebar_open);
        assert_eq!(window.find("toggle-sidebar").checked(), Some(true));
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn font_pickers_confirm_defaults_and_cancel_back_to_appearance(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.within("settings-panel").click("choose-ui-font", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose UI font"));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).route == Route::Settings);
        assert_eq!(cx.theme().font_family.as_str(), ".SystemUIFont");
        assert_eq!(cx.theme().font_size, px(16.));
        assert_eq!(cx.theme().mono_font_size, px(14.));
        window.within("settings-panel").click("choose-code-font", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose code font"));
        window.within("command-palette").press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).route == Route::Settings);
        assert!(view.read(cx).preferences.code_font_family.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_sizes_apply_live_survive_theme_changes_and_reset_only_appearance(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.within("settings-panel").click("font-ui-1", cx);
        window.render_frame(cx);
        window.within("settings-panel").click("font-code-0", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.ui_font_size(), 17);
        assert_eq!(view.read(cx).preferences.code_font_size(), 13);
        assert_eq!(cx.theme().font_size, px(17.));
        assert_eq!(cx.theme().mono_font_size, px(13.));
        let saved = crate::desktop::preferences::Preferences::load();
        assert_eq!((saved.ui_font_size(), saved.code_font_size()), (17, 13));
        view.update(cx, |view, cx| {
            view.preferences.touch_workspace("/preserve-workspace");
            view.apply_theme("ciapre".into(), window, cx);
        });
        assert_eq!(cx.theme().font_size, px(17.));
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-640.))), cx);
        window.within("settings-panel").click("reset-appearance", cx);
        window.render_frame(cx);
        assert_eq!(cx.theme().font_size, px(16.));
        assert_eq!(cx.theme().mono_font_size, px(14.));
        assert_eq!(view.read(cx).preferences.recent_workspaces[0], "/preserve-workspace");
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(24);
            view.preferences.code_font_size = Some(11);
            view.apply_typography(cx);
        });
        window.render_frame(cx);
        assert_eq!(window.within("settings-panel").find("font-ui-1").disabled(), Some(true));
        assert_eq!(window.within("settings-panel").find("font-code-0").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_app_icon_picker_switches_persists_and_resets(cx: &mut TestAppContext) {
    use vtcode_commons::ui_protocol::AppIconPreference;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        // The picker sits below reset-appearance; scroll it into view.
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-1600.))), cx);
        window.render_frame(cx);
        assert_eq!(window.find("appearance-app-icon-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("app-icon-dynamic").label(), Some("Dynamic (selected)"));
        window.within("settings-panel").click("app-icon-dark", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.app_icon(), AppIconPreference::Dark);
        assert_eq!(window.find("app-icon-dark").label(), Some("Dark (selected)"));
        assert_eq!(window.find("app-icon-dynamic").label(), Some("Dynamic"));
        let saved = crate::desktop::preferences::Preferences::load();
        assert_eq!(saved.app_icon(), AppIconPreference::Dark);
        // Scroll back to the top, then to the reset row above the picker.
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(1600.))), cx);
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-640.))), cx);
        window.within("settings-panel").click("reset-appearance", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.app_icon(), AppIconPreference::Dynamic);
        let reset = crate::desktop::preferences::Preferences::load();
        assert_eq!(reset.app_icon(), AppIconPreference::Dynamic);
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_dialog_traps_focus_and_escape_restores_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-l", cx);
        window.input("preserve this draft", cx);
        window.press("cmd-,", cx);
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Settings);
        assert_eq!(window.find("settings-panel").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("theme-ciapre-blue").role(), Some(gpui_kit::Role::Button));
        assert!(view.read(cx).settings_focus.contains_focused(window, cx));
        for _ in 0..8 {
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).settings_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).route != Route::Settings);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        assert_eq!(view.read(cx).composer.read(cx).value(), "preserve this draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_dismiss_button_restores_the_previous_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-l", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        assert!(window.within("settings-panel").find("settings-dismiss").visible());
        window.within("settings-panel").click("settings-dismiss", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).route != Route::Settings);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn welcome_workspace_prompt_wraps_long_names_and_preserves_workspace_guards(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    let workspace_name = "A workspace with a deliberately long name for responsive welcome typography";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.welcome_period = WelcomePeriod::Morning;
            view.welcome_copy_index = 0;
            view.workspace = Some(PathBuf::from("/projects").join(workspace_name));
        });
        window.render_frame(cx);
        let welcome = window.find("welcome").bounds();
        let link = window.find("welcome-workspace");
        assert_eq!(link.role(), Some(gpui_kit::Role::Button));
        assert_eq!(link.label(), Some(format!("Change workspace, current: {workspace_name}").as_str()));
        assert!(link.bounds().left() >= welcome.left() && link.bounds().right() <= welcome.right());
        assert!(link.bounds().size.height >= px(28.), "workspace link must retain headline-sized text");
        assert_eq!(window.find("welcome-prompt").role(), Some(gpui_kit::Role::Heading));
        assert_eq!(
            window.find("welcome-prompt").label(),
            Some(format!("Good morning. What should we build in {workspace_name}?").as_str())
        );
        view.update(cx, |view, _| view.workspace_picker_open = true);
        window.render_frame(cx);
        assert_eq!(window.find("welcome-workspace").disabled(), Some(true));
        view.update(cx, |view, _| {
            view.workspace_picker_open = false;
            view.welcome_period = WelcomePeriod::Evening;
            view.welcome_copy_index = 2;
            view.workspace = None;
        });
        window.render_frame(cx);
        assert!(window.find_all("welcome-workspace").is_empty());
        assert_eq!(window.find("welcome-prompt").label(), Some("Good evening. What are we exploring?"));
        assert_eq!(window.find("open-first-workspace").label(), Some("Select a folder"));
        window.within("welcome").click("open-first-workspace", cx);
        assert!(view.read(cx).workspace_picker_open, "the welcome action opens the native workspace picker");
    })
    .unwrap();

    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories);
        None
    });
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(!view.read(cx).workspace_picker_open, "canceling the picker restores the welcome state");
    })
    .unwrap();
}

#[test]
fn welcome_greeting_uses_local_day_period_boundaries() {
    use super::super::WelcomePeriod::{Afternoon, Evening, Morning};

    assert_eq!(super::super::welcome_period_for_hour(5), Morning);
    assert_eq!(super::super::welcome_period_for_hour(11), Morning);
    assert_eq!(super::super::welcome_period_for_hour(12), Afternoon);
    assert_eq!(super::super::welcome_period_for_hour(16), Afternoon);
    assert_eq!(super::super::welcome_period_for_hour(17), Evening);
    assert_eq!(super::super::welcome_period_for_hour(4), Evening);
}

#[gpui_kit::test]
fn welcome_greeting_refreshes_to_the_current_day_period(cx: &mut TestAppContext) {
    use chrono::Timelike;

    let (_window, view) = open_desktop(cx);
    let expected = super::super::welcome_period_for_hour(chrono::Local::now().hour());
    // Force a stale period that differs from now so the refresh is observable.
    let stale = match expected {
        WelcomePeriod::Morning => WelcomePeriod::Evening,
        _ => WelcomePeriod::Morning,
    };
    view.update(cx, |view, cx| {
        view.welcome_period = stale;
        view.refresh_welcome_period(cx);
        assert_eq!(view.welcome_period, expected);
        // A second refresh without a boundary crossing leaves the period alone.
        view.refresh_welcome_period(cx);
        assert_eq!(view.welcome_period, expected);
    });
}

#[gpui_kit::test]
fn window_maximized_state_persists_on_bounds_changes(cx: &mut TestAppContext) {
    let preferences_path = isolate_preferences();
    let (window, view) = open_desktop_sized_with_preferences_path(cx, 1280., 900., preferences_path.clone(), true);
    // Seed the file so the no-write check compares bytes: any save rewrites in pretty form.
    std::fs::write(&preferences_path, "{\"windowMaximized\":false}").expect("seed preferences file");
    view.update(cx, |view, _| {
        view.preferences.set_window_maximized(false);
    });
    cx.update_window(window, |_, window, cx| window.bounds_changed(cx)).unwrap();
    let seeded = std::fs::read_to_string(&preferences_path).expect("seeded file remains readable");
    assert_eq!(seeded, "{\"windowMaximized\":false}", "unchanged zoomed state must not rewrite preferences");
    // Headless windows report windowed, so flipping from maximized persists the change to disk.
    view.update(cx, |view, _| {
        view.preferences.set_window_maximized(true);
    });
    cx.update_window(window, |_, window, cx| window.bounds_changed(cx)).unwrap();
    let saved = std::fs::read_to_string(&preferences_path).expect("maximized flip is persisted");
    assert!(saved.contains("\"windowMaximized\": false"), "unexpected preferences content: {saved}");
    cx.update(|cx| {
        assert!(!view.read(cx).preferences.window_maximized());
    });
}
