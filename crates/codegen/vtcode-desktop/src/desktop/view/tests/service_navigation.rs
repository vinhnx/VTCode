use super::*;

#[gpui_kit::test]
fn disconnect_before_acceptance_retains_draft_with_uncertain_delivery(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Unconfirmed request", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(!view.running);
            assert!(view.backend.is_none());
            let draft = view.retained_submission.as_ref().unwrap();
            assert!(draft.delivery_uncertain);
            assert_eq!(draft.prompt, "Unconfirmed request");
            assert!(!view.transcript.is_empty(), "unconfirmed delivery is not a confirmed rejection");
            assert!(
                view.error
                    .as_deref()
                    .is_some_and(|error| error.contains("Restore or dismiss the retained message"))
            );
        });
        window.render_frame(cx);
        window.click("restore-submission", cx);
        let view = view.read(cx);
        assert_eq!(view.composer.read(cx).value(), "Unconfirmed request");
        assert!(view.transcript.is_empty());
    })
    .unwrap();
    assert!(requests.try_recv().is_ok());
    assert!(requests.try_recv().is_err(), "disconnect recovery never resends automatically");
}

#[gpui_kit::test]
fn acknowledged_submission_is_not_recovered_after_disconnect(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Accepted request", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "sessionId": "session", "turnId": "turn" })),
                    error: None,
                },
                cx,
            );
            assert!(view.pending_submission.is_none());
            assert_eq!(view.active_turn_id.as_deref(), Some("turn"));
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(view.retained_submission.is_none(), "accepted requests must not be offered as unsent drafts");
            assert_eq!(view.transcript.get(0).unwrap().text, "Accepted request");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn service_launch_failure_exposes_a_retry_path(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.first_submission = Some(super::super::SubmissionDraft {
                prompt: "Keep this request".into(),
                attachments: Vec::new(),
                echo_id: "first-request".into(),
                delivery_uncertain: false,
            });
            view.webmcp_status = Some(connected_webmcp_status());
            view.webmcp_pairing = Some(pending_webmcp_pairing());
            view.webmcp_request_pending = Some(vtcode_desktop_protocol::method::WEBMCP_STATUS);
            view.handle_backend_event(BackendEvent::LaunchFailed("Helper executable is unavailable.".into()), cx);
            assert_eq!(view.status, "Could not connect to the local VT Code service");
            assert_eq!(view.retained_submission.as_ref().map(|draft| draft.prompt.as_str()), Some("Keep this request"));
            assert!(view.webmcp_status.is_none());
            assert!(view.webmcp_pairing.is_none());
            assert!(view.webmcp_request_pending.is_none());
            assert!(!view.can_reload_cli_settings(), "resolve retained content before a service restart");
            assert!(view.error.as_deref().is_some_and(|error| {
                error.contains("Helper executable is unavailable")
                    && error.contains("Restore or dismiss the retained message")
                    && error.contains("Reload CLI settings")
            }));
        });

        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert!(error.label().unwrap().contains("Reload CLI settings"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn service_exit_clears_pending_interaction_and_answer_before_retry(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};

    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.interaction = Some(InteractionRequest {
                session_id: "session".into(),
                turn_id: "turn".into(),
                interaction_id: "approval".into(),
                kind: InteractionKind::Question,
                wizard: false,
                title: "Question".into(),
                details: Vec::new(),
                choices: Vec::new(),
                steps: Vec::new(),
                selected_choice_id: None,
                selected_choice_ids: Vec::new(),
            });
            view.interaction_choice_ids = vec![Some("approve".into())];
            view.pending_interaction_response = Some("approval".into());
            view.interaction_answer_texts = vec![Some("private answer".into())];
            view.question_answer
                .update(cx, |state, cx| state.set_value("private answer", window, cx));
            view.event_tx
                .send(BackendEvent::Exited { status: Some(1), authentication_required: false })
                .unwrap();

            let _batch_full = view.drain_events(window, cx);

            assert!(view.interaction.is_none());
            assert!(view.pending_interaction_response.is_none());
            assert!(view.interaction_choice_ids.is_empty());
            assert!(view.interaction_answer_texts.is_empty());
            assert!(view.question_answer.read(cx).value().is_empty());
            assert!(view.can_reload_cli_settings(), "an exited interaction cannot block reconnection");
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn service_exit_discards_stale_webmcp_state_and_refreshes_after_reconnect(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.webmcp_status = Some(connected_webmcp_status());
            view.webmcp_pairing = Some(pending_webmcp_pairing());
            view.webmcp_request_pending = Some(vtcode_desktop_protocol::method::WEBMCP_STATUS);
            view.event_tx
                .send(BackendEvent::Exited { status: Some(1), authentication_required: false })
                .unwrap();

            let _batch_full = view.drain_events(window, cx);

            assert!(view.webmcp_status.is_none());
            assert!(view.webmcp_pairing.is_none(), "pairing codes belong to the exited service process");
            assert!(view.webmcp_request_pending.is_none(), "an abandoned request cannot keep the panel busy");
            assert!(
                view.webmcp_notice
                    .as_ref()
                    .is_some_and(|notice| notice.text().contains("disconnected"))
            );

            let (commands, mut requests) = tokio::sync::mpsc::channel(8);
            view.backend = Some(BackendHandle { commands });
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::INITIALIZE.into(),
                    result: Some(json!({
                        "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION,
                        "workspace": "/workspace-new",
                        "serverVersion": "test",
                    })),
                    error: None,
                },
                cx,
            );
            let mut methods = Vec::new();
            for _ in 0..5 {
                let Some(BackendCommand::Request { method, .. }) = requests.try_recv().ok() else {
                    panic!("initialization should refresh WebMCP state");
                };
                methods.push(method);
            }
            assert_eq!(methods.last().map(String::as_str), Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            assert!(view.webmcp_status.is_none(), "the new status appears only after its response arrives");

            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::WEBMCP_STATUS.into(),
                    result: Some(serde_json::to_value(connected_webmcp_status()).unwrap()),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.webmcp_status, Some(connected_webmcp_status()));
            assert!(view.webmcp_request_pending.is_none());
            assert!(view.webmcp_notice.is_none(), "a successful refresh clears the disconnect notice");
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn unexpected_service_exit_replaces_connected_status_and_exposes_recovery(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.status = "Connected to the local VT Code service".into();
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);

            assert!(view.backend.is_none());
            assert_eq!(view.status, "Local service exited with code 1");
            assert!(view.can_reload_cli_settings(), "disconnected workspaces can restart the service");
            assert!(view.error.as_deref().is_some_and(|error| error.contains("Reload CLI settings")));
        });

        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert!(error.label().unwrap().contains("Reload CLI settings"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn authentication_failure_explains_how_to_reconnect_without_restarting_desktop(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.retained_submission = Some(super::super::SubmissionDraft {
                prompt: "Uncertain request".into(),
                attachments: Vec::new(),
                echo_id: "uncertain-request".into(),
                delivery_uncertain: true,
            });
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: true }, cx);
            assert_eq!(view.status, "Provider credentials required");
            let error = view.error.as_deref().unwrap();
            assert!(error.contains("vtcode auth"));
            assert!(error.contains("Restore or dismiss the retained message"));
            assert!(error.contains("Reload CLI settings"));
            assert!(!error.contains("restart the desktop app"));
            assert!(!view.can_reload_cli_settings(), "the retained submission must be resolved before restart");
        });
        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert!(error.label().unwrap().contains("Reload CLI settings"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn delayed_acknowledgement_cannot_restart_a_completed_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Quick request", window, cx));
            view.submit_prompt(window, cx);
            assert!(view.pending_submission.is_some());
            view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
                session_id: "session".into(),
                turn_id: Some("finished-turn".into()),
                event: vtcode_exec_events::VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::TurnCompleted(
                    vtcode_exec_events::TurnCompletedEvent {
                        completed_at: None,
                        usage: vtcode_exec_events::Usage {
                            input_tokens: 1,
                            output_tokens: 2,
                            cached_input_tokens: 0,
                            cache_creation_tokens: 0,
                        },
                        in_progress_exec_sessions: Vec::new(),
                    },
                )),
            });
            assert!(view.pending_submission.is_none(), "terminal runtime event proves input was accepted");
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "sessionId": "session", "turnId": "finished-turn" })),
                    error: None,
                },
                cx,
            );
            assert!(!view.running);
            assert!(view.active_turn_id.is_none());
            assert_eq!(view.status, "Completed");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn malformed_acknowledgement_retains_unconfirmed_submission(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer.update(cx, |state, cx| state.set_value("Retain this", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "turnId": "unscoped" })),
                    error: None,
                },
                cx,
            );
            assert!(!view.running);
            assert!(view.active_turn_id.is_none());
            assert!(view.retained_submission.as_ref().unwrap().delivery_uncertain);
            assert_eq!(view.retained_submission.as_ref().unwrap().prompt, "Retain this");
            assert!(view.error.as_deref().unwrap().contains("invalid turn acknowledgement"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn keyboard_shortcuts_share_session_and_cancellation_guards(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.focus_composer(window, cx);
        });
        window.render_frame(cx);
        window.press("cmd-n", cx);
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                    result: Some(json!({ "closed": true })),
                    error: None,
                },
                cx,
            );
            view.selected_session_id = Some("session".into());
            cx.notify();
        });
        window.render_frame(cx);
        window.press("cmd-shift-w", cx);
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                    result: Some(json!({ "closed": true })),
                    error: None,
                },
                cx,
            )
        });
        window.press("cmd-shift-i", cx);
        assert!(!view.read(cx).inspector_open);
        assert_eq!(view.read(cx).preferences.inspector_open, Some(false));
        window.press("cmd-shift-i", cx);
        assert!(view.read(cx).inspector_open);
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            view.running = true;
            view.active_turn_id = Some("turn".into());
        });
        window.press("cmd-n", cx);
        window.press("cmd-shift-w", cx);
        window.press("escape", cx);
        assert!(view.read(cx).running, "Escape without an overlay must not cancel a run");
        assert!(!view.read(cx).cancel_requested);
        window.press("cmd-.", cx);
        assert!(view.read(cx).cancel_requested);
        assert!(view.read(cx).cancel_request_sent);
    })
    .unwrap();
    for expected in [
        vtcode_desktop_protocol::method::SESSION_CLOSE,
        vtcode_desktop_protocol::method::SESSION_CLOSE,
        vtcode_desktop_protocol::method::TURN_CANCEL,
    ] {
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("request")
        };
        assert_eq!(method, expected);
        if method == vtcode_desktop_protocol::method::TURN_CANCEL {
            assert_eq!(params, json!({ "sessionId": "session", "turnId": "turn" }));
        }
    }
    assert!(requests.try_recv().is_err(), "active work blocks session changes for shortcuts too");
}

