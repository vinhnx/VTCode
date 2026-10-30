use super::*;

#[gpui_kit::test]
fn palette_cancel_restores_focus_and_composer_shortcut_dismisses_overlay(cx: &mut TestAppContext) {
    use gpui_kit::Focusable;
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-l", cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.press("cmd-k", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.press("cmd-k", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.press("cmd-l", cx);
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.input("A focused draft", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "A focused draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn rejected_first_prompt_waits_for_startup_answer_and_retries_once(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer.update(cx, |input, cx| input.set_value("First prompt", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment { path: "notes.txt".into(), text: "original".into() });
            view.submit_prompt(window, cx);
        });
        assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { method, .. }) if method == vtcode_desktop_protocol::method::SESSION_CREATE));
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::Response { method: vtcode_desktop_protocol::method::SESSION_CREATE.into(), result: Some(json!({ "sessionId": "new", "title": null, "workspace": "/workspace", "completed": false, "resumable": true })), error: None }, cx);
        });
        let BackendCommand::Request { method, params: original } = requests.try_recv().unwrap() else { panic!("first turn") };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| input.set_value("Newer draft", window, cx));
            view.handle_backend_event(BackendEvent::TurnBlockedByStartupInteraction { session_id: "old".into() }, cx);
            assert!(view.pending_submission.is_some());
            assert!(view.running);
            view.handle_backend_event(BackendEvent::TurnBlockedByStartupInteraction { session_id: "new".into() }, cx);
            assert!(view.first_submission.is_some());
            assert!(!view.running);
            assert!(view.transcript.is_empty());
            assert!(!view.workspace_switch_idle());
            view.handle_backend_event(BackendEvent::Interaction(InteractionRequest {
                session_id: "new".into(), turn_id: "session-start".into(), interaction_id: "question".into(),
                kind: InteractionKind::Question, wizard: false, title: "Question".into(), details: Vec::new(),
                choices: Vec::new(), steps: Vec::new(), selected_choice_id: None, selected_choice_ids: Vec::new(),
            }), cx);
            view.submit_interaction(Vec::new(), true, window, cx);
        });
        assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { method, .. }) if method == vtcode_desktop_protocol::method::INTERACTION_RESPOND));
        assert!(requests.try_recv().is_err(), "the first prompt waits for the acknowledged decision");
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::InteractionResponded { identity: InteractionResponseIdentity {
                session_id: "new".into(), turn_id: "session-start".into(), interaction_id: "question".into(), cancelled: true,
            }, error: Some("Decision rejected".into()) }, cx);
            assert!(view.first_submission.is_some(), "a failed decision must not send or discard the prompt");
            assert!(view.interaction.is_some());
            view.submit_interaction(Vec::new(), true, window, cx);
        });
        assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { method, .. }) if method == vtcode_desktop_protocol::method::INTERACTION_RESPOND));
        assert!(requests.try_recv().is_err());
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::InteractionResponded { identity: InteractionResponseIdentity {
                session_id: "new".into(), turn_id: "session-start".into(), interaction_id: "question".into(), cancelled: true,
            }, error: None }, cx);
            assert_eq!(view.composer.read(cx).value(), "Newer draft");
            assert!(view.first_submission.is_none());
            assert!(view.running);
        });
        let BackendCommand::Request { method, params: retried } = requests.try_recv().unwrap() else { panic!("retried turn") };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        assert_eq!(retried, original, "the exact prompt and attachments are retried");
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::TurnBlockedByStartupInteraction { session_id: "new".into() }, cx);
            assert!(view.first_submission.is_none());
            assert_eq!(view.retained_submission.as_ref().unwrap().prompt, "First prompt");
        });
        assert!(requests.try_recv().is_err(), "only one retry is queued");
    }).unwrap();
}

