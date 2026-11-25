use super::*;
use std::cell::Cell;
use std::rc::Rc;

/// A download emits many progress ticks; only a real phase change may
/// invalidate the whole view. This drives the real event router and observes
/// notifications, so a duplicate tick that notifies is caught.
#[gpui_kit::test]
fn duplicate_install_progress_ticks_do_not_notify(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::UpdateInstallState;

    let (_window, view) = open_desktop(cx);
    let tick = UpdateInstallState::Downloading { downloaded: 1024, total: Some(2048) };
    // Seed the current phase directly (no notify) before arming the probe, so
    // the no-change phase is measured without any intervening render.
    view.update(cx, |view, _| view.update_install_state = tick.clone());
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });

    view.update(cx, |view, cx| {
        view.handle_backend_event(BackendEvent::UpdateInstallProgress(tick), cx);
    });
    cx.run_until_parked();
    cx.update(|_| {});
    assert_eq!(notified.get(), 0, "a duplicate download tick must not dirty the view");

    view.update(cx, |view, cx| {
        view.handle_backend_event(BackendEvent::UpdateInstallProgress(UpdateInstallState::Verifying), cx);
    });
    cx.run_until_parked();
    cx.update(|_| {});
    assert!(notified.get() >= 1, "a real phase change must notify");
    view.update(cx, |view, _| {
        assert_eq!(view.update_install_state, UpdateInstallState::Verifying);
    });
}
