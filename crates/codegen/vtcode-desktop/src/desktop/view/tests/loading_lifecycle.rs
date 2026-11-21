use super::*;
use vtcode_desktop_protocol as protocol;

#[gpui_kit::test]
fn connecting_greeting_keeps_the_ready_geometry(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized_with_preferences_path(cx, 680., 650., isolate_preferences(), true);
    cx.update_window(window, |_, window, cx| {
        let (commands, _requests) = tokio::sync::mpsc::channel(16);
        view.update(cx, |view, _| {
            view.workspace = Some(PathBuf::from("/work/A workspace with a long name"));
            view.welcome_period = WelcomePeriod::Morning;
            view.welcome_copy_index = 0;
            view.initialize_backend(BackendHandle { commands });
        });
        for (zen, prompt_id) in [(false, "welcome-prompt"), (true, "zen-welcome-prompt")] {
            view.update(cx, |view, cx| {
                view.zen_mode = zen;
                view.session_request = Some(protocol::method::INITIALIZE);
                cx.notify();
            });
            window.render_frame(cx);
            let mark = window.find("welcome-brand-mark").bounds();
            let loading_title = window.find("workspace-connecting-title").bounds();
            assert_eq!(window.find("workspace-connecting").role(), Some(gpui_kit::Role::Status));
            assert!(window.try_find(prompt_id).is_none(), "only the loading title is exposed");

            view.update(cx, |view, cx| {
                view.session_request = None;
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(window.find("welcome-brand-mark").bounds(), mark, "logo stays fixed in Zen={zen}");
            let ready_title = window.find(prompt_id).bounds();
            assert!(
                (loading_title.center().y - ready_title.center().y).abs() <= px(0.5),
                "title stays centered in its slot"
            );
            assert!((loading_title.center().x - ready_title.center().x).abs() <= px(0.5));
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_shell_and_settings_are_available_during_backend_initialize(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let (commands, mut requests) = tokio::sync::mpsc::channel(16);
        view.update(cx, |view, _| view.initialize_backend(BackendHandle { commands }));
        assert!(requests.try_recv().is_ok(), "initialize was sent");

        window.render_frame(cx);
        assert!(window.try_find("workspace-surface").is_some());
        assert!(window.try_find("workspace-connecting").is_some());
        assert!(!view.read(cx).new_thread_available(), "the handshake still gates backend actions");

        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).current_route(), Route::Settings);
        assert!(window.try_find("workspace-connecting").is_none());

        let workspace = view.read(cx).workspace.as_ref().unwrap().display().to_string();
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: protocol::method::INITIALIZE.into(),
                    result: Some(json!(protocol::InitializeResult {
                        protocol: protocol::PROTOCOL_VERSION,
                        workspace,
                        server_version: "test".into(),
                        slash_commands: Vec::new(),
                    })),
                    error: None,
                },
                cx,
            );
            assert!(view.new_thread_available(), "the response frees the gate immediately");
            view.close_settings(window, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("workspace-surface").is_some());
        assert!(window.try_find("workspace-connecting").is_none(), "there is no minimum dwell");
    })
    .unwrap();
}

#[gpui_kit::test]
fn disconnect_events_clear_transient_loading_state(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    for event in [
        BackendEvent::LaunchFailed("service failed".into()),
        BackendEvent::Exited { status: Some(1), authentication_required: false },
        BackendEvent::Exited { status: Some(1), authentication_required: true },
    ] {
        view.update(cx, |view, cx| {
            view.running = true;
            view.ui_progress_active = true;
            view.background_activity_count = 2;
            view.handle_backend_event(event, cx);
            assert!(!view.running);
            assert!(!view.ui_progress_active);
            assert_eq!(view.background_activity_count, 0);
            assert!(view.backend.is_none());
        });
    }
}

