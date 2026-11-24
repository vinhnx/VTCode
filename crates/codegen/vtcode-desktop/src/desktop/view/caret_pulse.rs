//! Composer caret pulse policy: persisted choice and bounded idle tracking.
//!
//! The framework caret stays solid; this module owns the short, idle-stopping
//! fade window so no animation frames are requested while the composer is
//! untouched, blurred, reduced-motion, or set to Steady.

use super::*;

impl DesktopView {
    /// Retire the animation element and make any pending idle timer a no-op.
    /// Returns whether the pulse was live, so callers can avoid redundant
    /// repaints when state was already settled.
    fn retire_caret_pulse(&mut self) -> bool {
        if !self.caret_pulse_active {
            return false;
        }
        self.caret_pulse_active = false;
        self.caret_pulse_generation = self.caret_pulse_generation.wrapping_add(1);
        true
    }

    /// Blink/Steady choice for the composer caret. Steady cancels the pulse at
    /// once so no animation frames stay scheduled after the switch.
    pub(super) fn set_caret_blink(&mut self, preference: preferences::CaretBlinkPreference, cx: &mut Context<Self>) {
        if self.preferences.caret_blink == preference {
            return;
        }
        self.preferences.caret_blink = preference;
        if !preference.blinks() {
            let _retired = self.retire_caret_pulse();
        }
        self.save_preferences();
        cx.notify();
    }

    /// A blurred composer has no visible caret to fade, so stop the pulse now
    /// instead of animating an invisible caret until the idle timer fires.
    pub(super) fn mark_caret_inactive(&mut self, cx: &mut Context<Self>) {
        if self.retire_caret_pulse() {
            cx.notify();
        }
    }

    /// Restart the bounded caret-pulse idle window on composer activity.
    ///
    /// One short timer per activity burst; an older timer is ignored by its
    /// generation. Under reduced motion or a Steady preference the pulse stays
    /// off, so no animation frames are ever requested.
    pub(super) fn mark_caret_active(&mut self, cx: &mut Context<Self>) {
        if cx.reduce_motion() || !self.preferences.caret_blink.blinks() {
            self.mark_caret_inactive(cx);
            return;
        }
        // Continuous typing emits one event per keystroke. Only a transition
        // from idle to active needs a repaint; extending an open idle window
        // just refreshes the timer, so avoid dirtying the whole DesktopView.
        let was_active = self.caret_pulse_active;
        self.caret_pulse_active = true;
        let generation = self.caret_pulse_generation.wrapping_add(1);
        self.caret_pulse_generation = generation;
        if !was_active {
            cx.notify();
        }
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(preferences::CARET_IDLE_TIMEOUT).await;
            let _updated = view.update(cx, |view, cx| {
                if view.caret_pulse_active && view.caret_pulse_generation == generation {
                    let _retired = view.retire_caret_pulse();
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