#[gpui_kit::test]
fn disconnect_while_waiting_for_startup_decision_retains_unsent_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer.update(cx, |input, cx| input.set_value("Preserve this", window, cx));
            view.submit_prompt(window, cx);
            assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { method, .. }) if method == vtcode_desktop_protocol::method::SESSION_CREATE));
            view.handle_backend_event(BackendEvent::Response { method: vtcode_desktop_protocol::method::SESSION_CREATE.into(), result: Some(json!({ "sessionId": "new", "title": null, "workspace": "/workspace", "completed": false, "resumable": true })), error: None }, cx);
            assert!(matches!(requests.try_recv(), Ok(BackendCommand::Request { method, .. }) if method == vtcode_desktop_protocol::method::TURN_SUBMIT));
            view.handle_backend_event(BackendEvent::TurnBlockedByStartupInteraction { session_id: "new".into() }, cx);
            view.handle_backend_event(BackendEvent::Exited { status: None, authentication_required: false }, cx);
            let draft = view.retained_submission.as_ref().expect("unsent prompt retained");
            assert_eq!(draft.prompt, "Preserve this");
            assert!(!draft.delivery_uncertain);
            assert!(view.first_submission.is_none());
            assert!(requests.try_recv().is_err());
        });
    }).unwrap();
}

#[gpui_kit::test]
fn pending_startup_send_can_be_kept_as_a_draft(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("new".into());
            view.first_submission = Some(super::super::SubmissionDraft {
                prompt: "Keep this".into(),
                attachments: Vec::new(),
                echo_id: "echo".into(),
                delivery_uncertain: false,
            });
            view.interaction = Some(InteractionRequest {
                session_id: "new".into(),
                turn_id: "session-start".into(),
                interaction_id: "question".into(),
                kind: InteractionKind::Question,
                wizard: false,
                title: "Question".into(),
                details: Vec::new(),
                choices: Vec::new(),
                steps: Vec::new(),
                selected_choice_id: None,
                selected_choice_ids: Vec::new(),
            });
        });
        window.render_frame(cx);
        window.click("cancel-startup-send", cx);
        view.update(cx, |view, cx| {
            assert!(view.first_submission.is_none());
            assert_eq!(view.retained_submission.as_ref().unwrap().prompt, "Keep this");
            view.pending_interaction_response = Some("question".into());
            view.handle_backend_event(
                BackendEvent::InteractionResponded {
                    identity: InteractionResponseIdentity {
                        session_id: "new".into(),
                        turn_id: "session-start".into(),
                        interaction_id: "question".into(),
                        cancelled: true,
                    },
                    error: None,
                },
                cx,
            );
        });
        assert!(requests.try_recv().is_err(), "a cancelled pending send is never retried");
    })
    .unwrap();
}

#[gpui_kit::test]
fn first_send_creates_one_session_then_submits_the_captured_message(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer.update(cx, |state, cx| state.set_value("First request", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment { path: "first.rs".into(), text: "first content".into() });
        });
        window.render_frame(cx);
        window.click("send-prompt", cx);
        assert_eq!(view.read(cx).session_request, Some(vtcode_desktop_protocol::method::SESSION_CREATE));
        assert!(view.read(cx).first_submission.is_some());
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("Newer unsent draft", window, cx));
            view.submit_prompt(window, cx);
            view.create_session(cx);
            let _ = view.load_session("other-session".into(), cx);
            view.select_model("other".into(), "model".into(), cx);
            assert!(!view.workspace_switch_idle());
        });
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else { panic!("create") };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
        assert!(requests.try_recv().is_err(), "transition blocks repeated Send and session/model changes");
        view.update(cx, |view, cx| view.handle_backend_event(BackendEvent::Response {
            method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
            result: Some(json!({ "sessionId": "created", "title": null, "workspace": "/workspace", "completed": false, "resumable": true })), error: None,
        }, cx));
        assert!(view.read(cx).first_submission.is_none());
        assert!(view.read(cx).session_request.is_none());
        assert!(view.read(cx).running);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Newer unsent draft");
        assert_eq!(view.read(cx).transcript.len(), 1);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else { panic!("submit") };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        assert_eq!(params, json!({ "sessionId": "created", "prompt": "First request", "attachments": [{ "path": "first.rs", "text": "first content" }] }));
        assert!(requests.try_recv().is_err(), "exactly one turn is submitted");
    }).unwrap();
}

