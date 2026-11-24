//! Composer caret policy: Blink/Steady choice and bounded idle tracking.

use super::*;

#[gpui_kit::test]
fn caret_blink_choice_gates_the_pulse_and_persists(cx: &mut TestAppContext) {
    use crate::desktop::preferences::{CaretBlinkPreference, Preferences};

    cx.update(|cx| cx.set_reduce_motion(false));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.preferences.caret_blink, CaretBlinkPreference::Blink);
            view.mark_caret_active(cx);
            assert!(view.caret_pulse_active, "Blink starts the bounded pulse on activity");

            view.set_caret_blink(CaretBlinkPreference::Steady, cx);
            assert!(!view.caret_pulse_active, "Steady cancels a running pulse");
            assert_eq!(Preferences::load().caret_blink, CaretBlinkPreference::Steady);
            view.mark_caret_active(cx);
            assert!(!view.caret_pulse_active, "Steady ignores later activity");

            view.set_caret_blink(CaretBlinkPreference::Blink, cx);
            view.mark_caret_active(cx);
            assert!(view.caret_pulse_active);
            view.mark_caret_inactive(cx);
            assert!(!view.caret_pulse_active, "blur drops the pulse without waiting for the idle timer");
        });
        cx.set_reduce_motion(true);
        view.update(cx, |view, cx| {
            view.mark_caret_active(cx);
            assert!(!view.caret_pulse_active, "reduced motion keeps the caret solid");
        });
        cx.set_reduce_motion(false);
        view.update(cx, |view, cx| {
            view.mark_caret_active(cx);
            assert!(view.caret_pulse_active, "an active Blink composer starts a fresh pulse");
        });
    })
    .unwrap();

    // Both sides of the idle boundary, entered with a live pulse.
    cx.executor().advance_clock(Duration::from_millis(4900));
    cx.run_until_parked();
    cx.update_entity(&view, |view, _| assert!(view.caret_pulse_active, "the pulse survives until idle"));
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update_entity(&view, |view, _| {
        assert!(!view.caret_pulse_active, "an idle composer drops the pulse and its frames");
    });
}

#[gpui_kit::test]
fn repeated_caret_activity_owns_one_replaceable_idle_timer(cx: &mut TestAppContext) {
    use crate::desktop::preferences::CARET_IDLE_TIMEOUT;

    cx.update(|cx| cx.set_reduce_motion(false));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            // Continuous typing emits one activity event per keystroke. Each
            // event replaces the single owned timer instead of detaching a new
            // one, so no timer accumulates behind the composer.
            for _ in 0..8 {
                view.mark_caret_active(cx);
                assert!(view.caret_pulse_task.is_some(), "activity must leave exactly one owned idle timer");
            }
            assert!(view.caret_pulse_active);
            view.mark_caret_inactive(cx);
            assert!(view.caret_pulse_task.is_none(), "blur cancels the owned timer instead of leaving it pending");
        });
    })
    .unwrap();

    // The surviving timer still retires a later pulse at its idle deadline;
    // cancelling the earlier ones must not disable the policy.
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.mark_caret_active(cx));
    })
    .unwrap();
    cx.executor().advance_clock(CARET_IDLE_TIMEOUT + Duration::from_secs(1));
    cx.run_until_parked();
    cx.update_entity(&view, |view, _| {
        assert!(!view.caret_pulse_active, "the remaining idle timer still ends the pulse");
    });
}