#[gpui_kit::test]
fn workspace_shortcuts_match_navigation_and_review_controls(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
                slash_commands: Vec::new(),
            });
        });
        window.render_frame(cx);

        window.press("cmd-shift-u", cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Utilities);

        window.press("cmd-shift-e", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);

        window.press("cmd-shift-g", cx);
        window.render_frame(cx);
        assert!(view.read(cx).review_open);
        assert!(view.read(cx).workspace_review.is_some());

        window.press("cmd-r", cx);
        assert!(view.read(cx).workspace_review.as_ref().unwrap().pending);
    })
    .unwrap();

    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("workspace review request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WORKSPACE_REVIEW);
}

#[gpui_kit::test]
fn navigation_toolbar_and_shortcuts_traverse_thread_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let workspace = std::env::temp_dir();
    let previous = vtcode_desktop_protocol::SessionSummary {
        session_id: "previous-session".into(),
        title: Some("Previous".into()),
        workspace: workspace.display().to_string(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    };
    let current = vtcode_desktop_protocol::SessionSummary {
        session_id: "current-session".into(),
        title: Some("Current".into()),
        workspace: workspace.display().to_string(),
        completed: false,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    };
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(workspace.clone());
        view.sessions = vec![current.clone(), previous.clone()];
        view.selected_session_id = Some(current.session_id.clone());
        view.navigation_history.record(NavigationLocation {
            workspace: workspace.clone(),
            session_id: Some(previous.session_id.clone()),
        });
        view.navigation_history.record(NavigationLocation {
            workspace: workspace.clone(),
            session_id: Some(current.session_id.clone()),
        });
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("navigate-back", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("back button should load the previous thread");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "previous-session" }));

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({
                    "session": previous,
                    "messages": [{ "role": "assistant", "text": "Previous conversation" }],
                })),
                error: None,
            },
            cx,
        );
        assert_eq!(view.selected_session_id.as_deref(), Some("current-session"));
        assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let view = view.read(cx);
        assert_eq!(view.selected_session_id.as_deref(), Some("previous-session"));
        assert_eq!(view.session_request, None);
        assert_eq!(view.navigation_history.back_index(), None);
        assert_eq!(view.navigation_history.forward_index(), Some(1));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-]", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("forward shortcut should load the next thread");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "current-session" }));
}