/// A service that dies mid-install leaves the optimistic `Downloading 0 bytes`
/// phase with no terminal tick to retire it. The disconnect must fail the row
/// locally so the user can retry instead of staring at a frozen download.
#[gpui_kit::test]
fn disconnect_while_an_install_is_running_fails_the_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.update_install_state =
                vtcode_desktop_protocol::UpdateInstallState::Downloading { downloaded: 0, total: None };
            view.update_request_pending = true;
            view.update_backfill_in_flight = true;
        });
        // Swap in a fresh event channel and drop its sender: the drain loop
        // then observes the same `Disconnected` a dying service produces.
        let (sender, receiver) = crate::desktop::transport::BackendEventSender::channel(1);
        view.update(cx, |view, _| view.event_rx = receiver);
        drop(sender);
        view.update(cx, |view, cx| {
            let _ = view.drain_events(window, cx);
        });
        view.update(cx, |view, _| {
            match &view.update_install_state {
                vtcode_desktop_protocol::UpdateInstallState::Failed { message } => {
                    assert!(message.contains("disconnected"), "{message}");
                }
                other => panic!("an interrupted install must be failed locally, got {other:?}"),
            }
            assert!(!view.update_request_pending, "a check that died with the service is no longer pending");
            assert!(!view.update_backfill_in_flight, "a backfill that died with the service is no longer in flight");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn global_shimmer_covers_thinking_then_follows_the_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Thinking phase: the pending row is up and the global shimmer is on.
        assert!(window.try_find("turn-pending").is_some());
        assert!(window.try_find("sidebar-status-loading").is_some());

        // Content keeps the pending row up: the activity shimmer follows the
        // whole running turn alongside the global shimmer.
        view.update(cx, |view, cx| {
            view.handle_backend_event(agent_message_update_event("session", "turn-1", "item-1", "hello back"), cx);
            assert!(view.running);
            assert!(view.turn_content_seen);
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_some());
        assert!(window.try_find("sidebar-status-loading").is_some());

        // Turn end clears both with the turn.
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed_event("session", "turn-1"), cx);
            assert!(!view.running);
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_none());
        assert!(window.try_find("sidebar-status-loading").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn matrix_snapshots_drive_background_loading_until_terminal(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{
        ThreadEvent, VersionedThreadEvent,
        matrix::{MatrixLifecycle, MatrixSnapshot, MatrixSpec, MatrixTaskState, MatrixTaskStatus},
    };

    fn snapshot(lifecycle: MatrixLifecycle, statuses: &[MatrixTaskStatus]) -> MatrixSnapshot {
        MatrixSnapshot {
            spec: MatrixSpec {
                id: "matrix-1".into(),
                tasks: Vec::new(),
                resources: Default::default(),
            },
            lifecycle,
            tasks: statuses
                .iter()
                .enumerate()
                .map(|(index, status)| MatrixTaskState {
                    id: format!("task-{index}"),
                    status: *status,
                    attempts: Vec::new(),
                    automatic_retries: 0,
                })
                .collect(),
            generation: None,
            revision: 0,
        }
    }

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
        });
        window.render_frame(cx);
        assert!(window.try_find("sidebar-status-loading").is_none());

        let event = |snapshot: MatrixSnapshot| ThreadEventNotification {
            session_id: "session".into(),
            turn_id: None,
            event: VersionedThreadEvent::new(ThreadEvent::MatrixUpdated(Box::new(snapshot))),
        };
        // Mixed statuses: queued/assigned/executed/verified load, failed does not.
        view.update(cx, |view, _| {
            view.handle_thread_event(event(snapshot(
                MatrixLifecycle::Running,
                &[
                    MatrixTaskStatus::Queued,
                    MatrixTaskStatus::Assigned,
                    MatrixTaskStatus::Executed,
                    MatrixTaskStatus::Failed,
                    MatrixTaskStatus::Verified,
                ],
            )));
            assert_eq!(view.background_activity_count, 4);
            assert!(view.global_loading_active());
        });
        window.render_frame(cx);
        assert!(window.try_find("sidebar-status-loading").is_some());

        // Verified counts only before the matrix succeeds.
        view.update(cx, |view, _| {
            view.handle_thread_event(event(snapshot(MatrixLifecycle::Running, &[MatrixTaskStatus::Verified])));
            assert_eq!(view.background_activity_count, 1);
            view.handle_thread_event(event(snapshot(MatrixLifecycle::Succeeded, &[MatrixTaskStatus::Verified])));
            assert_eq!(view.background_activity_count, 0);
        });

        // A cancelled matrix settles every task.
        view.update(cx, |view, _| {
            view.handle_thread_event(event(snapshot(
                MatrixLifecycle::Cancelled,
                &[MatrixTaskStatus::Assigned, MatrixTaskStatus::Queued],
            )));
            assert_eq!(view.background_activity_count, 0);
            assert!(!view.global_loading_active());
        });
        window.render_frame(cx);
        assert!(window.try_find("sidebar-status-loading").is_none());
    })
    .unwrap();
}