#[gpui_kit::test]
fn new_thread_shows_pending_then_creates_a_runtime_on_first_send(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("old".into());
            view.transcript.push(TranscriptItem::new(
                "old-message".into(),
                "assistant".into(),
                "VT Code".into(),
                "Old conversation".into(),
            ));
            view.attachments
                .push(vtcode_desktop_protocol::TurnAttachment { path: "notes.txt".into(), text: "source".into() });
            view.composer.update(cx, |input, cx| input.set_value("New draft", window, cx));
            view.create_session(cx);
        });
        window.render_frame(cx);
        let pending = window.find("new-thread-pending");
        assert_eq!(pending.role(), Some(gpui_kit::Role::Status));
        assert_eq!(pending.label(), Some("Preparing a new thread…"));
        assert!(window.try_find("message-old-message").is_none());
        assert_eq!(
            view.read(cx).selected_session_id.as_deref(),
            Some("old"),
            "old session remains recoverable until close succeeds"
        );
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("Edited while closing", window, cx));
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                    result: Some(json!({ "closed": true })),
                    error: None,
                },
                cx,
            );
            assert!(view.selected_session_id.is_none());
            assert!(view.transcript.is_empty());
            assert_eq!(view.attachments[0].path, "notes.txt");
            assert_eq!(view.composer.read(cx).value(), "Edited while closing");
        });
        window.render_frame(cx);
        assert!(window.try_find("new-thread-pending").is_none());
        assert!(window.try_find("welcome").is_some());
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("close")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
        assert_eq!(params, json!({ "sessionId": "old" }));
        assert!(requests.try_recv().is_err(), "opening a draft must not create a runtime");
        view.update(cx, |view, cx| view.submit_prompt(window, cx));
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("create")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
        assert_eq!(view.read(cx).first_submission.as_ref().unwrap().prompt, "Edited while closing");
    })
    .unwrap();
}

#[gpui_kit::test]
fn failed_new_thread_close_restores_the_previous_conversation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("old".into());
            view.transcript.push(TranscriptItem::new(
                "old-message".into(),
                "assistant".into(),
                "VT Code".into(),
                "Old conversation".into(),
            ));
            view.create_session(cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                    result: None,
                    error: Some("Close failed".into()),
                },
                cx,
            );
            assert_eq!(view.selected_session_id.as_deref(), Some("old"));
            assert_eq!(view.transcript.len(), 1);
            assert_eq!(view.error.as_deref(), Some("Close failed"));
            assert!(!view.pending_new_thread);
        });
        window.render_frame(cx);
        assert!(window.try_find("new-thread-pending").is_none());
        assert!(window.try_find("welcome").is_none(), "the previous transcript remains selected");
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_creation_failure_retains_first_message_and_unlocks_transitions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer
                .update(cx, |state, cx| state.set_value("Keep first message", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "keep.txt".into(),
                text: "keep attachment".into(),
            });
            view.submit_prompt(window, cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
                    result: None,
                    error: Some("Provider could not start".into()),
                },
                cx,
            );
            assert!(view.first_submission.is_none());
            assert!(view.session_request.is_none());
            assert!(!view.running);
            assert!(view.selected_session_id.is_none());
            assert!(view.transcript.is_empty());
            assert!(view.retained_submission.is_some());
            assert_eq!(view.error.as_deref(), Some("Provider could not start"));
        });
        window.render_frame(cx);
        window.click("restore-submission", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep first message");
        assert_eq!(view.read(cx).attachments[0].text, "keep attachment");
        window.render_frame(cx);
        window.click("send-prompt", cx);
    })
    .unwrap();
    for _ in 0..2 {
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("create")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
    }
    assert!(requests.try_recv().is_err(), "failed creation never submits a turn");
}

