use super::*;
use std::cell::Cell;
use std::rc::Rc;

// Handler-boundary contract: calling the router directly with an unchanged
// value must not add a redundant notify. In production the event drain also
// notifies once per batch (backend_events.rs::drain_events), so this guard
// saves a duplicate notify call, not a separate draw — GPUI coalesces them.
#[gpui_kit::test]
fn unchanged_ui_progress_does_not_notify(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| view.selected_session_id = Some("current".into()));
    // Settle bootstrap work and flush its notify effects before arming the
    // probe, so only progress events count towards these observations.
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });

    // The flag starts false: repeated "inactive" progress for the selected
    // session changes nothing, so no window invalidation may be requested.
    view.update(cx, |view, cx| {
        view.handle_backend_event(BackendEvent::UiProgress(json!({"sessionId": "current", "active": false})), cx);
        view.handle_backend_event(BackendEvent::UiProgress(json!({"sessionId": "current", "active": false})), cx);
    });
    cx.run_until_parked();
    cx.update(|_| {});
    assert_eq!(notified.get(), 0, "an unchanged progress flag must not add a notify");

    // A real transition still notifies, so the status row updates without a
    // manual refresh. Fire counts can multiply once rendered, so assert at
    // least once rather than an exact count.
    view.update(cx, |view, cx| {
        view.handle_backend_event(BackendEvent::UiProgress(json!({"sessionId": "current", "active": true})), cx);
    });
    cx.run_until_parked();
    cx.update(|_| {});
    assert!(notified.get() >= 1, "a progress transition must notify");
    view.update(cx, |view, _| assert!(view.ui_progress_active));
}