#[gpui_kit::test]
fn trackpad_swipe_back_loads_previous_thread_with_loading_feedback(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("swipe back should load the previous thread");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-first" }));

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert_eq!(view_state.pending_navigation.map(|pending| pending.target_index), Some(0));
        assert_eq!(view_state.pending_navigation_direction(), Some(NavigationDirection::Back));
        assert_eq!(window.find("navigate-back").label(), Some("Back (navigating…)"));
        assert_eq!(window.find("navigate-forward").label(), Some("Forward"));
        let loading = window.find("navigation-loading");
        assert_eq!(loading.role(), Some(gpui_kit::Role::Status));
        assert_eq!(loading.label(), Some("Navigating back…"));
    })
    .unwrap();

    swipe_navigate(window, NavigationDirection::Back, cx);
    assert!(requests.try_recv().is_err(), "a second swipe while pending must not issue another request");

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({
                    "session": summaries[0],
                    "messages": [{ "role": "assistant", "text": "First conversation" }],
                })),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert_eq!(view_state.selected_session_id.as_deref(), Some("swipe-first"));
        assert!(view_state.pending_navigation.is_none());
        assert_eq!(window.find("navigate-back").label(), Some("Back"));
        assert!(window.try_find("navigation-loading").is_none(), "loading status clears after the load");
    })
    .unwrap();
}