#[gpui_kit::test]
fn session_load_blocks_submission_until_the_new_session_is_selected(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(3);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("old-session".into());
            view.composer.update(cx, |state, cx| state.set_value("Send to loaded session", window, cx));
            let _ = view.load_session("new-session".into(), cx);
            view.submit_prompt(window, cx);
            view.close_session(cx);
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({ "session": { "sessionId": "new-session", "title": "New", "workspace": "/workspace", "completed": false, "resumable": true }, "messages": [] })), error: None,
            }, cx);
            view.submit_prompt(window, cx);
            assert_eq!(view.selected_session_id.as_deref(), Some("old-session"));
            assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
        });
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.selected_session_id.as_deref(), Some("new-session"));
            assert!(view.session_request.is_none());
            view.submit_prompt(window, cx);
        });
    })
    .unwrap();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("load")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("submit")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["sessionId"], "new-session");
    assert_eq!(params["prompt"], "Send to loaded session");
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn first_message_survives_failed_handoff_after_session_creation(cx: &mut TestAppContext) {
    for malformed in [false, true] {
        let (window, view) = open_desktop(cx);
        let (commands, mut requests) = tokio::sync::mpsc::channel(1);
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.backend = Some(BackendHandle { commands: commands.clone() });
                view.composer.update(cx, |state, cx| state.set_value("Captured first message", window, cx));
                view.submit_prompt(window, cx);
                assert!(requests.try_recv().is_ok());
                if !malformed {
                    commands.try_send(BackendCommand::Stop).unwrap();
                }
                view.composer.update(cx, |state, cx| state.set_value("New draft stays", window, cx));
                view.handle_backend_event(BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
                    result: Some(if malformed { json!({}) } else { json!({ "sessionId": "created", "title": null, "workspace": "/workspace", "completed": false, "resumable": true }) }), error: None,
                }, cx);
                assert!(view.first_submission.is_none());
                assert!(view.session_request.is_none());
                assert!(view.pending_submission.is_none());
                assert!(!view.running);
                assert!(view.transcript.is_empty());
                assert_eq!(view.retained_submission.as_ref().unwrap().prompt, "Captured first message");
                assert_eq!(view.composer.read(cx).value(), "New draft stays");
                assert_eq!(view.selected_session_id.as_deref(), if malformed { None } else { Some("created") });
                assert!(view.error.is_some());
            });
        }).unwrap();
    }
}

