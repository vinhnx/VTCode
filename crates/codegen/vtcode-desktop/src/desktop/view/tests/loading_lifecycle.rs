use super::*;

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

#[gpui_kit::test]
fn global_shimmer_covers_thinking_then_follows_the_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Thinking phase: the pending row is up and the global shimmer is on.
        assert!(window.try_find("turn-pending").is_some());
        assert!(window.try_find("sidebar-status-loading").is_some());

        // First content clears the pending row; the running turn keeps the shimmer.
        view.update(cx, |view, cx| {
            view.handle_backend_event(agent_message_update_event("session", "turn-1", "item-1", "hello back"), cx);
            assert!(view.running);
            assert!(view.turn_content_seen);
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_none());
        assert!(window.try_find("sidebar-status-loading").is_some());

        // Turn end clears the shimmer with the turn.
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed_event("session", "turn-1"), cx);
            assert!(!view.running);
        });
        window.render_frame(cx);
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