#[gpui_kit::test]
fn trackpad_swipe_forward_loads_next_thread_with_loading_feedback(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Forward, cx);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("swipe forward should load the next thread");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-third" }));

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert_eq!(view_state.pending_navigation.map(|pending| pending.target_index), Some(2));
        assert_eq!(view_state.pending_navigation_direction(), Some(NavigationDirection::Forward));
        assert_eq!(window.find("navigate-back").label(), Some("Back"));
        assert_eq!(window.find("navigate-forward").label(), Some("Forward (navigating…)"));
        let loading = window.find("navigation-loading");
        assert_eq!(loading.role(), Some(gpui_kit::Role::Status));
        assert_eq!(loading.label(), Some("Navigating forward…"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn trackpad_swipe_ignores_empty_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let workspace = std::env::temp_dir();
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(workspace.clone());
        view.selected_session_id = Some("only-session".into());
        view.navigation_history
            .record(NavigationLocation { workspace, session_id: Some("only-session".into()) });
    });

    swipe_navigate(window, NavigationDirection::Back, cx);
    swipe_navigate(window, NavigationDirection::Forward, cx);
    assert!(requests.try_recv().is_err(), "swipes with no history in either direction must not navigate");
}

#[gpui_kit::test]
fn trackpad_swipe_over_open_palette_does_not_navigate(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        window.click_with_options(
            "pane-root",
            ClickOptions::new().with_button(MouseButton::Navigate(NavigationDirection::Back)),
            cx,
        );
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "a swipe over an open palette must not navigate the background");

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.close_palette(window, cx));
        window.render_frame(cx);
        window.click_with_options(
            "pane-root",
            ClickOptions::new().with_button(MouseButton::Navigate(NavigationDirection::Back)),
            cx,
        );
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("the same swipe navigates once the palette is closed");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-first" }));
}