#[gpui_kit::test]
fn blank_first_prompt_and_full_creation_queue_keep_the_composer_intact(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands: commands.clone() });
            view.composer.update(cx, |state, cx| state.set_value("   ", window, cx));
            view.submit_prompt(window, cx);
            assert!(requests.try_recv().is_err(), "blank Enter must not create empty sessions");
            commands.try_send(BackendCommand::Stop).unwrap();
            view.composer
                .update(cx, |state, cx| state.set_value("Keep unsent message", window, cx));
            view.attachments
                .push(vtcode_desktop_protocol::TurnAttachment { path: "notes.txt".into(), text: "notes".into() });
            view.submit_prompt(window, cx);
            assert!(view.session_request.is_none());
            assert!(view.first_submission.is_none());
            assert!(view.retained_submission.is_none());
            assert_eq!(view.composer.read(cx).value(), "Keep unsent message");
            assert_eq!(view.attachments[0].text, "notes");
            assert!(view.error.as_deref().unwrap().contains("queue is full"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn disconnect_during_creation_retains_a_message_that_was_never_submitted(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer
                .update(cx, |state, cx| state.set_value("Never submitted", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(view.session_request.is_none());
            assert!(view.first_submission.is_none());
            let retained = view.retained_submission.as_ref().unwrap();
            assert_eq!(retained.prompt, "Never submitted");
            assert!(!retained.delivery_uncertain, "only session creation crossed the wire");
            assert!(view.transcript.is_empty());
        });
    })
    .unwrap();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("create")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn stale_runtime_notifications_cannot_finish_a_new_submission_or_reopen_a_closed_session(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadEvent, TurnFailedEvent, TurnStartedEvent, VersionedThreadEvent};
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("First request", window, cx));
            view.submit_prompt(window, cx);
            let failure = || {
                BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "session".into(),
                    turn_id: Some("old-turn".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
                        completed_at: None,
                        message: "Old failure".into(),
                        usage: None,
                    })),
                })
            };
            view.handle_backend_event(failure(), cx);
            assert!(!view.running);
            assert_eq!(view.finished_turn_id.as_deref(), Some("old-turn"));
            view.composer
                .update(cx, |state, cx| state.set_value("Second request", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(failure(), cx);
            assert!(view.running, "old terminal tails cannot finish an unacknowledged submission");
            assert_eq!(view.status, "Submitting turn…");
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "sessionId": "session", "turnId": "new-turn" })),
                    error: None,
                },
                cx,
            );
            view.handle_backend_event(failure(), cx);
            view.handle_backend_event(
                BackendEvent::TurnStatus(json!({ "sessionId": "session", "turnId": "old-turn", "status": "failed" })),
                cx,
            );
            assert!(view.running);
            assert_eq!(view.active_turn_id.as_deref(), Some("new-turn"));
            assert_eq!(view.status, "Running");
            view.handle_backend_event(
                BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "other-session".into(),
                    turn_id: Some("other-turn".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
                }),
                cx,
            );
            assert_eq!(view.active_turn_id.as_deref(), Some("new-turn"));
            view.handle_backend_event(
                BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "session".into(),
                    turn_id: Some("new-turn".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
                        completed_at: None,
                        message: "New failure".into(),
                        usage: None,
                    })),
                }),
                cx,
            );
            assert!(!view.running, "the current turn still reaches a terminal state");
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                    result: Some(json!({ "closed": true })),
                    error: None,
                },
                cx,
            );
            view.handle_backend_event(failure(), cx);
            view.handle_backend_event(
                BackendEvent::TurnStatus(json!({ "sessionId": "session", "turnId": "old-turn", "status": "failed" })),
                cx,
            );
            assert!(view.transcript.is_empty());
            assert!(!view.running);
            assert_eq!(view.status, "Session closed. Start a new session to continue.");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn interaction_notifications_preserve_startup_questions_and_reject_stale_turns(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.running = true;
            view.active_turn_id = Some("active-turn".into());
            let request = |session: &str, turn: &str, id: &str| {
                BackendEvent::Interaction(InteractionRequest {
                    session_id: session.into(),
                    turn_id: turn.into(),
                    interaction_id: id.into(),
                    kind: InteractionKind::Question,
                    wizard: false,
                    title: "Question".into(),
                    details: Vec::new(),
                    choices: Vec::new(),
                    steps: Vec::new(),
                    selected_choice_id: None,
                    selected_choice_ids: Vec::new(),
                })
            };
            view.handle_backend_event(request("other-session", "active-turn", "wrong-session"), cx);
            view.handle_backend_event(request("session", "old-turn", "wrong-turn"), cx);
            assert!(view.interaction.is_none());
            view.handle_backend_event(request("session", "session-start", "startup-question"), cx);
            assert_eq!(view.interaction.as_ref().unwrap().interaction_id, "startup-question");
            view.handle_backend_event(request("session", "active-turn", "current-question"), cx);
            assert_eq!(view.interaction.as_ref().unwrap().interaction_id, "current-question");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn multi_step_question_progress_tracks_the_runtime_step_order(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{InteractionChoice, InteractionKind, InteractionRequest, InteractionStep};

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let make_choice = |id: &str| InteractionChoice {
        id: id.into(),
        label: format!("Choose {id}"),
        description: None,
        recommended: false,
    };
    let steps = [
        ("Workspace", "Choose a workspace", "workspace"),
        ("Task", "Describe the task", "task"),
        ("Review", "Review the plan", "review"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (title, question, choice_id))| {
        let mut choices = vec![make_choice(choice_id)];
        if index == 0 {
            choices.push(make_choice("another-workspace"));
        }
        InteractionStep {
            title: title.into(),
            question: question.into(),
            choices,
            selected_choice_id: None,
            allow_freeform: true,
        }
    })
    .collect();
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.interaction = Some(InteractionRequest {
            session_id: "session".into(),
            turn_id: "turn".into(),
            interaction_id: "question-wizard".into(),
            kind: InteractionKind::Question,
            wizard: true,
            title: "Set up the task".into(),
            details: Vec::new(),
            choices: Vec::new(),
            steps,
            selected_choice_id: None,
            selected_choice_ids: Vec::new(),
        });
        view.interaction_step_index = 0;
        view.interaction_choice_ids = vec![None; 3];
        view.interaction_answer_texts = vec![None; 3];
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("interaction-question-progress").label(), Some("Question progress"));
        let stepper = window
            .within("interaction-question-progress")
            .find("interaction-question-stepper");
        assert!(stepper.visible());
        assert_eq!(stepper.role(), Some(gpui_kit::Role::List));
        assert_eq!(stepper.label(), Some("Question steps"));
        assert_eq!(window.find("question-step-counter").label(), Some("Question 1 of 3"));
        let options = window.find("interaction-question-options");
        assert_eq!(options.role(), Some(gpui_kit::Role::RadioGroup));
        assert_eq!(options.label(), Some("Choose a workspace"));
        let workspace_choice = window.find("interaction-question-choice-0");
        assert_eq!(workspace_choice.role(), Some(gpui_kit::Role::RadioButton));
        assert_eq!(workspace_choice.selected(), Some(false));
        assert!(window.try_find("previous-question").is_none());
        window.click("continue-question", cx);
        window.render_frame(cx);
        assert_eq!(window.find("question-step-counter").label(), Some("Question 1 of 3"));
        assert!(requests.try_recv().is_err(), "an unanswered step cannot submit or advance");
        view.update(cx, |view, cx| {
            view.question_answer
                .update(cx, |input, cx| input.set_value("Original workspace answer", window, cx));
        });
        window.click("interaction-question-choice-0", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).interaction_choice_ids[0].as_deref(),
            Some("workspace"),
            "selecting a radio must update VT Code's controlled interaction state"
        );
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(true));
        assert_eq!(window.find("interaction-question-choice-1").selected(), Some(false));
        window.click("interaction-question-choice-1", cx);
        window.render_frame(cx);
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(false));
        assert_eq!(window.find("interaction-question-choice-1").selected(), Some(true));
        assert_eq!(
            view.read(cx).interaction_choice_ids[0].as_deref(),
            Some("another-workspace"),
            "selecting another radio must update the controlled interaction state"
        );
        window.click("interaction-question-choice-0", cx);
        window.render_frame(cx);
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(true));
        assert_eq!(window.find("interaction-question-choice-1").selected(), Some(false));
        window.click("continue-question", cx);
        window.render_frame(cx);
        assert_eq!(window.find("question-step-counter").label(), Some("Question 2 of 3"));
        view.update(cx, |view, cx| {
            view.question_answer
                .update(cx, |input, cx| input.set_value("Task draft", window, cx));
        });
        window.click("previous-question", cx);
        window.render_frame(cx);
        assert_eq!(window.find("question-step-counter").label(), Some("Question 1 of 3"));
        assert_eq!(view.read(cx).question_answer.read(cx).value(), "Original workspace answer");
        assert_eq!(window.find("interaction-question-choice-0").selected(), Some(true));
        view.update(cx, |view, cx| {
            view.question_answer
                .update(cx, |input, cx| input.set_value("Revised workspace answer", window, cx));
        });
        window.click("continue-question", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).question_answer.read(cx).value(), "Task draft");
        window.click("interaction-question-choice-0", cx);
        window.click("continue-question", cx);
        window.render_frame(cx);
        assert_eq!(window.find("question-step-counter").label(), Some("Question 3 of 3"));
        window.click("interaction-question-choice-0", cx);
        window.click("continue-question", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("completed question response") else {
        panic!("expected interaction response");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::INTERACTION_RESPOND);
    assert_eq!(params["choiceIds"], json!(["workspace", "task", "review"]));
    assert_eq!(params["answerTexts"], json!(["Revised workspace answer", "Task draft", null]));
    assert!(requests.try_recv().is_err(), "editing questions sends only the final response");
}

