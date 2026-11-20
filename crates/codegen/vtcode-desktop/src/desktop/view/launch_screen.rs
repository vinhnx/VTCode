//! Launch screen: the V monogram over a shimmer status line, painted while
//! the backend transport is up but the handshake has not completed. A minimum
//! dwell keeps the brand frame from flashing for a single frame on fast
//! starts; reduced motion collapses the shimmer to a static readable label
//! (see [`shimmer_label`]).

use std::time::Duration;

use super::*;

/// Minimum time the launch screen stays up once the window is open, so a
/// fast handshake never flashes the splash away mid-paint.
const LAUNCH_SPLASH_MIN_DISPLAY: Duration = Duration::from_millis(400);

impl DesktopView {
    /// The splash is live while the transport exists and the handshake has
    /// not completed, and through the minimum dwell after a fast handshake.
    /// Without a transport (no workspace, or the service failed to launch)
    /// the normal welcome/error surfaces paint instead.
    pub(super) fn launch_screen_active(&self) -> bool {
        if self.backend.is_none() {
            return false;
        }
        self.launch_handshake_pending || !self.launch_splash_expired
    }

    /// One-shot dwell timer, scheduled once at window construction. Tests
    /// drive `launch_splash_expired` directly; the simulated executor only
    /// retires the timer when a test actually parks past the dwell.
    pub(super) fn start_launch_splash_dwell(&mut self, cx: &mut Context<Self>) {
        if self.launch_dwell_task.is_some() {
            return;
        }
        self.launch_dwell_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(LAUNCH_SPLASH_MIN_DISPLAY).await;
            let _updated = view.update(cx, |view, cx| {
                view.launch_splash_expired = true;
                cx.notify();
            });
        }));
    }

    /// Full-window brand placeholder: the monochrome V mark tinted with the
    /// theme ink above the localized shimmer status line, centered on the
    /// canvas so the window reads as "starting" from the first frame. The
    /// shared mount fade keeps the appearance on the app's motion tokens and
    /// renders statically under reduced motion.
    pub(super) fn render_launch_screen(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        div()
            .id("launch-screen")
            .test_support()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(colors.background)
            .child(brand_mark("launch-logo", 64., colors.foreground))
            .child(shimmer_label(
                "launch-loading",
                rust_i18n::t!("app.workspace.starting_service").to_string(),
                colors.foreground,
                cx,
            ))
            .fade_in("launch-screen-fade", cx.theme())
            .into_any_element()
    }
}
