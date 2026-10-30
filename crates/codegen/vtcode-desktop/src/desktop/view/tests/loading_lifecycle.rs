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
            view.handle_backend_event(event, cx);
            assert!(!view.running);
            assert!(!view.ui_progress_active);
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