#[gpui_kit::test]
fn swipe_navigation_guard_covers_modal_overlay_state(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        assert!(view.swipe_navigation_allowed());
        view.settings_open = true;
        assert!(!view.swipe_navigation_allowed());
        view.settings_open = false;
        view.config_page_open = true;
        assert!(!view.swipe_navigation_allowed());
        view.config_page_open = false;
        view.review_open = true;
        assert!(!view.swipe_navigation_allowed());
        view.review_open = false;
        view.file_preview = Some(crate::desktop::files::FilePreview {
            rel_path: "src/app.rs".into(),
            text: "preview".into(),
            truncated: false,
        });
        assert!(!view.swipe_navigation_allowed());
        view.file_preview = None;
        assert!(view.swipe_navigation_allowed());
    });
}

#[gpui_kit::test]
fn navigating_back_to_a_blank_thread_closes_the_service_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            let workspace = view.workspace.clone().unwrap();
            view.navigation_history
                .record(NavigationLocation { workspace: workspace.clone(), session_id: None });
            view.navigation_history
                .record(NavigationLocation { workspace, session_id: Some("current".into()) });
            view.selected_session_id = Some("current".into());
            view.navigate_back(cx);
            assert_eq!(view.selected_session_id.as_deref(), Some("current"));
            assert_eq!(view.pending_navigation.map(|pending| pending.target_index), Some(0));
        });
        window.render_frame(cx);
        assert_eq!(window.find("new-thread-pending").role(), Some(gpui_kit::Role::Status));
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("close")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
        assert_eq!(params, json!({ "sessionId": "current" }));
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                    result: Some(json!({ "closed": true })),
                    error: None,
                },
                cx,
            );
            assert!(view.selected_session_id.is_none());
            assert!(view.pending_navigation.is_none());
            assert_eq!(view.navigation_history.forward_index(), Some(1));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn cross_workspace_back_waits_for_session_discovery_before_loading(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let target_workspace = PathBuf::from("/workspace-a");
    let current_workspace = PathBuf::from("/workspace-b");
    let target = vtcode_desktop_protocol::SessionSummary {
        session_id: "target-session".into(),
        title: Some("Target".into()),
        workspace: target_workspace.display().to_string(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    };
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(target_workspace.clone());
        view.navigation_history.record(NavigationLocation {
            workspace: target_workspace.clone(),
            session_id: Some(target.session_id.clone()),
        });
        view.navigation_history
            .record(NavigationLocation { workspace: current_workspace, session_id: None });
        assert!(view.navigation_history.navigate_to(1));
        view.pending_navigation = Some(PendingNavigation { target_index: 0, workspace_switch: true });
    });

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LIST.into(),
                result: Some(json!([target])),
                error: None,
            },
            cx,
        );
        assert_eq!(view.pending_navigation.map(|pending| pending.target_index), Some(0));
        assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
    });
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("session discovery should be followed by the destination load");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "target-session" }));

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({ "session": target, "messages": [] })),
                error: None,
            },
            cx,
        );
        assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let view = view.read(cx);
        assert_eq!(view.selected_session_id.as_deref(), Some("target-session"));
        assert!(view.pending_navigation.is_none());
        assert_eq!(view.navigation_history.back_index(), None);
        assert_eq!(view.navigation_history.forward_index(), Some(1));
    });
}

#[gpui_kit::test]
fn escape_closes_preview_and_returns_keyboard_input_to_composer(cx: &mut TestAppContext) {
    use gpui_kit::Focusable;
    let workspace = tempfile::TempDir::new().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "Preview text").unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.focus_composer(window, cx);
            view.open_file_preview("notes.txt".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.is_some());
        assert!(view.read(cx).preview_focus.is_focused(window));
        window.press("escape", cx);
        assert!(view.read(cx).file_preview.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.input("Continue editing", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Continue editing");
    })
    .unwrap();
}