#[gpui_kit::test]
fn startup_interaction_refreshes_webmcp_availability_on_question_and_answer(cx: &mut TestAppContext) {
    use crate::desktop::transport::InteractionResponseIdentity;
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};
    let (_window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session-old".into());
        view.webmcp_status = Some(connected_webmcp_status());
        view.handle_backend_event(
            BackendEvent::Interaction(InteractionRequest {
                session_id: "session-old".into(),
                turn_id: "session-start".into(),
                interaction_id: "question".into(),
                kind: InteractionKind::Question,
                wizard: false,
                title: "Question".into(),
                details: Vec::new(),
                choices: Vec::new(),
                steps: Vec::new(),
                selected_choice_id: None,
                selected_choice_ids: Vec::new(),
            }),
            cx,
        );
        assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
        let mut busy = connected_webmcp_status();
        busy.turns_available = false;
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WEBMCP_STATUS.into(),
                result: Some(serde_json::to_value(busy).unwrap()),
                error: None,
            },
            cx,
        );
        view.pending_interaction_response = Some("question".into());
        view.handle_backend_event(
            BackendEvent::InteractionResponded {
                identity: InteractionResponseIdentity {
                    session_id: "session-old".into(),
                    turn_id: "session-start".into(),
                    interaction_id: "question".into(),
                    cancelled: true,
                },
                error: None,
            },
            cx,
        );
        assert!(view.interaction.is_none());
        assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
    });
    for _ in 0..2 {
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("status request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    }
}
